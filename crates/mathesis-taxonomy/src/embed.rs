//! Concept候補フレーズのembedding生成（アーキテクチャ.txt 5.4 Step 4）。
//!
//! Qdrant等の専用ベクトルDBはこの時点では導入しない——この環境には
//! Docker/GPUのいずれも入っておらず（実機で確認済み）、Qdrantは通常
//! コンテナで動かすため前提が満たせない。代わりに:
//!   - embedding自体はOllama（`https://ollama.com`、公式winget配布、
//!     信頼できる小型ソフトとして導入）のローカルAPI
//!     （`http://localhost:11434/api/embeddings`）で生成する。GPU不要、
//!     ネットワーク接続も不要（完全ローカル）。
//!   - ベクトルはpapers.dbのSQLiteにBLOBとして保存する。単発のクエリ
//!     （`nearest`、CLIの近傍プレビューや`search`コマンドのon-the-fly
//!     related検索）はO(n)の総当たりのままで十分（1クエリ対n件は元々線形）。
//!     一方、全ペアの近傍を求める`graph.rs`のクラスタ用グラフ構築と
//!     `search.rs::top_k_by_embedding`は、実データで10万〜100万論文規模の
//!     候補数（数万〜数十万）になるとO(n²)総当たりが非現実的になることが
//!     判明したため、`ann.rs`のLSH（近似最近傍探索）に置き換え済み
//!     （5.6の「実際に導入するフェーズで個別に確認しながら進める」方針の
//!     とおり、外部ベクトルDBではなく自前実装で対応した）。

use anyhow::{anyhow, Context, Result};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

// "localhost" ではなく "127.0.0.1" を直接使う。実測したところ、この環境
// （hostsファイルの localhost 静的エントリがコメントアウトされている）
// では "localhost" の名前解決自体に約1.7秒かかっており、embedding呼び出し
// 1件が2秒前後になっていた主因だった（curlでの直接計測で切り分け済み）。
// IPを直に指定すれば名前解決そのものが発生せず、1件あたり150ms前後まで
// 縮む（後段のコメント参照）。
const OLLAMA_URL: &str = "http://127.0.0.1:11434/api/embeddings";

/// 実データ（arXiv 100,000論文の候補81,460件をembed_all、workers=8で実行中）
/// で発覚したバグ: `ureq::post`（引数無しの自由関数）はプロセス全体で共有される
/// デフォルトAgentを暗黙に使うが、そのAgentの`max_idle_connections_per_host`は
/// ureq側の既定値で**1**——同じホスト（127.0.0.1:11434のOllama）へ複数スレッドが
/// 同時にリクエストを投げても、コネクションプールに乗れるのは1本だけで、
/// それ以外は使い捨てのソケットを毎回新規に開いては閉じる。数万件規模の
/// リクエストをworkers本の並列で叩き続けると、閉じたソケットがTIME_WAIT状態を
/// 抜けきる前に新規ソケットを開き続けることになり、Windowsのエフェメラル
/// ポートを枯渇させて`os error 10048`（"通常、各ソケットアドレスに対して
/// プロトコル、ネットワークアドレス、またはポートのどれか1つのみを使用できます"）
/// で失敗する——実際に39,400/81,460件処理した時点で発生した。
/// `max_idle_connections_per_host`を明示的にworkers数以上に設定した専用の
/// `Agent`を1つだけ作り、全スレッドで使い回すことで、Keep-Aliveでの
/// コネクション再利用を確実にし、この規模のリクエスト数でもソケットを
/// 使い捨てにしない。
static HTTP_AGENT: LazyLock<ureq::Agent> = LazyLock::new(|| {
    ureq::AgentBuilder::new()
        .max_idle_connections_per_host(32)
        .timeout(Duration::from_secs(60))
        .build()
});

/// Ollamaのローカルembedding APIを1回呼び出す。`ollama serve`が起動しており
/// `model` がpull済みであることが前提。
pub fn embed_text(model: &str, text: &str) -> Result<Vec<f32>> {
    let body = serde_json::json!({ "model": model, "prompt": text }).to_string();

    let resp = HTTP_AGENT
        .post(OLLAMA_URL)
        .set("Content-Type", "application/json")
        .send_string(&body)
        .map_err(|e| {
            anyhow!(
                "Ollamaへのリクエストに失敗しました: {e}\n\
                 `ollama serve` が起動しているか、`ollama pull {model}` 済みか確認してください。"
            )
        })?;

    let text_resp = resp.into_string().context("Ollama応答の読み取りに失敗")?;
    let parsed: serde_json::Value =
        serde_json::from_str(&text_resp).context("Ollama応答のJSONパースに失敗")?;

    let arr = parsed
        .get("embedding")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("Ollama応答に embedding フィールドがありません: {text_resp}"))?;

    arr.iter()
        .map(|v| {
            v.as_f64()
                .map(|f| f as f32)
                .ok_or_else(|| anyhow!("embeddingの要素が数値ではありません"))
        })
        .collect()
}

/// `phrases`全件をOllamaでembedding化する。`workers`本のスレッドで並列に
/// リクエストを投げる——実測（このマシンのOllama、`all-minilm`）で
/// 20件を逐次実行5.7秒に対し20件同時実行0.7秒と、Ollama側が複数リクエストを
/// 待たせずに捌けることを確認済み（ネットワーク往復がボトルネックで、
/// CPU/GPU側の推論自体はモデルが小さく軽いため）。10万〜100万論文規模では
/// 候補数（embedding対象のフレーズ数）も数万〜数十万に達するため、逐次実行
/// （旧実装）のままだと後段のグラフ構築より embed 自体が支配的な所要時間に
/// なりかねない。`workers<=1`または件数が`workers`未満なら、単純さを優先して
/// 逐次実行にフォールバックする。順序は保証する（結果は`phrases`と同じ順）。
///
/// エラー時の扱い: 最初に失敗したリクエストのエラーを返す（他のワーカーは
/// 現在処理中の1件を最後まで終えてから停止するため、多少の無駄打ちは
/// 許容している——`?`で即座に打ち切る逐次版と厳密には同じではないが、
/// 「1件でも失敗したら全体を失敗として報告する」という結果は変わらない）。
pub fn embed_all(
    model: &str,
    phrases: &[String],
    workers: usize,
    on_progress: impl FnMut(usize, usize) + Send,
) -> Result<Vec<(String, Vec<f32>)>> {
    embed_all_with(phrases, workers, on_progress, |phrase| embed_text(model, phrase))
}

/// `embed_all`の中核（並列実行・順序保持・エラー伝播）を、実際にOllamaへ
/// 接続する`embed_text`から切り離したテスト可能な形。本番は`embed_all`が
/// `embed_one = |phrase| embed_text(model, phrase)`を渡して呼ぶ。
fn embed_all_with(
    phrases: &[String],
    workers: usize,
    mut on_progress: impl FnMut(usize, usize) + Send,
    embed_one: impl Fn(&str) -> Result<Vec<f32>> + Sync,
) -> Result<Vec<(String, Vec<f32>)>> {
    if workers <= 1 || phrases.len() < workers {
        let mut out = Vec::with_capacity(phrases.len());
        for (i, phrase) in phrases.iter().enumerate() {
            out.push((phrase.clone(), embed_one(phrase)?));
            on_progress(i + 1, phrases.len());
        }
        return Ok(out);
    }

    let next = AtomicUsize::new(0);
    let completed = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<Vec<f32>>>> = Mutex::new(vec![None; phrases.len()]);
    let first_error: Mutex<Option<anyhow::Error>> = Mutex::new(None);
    let progress = Mutex::new(&mut on_progress);

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                if first_error.lock().unwrap().is_some() {
                    break;
                }
                let idx = next.fetch_add(1, Ordering::SeqCst);
                if idx >= phrases.len() {
                    break;
                }
                match embed_one(&phrases[idx]) {
                    Ok(v) => {
                        results.lock().unwrap()[idx] = Some(v);
                        let done = completed.fetch_add(1, Ordering::SeqCst) + 1;
                        (progress.lock().unwrap())(done, phrases.len());
                    }
                    Err(e) => {
                        first_error.lock().unwrap().get_or_insert(e);
                        break;
                    }
                }
            });
        }
    });

    if let Some(e) = first_error.into_inner().unwrap() {
        return Err(e);
    }
    let results = results.into_inner().unwrap();
    Ok(phrases
        .iter()
        .cloned()
        .zip(results.into_iter().map(|v| v.expect("every non-error slot is filled before threads finish")))
        .collect())
}

/// コサイン類似度。次元が食い違う場合は0を返す（呼び出し側のバグ検出用に
/// panicさせず、明確に「似ていない」扱いにする）。
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

/// SQLiteにBLOBとして保存するための素朴な f32→バイト列変換（リトルエンディアン）。
pub fn f32_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

pub fn bytes_to_f32(bytes: &[u8]) -> Vec<f32> {
    let (chunks, _remainder) = bytes.as_chunks::<4>();
    chunks.iter().map(|c| f32::from_le_bytes(*c)).collect()
}

/// `vectors` の中から `query` に最もコサイン類似度が高い上位 `k` 件を返す
/// （フレーズ, 類似度）のペア、類似度降順。
pub fn nearest(query: &[f32], vectors: &[(String, Vec<f32>)], k: usize) -> Vec<(String, f32)> {
    let mut scored: Vec<(String, f32)> = vectors
        .iter()
        .map(|(phrase, v)| (phrase.clone(), cosine_similarity(query, v)))
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(k);
    scored
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embed_all_with_preserves_input_order_when_run_in_parallel() {
        let phrases: Vec<String> = (0..37).map(|i| format!("phrase{i}")).collect();
        let result = embed_all_with(&phrases, 8, |_, _| {}, |p| {
            // 決定的な"embedding": フレーズ末尾の数字をベクトルの1要素目にする。
            let n: f32 = p.trim_start_matches("phrase").parse().unwrap();
            Ok(vec![n])
        })
        .unwrap();
        let recovered: Vec<String> = result.iter().map(|(p, _)| p.clone()).collect();
        assert_eq!(recovered, phrases, "output order must match input order despite parallel execution");
        for (i, (_, v)) in result.iter().enumerate() {
            assert_eq!(v, &vec![i as f32], "embedding for phrase{i} must come from the call for that exact phrase");
        }
    }

    #[test]
    fn embed_all_with_falls_back_to_sequential_when_workers_is_one() {
        let phrases = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let mut progress_calls = Vec::new();
        let result = embed_all_with(
            &phrases,
            1,
            |done, total| progress_calls.push((done, total)),
            |p| Ok(vec![p.len() as f32]),
        )
        .unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(progress_calls, vec![(1, 3), (2, 3), (3, 3)]);
    }

    #[test]
    fn embed_all_with_propagates_the_first_error() {
        let phrases: Vec<String> = (0..10).map(|i| i.to_string()).collect();
        let err = embed_all_with(&phrases, 4, |_, _| {}, |p| {
            if p == "5" {
                Err(anyhow::anyhow!("simulated failure on phrase 5"))
            } else {
                Ok(vec![0.0])
            }
        })
        .unwrap_err();
        assert!(err.to_string().contains("simulated failure on phrase 5"));
    }

    #[test]
    fn cosine_similarity_of_identical_vectors_is_one() {
        let v = vec![1.0, 2.0, 3.0];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_of_orthogonal_vectors_is_zero() {
        assert!((cosine_similarity(&[1.0, 0.0], &[0.0, 1.0])).abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_of_opposite_vectors_is_negative_one() {
        assert!((cosine_similarity(&[1.0, 0.0], &[-1.0, 0.0]) - (-1.0)).abs() < 1e-6);
    }

    #[test]
    fn f32_byte_round_trip_is_lossless() {
        let v = vec![1.0f32, -2.5, 0.0, 7.25, f32::MIN, f32::MAX];
        assert_eq!(bytes_to_f32(&f32_to_bytes(&v)), v);
    }

    #[test]
    fn nearest_returns_the_closest_k_sorted_descending() {
        let query = vec![1.0, 0.0];
        let vectors = vec![
            ("far".to_string(), vec![0.0, 1.0]),
            ("close".to_string(), vec![0.99, 0.01]),
            ("exact".to_string(), vec![1.0, 0.0]),
        ];
        let top = nearest(&query, &vectors, 2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].0, "exact");
        assert_eq!(top[1].0, "close");
    }
}

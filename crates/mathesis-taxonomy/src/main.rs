use anyhow::{anyhow, Context, Result};
use mathesis_ingest::store::PaperStore;
use mathesis_taxonomy::{
    alignment, ann, concepts, context, embed, export, graph, llm_judge, louvain, lpa, relations,
    resolve, search, store::TaxonomyStore,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

const DEFAULT_MODEL: &str = "all-minilm";
// 実測（このマシンのOllama、`all-minilm`）で20件同時実行が20件逐次実行の
// 約8倍速かった（`embed.rs::embed_all`のコメント参照）ことに基づく既定値。
// 大きすぎるとOllama側のキューイングで頭打ちになるだけなので、控えめに。
const DEFAULT_EMBED_WORKERS: usize = 8;
// Web版Explorerの「related」検索段階に載せる、候補1件あたりの近傍数と
// 類似度の下限。
//
// 0.5は文字列embedding時代の値で、文脈ベクトル（`context.rs`）では
// cos類似度の分布が違う。実データ（142,948論文）で 0.30 / 0.40 / 0.50 を
// 実際に書き出して中身を読み比べた:
//
//   0.50  moduli space → (なし) / finite field → (なし) / quantum groups → drinfeld
//   0.40  moduli space → stable curves, hitchin
//   0.30  moduli space → stable curves, hitchin, tropical semifield, zorich,
//                        special member, generalized spin curves
//         quantum groups → drinfeld, weyl group element, cartan calculus,
//                        billig, canonical basis, lie derivative
//         zeta function  → riemann, functional equations, poles,
//                        classical riemann hypothesis, good reduction, etale cohomology
//
// 0.50 では文書頻度の高い概念ほど近傍が空になる（PPMIは高頻度語ほど
// 相対的に低い値を取るため）——**最も検索される概念で関連が空になる**
// という最悪の形の欠落だった。0.30 でも綴りの変種が占める割合は1.2%と
// 悪化しない（むしろ本物の関連が増えて希釈される）。配信量は
// 12.4MB→15.5MB。
const SEARCH_NEIGHBOR_K: usize = 6;
const SEARCH_NEIGHBOR_MIN_SIM: f32 = 0.30;

fn usage() -> ! {
    eprintln!(
        "Usage:\n\
         \x20 mathesis-taxonomy extract <papers.db> [--min-df N] [--dry-run]\n\
         \x20 mathesis-taxonomy embed   <papers.db> [--model NAME] [--limit N] [--parallel N]\n\
         \x20 mathesis-taxonomy context <papers.db> [--dim N] [--min-cooccur N] [--alpha F]\n\
         \x20 mathesis-taxonomy cluster <papers.db> [--top-k N] [--min-sim F] [--min-cooccur N] [--vectors context|string]\n\
         \x20 mathesis-taxonomy relations <papers.db> [--min-sim F]\n\
         \x20 mathesis-taxonomy align   <papers.db>\n\
         \x20 mathesis-taxonomy export  <papers.db> <output.json>\n\
         \x20 mathesis-taxonomy search  <papers.db> <query> [--top-k N]\n\n\
         extract: title+abstractからConcept候補を抽出する（Phase 2）。\n\
         \x20        --dry-runでDBへの保存を省き、抽出結果の集計表示だけを行う\n\
         \x20        （rake.rs/concepts.rsの抽出ロジックを変更した際に、実データへの\n\
         \x20        影響を書き込み前に確認する用途）。\n\
         embed:   抽出済みの候補フレーズをOllama（既定モデル: {DEFAULT_MODEL}）で\n\
         \x20        embedding化して保存する（Phase 3）。事前に `ollama serve` の起動と\n\
         \x20        `ollama pull {DEFAULT_MODEL}` が必要。既定で{DEFAULT_EMBED_WORKERS}並列\n\
         \x20        リクエストを投げる（`--parallel 1`で逐次実行に戻せる）。\n\
         context: 概念の**文脈ベクトル**を作る（共起のPPMI → ランダム化対称\n\
         \x20        固有分解）。Ollamaを必要とせず、純Rustで決定的。文字列を\n\
         \x20        embedしていた従来方式では関連概念の86.5%が綴りの変種に\n\
         \x20        なっていた問題への根本修正（`context.rs`冒頭参照）。\n\
         \x20        事前に `extract` が必要（`embed` は不要）。\n\
         cluster: ベクトル類似度+共起+MSC関連度で概念グラフを作り、\n\
         \x20        CPM(Constant Potts Model)でクラスタリングする（Phase 4）。\n\
         \x20        --algorithm cpm|louvain|lpa / --resolution F で切替。\n\
         \x20        --vectors context（既定、要 `context`）/ string（要 `embed`）で\n\
         \x20        ベクトルの出所を切り替えて比較できる。\n\
         relations: 型付き関係（特殊化/同値、アーキテクチャ.txt 5.3の`Relation`）を\n\
         \x20        2つの独立経路——分布的非対称包含（WeedsPrec/invCL、根拠文なし）と\n\
         \x20        Hearstパターン（論文本文の実際の一文が根拠）——から抽出し、\n\
         \x20        両方一致すればConfirmed、片方だけならGrounded/Proposedとして\n\
         \x20        保存する（`relations.rs`冒頭参照）。事前に `context` と `cluster`\n\
         \x20        （Entity Resolutionの解決結果を使うため）が必要。\n\
         align:   クラスタ単位でMSC2020とのalignmentを評価し、確信度の高い\n\
         \x20        クラスタからMSC未申告の候補へコードを伝播、MSC対応の無い\n\
         \x20        クラスタを新語彙候補として報告する（Phase 5）。事前に `cluster` が必要。\n\
         export:  Phase 1〜5の出力をWeb版Explorer用の静的JSONへ書き出す\n\
         \x20        （Phase 6）。事前に `align` が必要。出力は5ファイル:\n\
         \x20        <output.json> 本体、<output>.related.json（近傍の辺）、\n\
         \x20        <output>.papers.json（概念ごとの出典論文、題名つき）、\n\
         \x20        <output>.aliases.json（表記ゆれ）、<output>.head.json\n\
         \x20        （文書頻度上位件だけのヘッドシャード、フル索引の読み込み中の\n\
         \x20        即答用）。本体・ヘッドシャード以外はブラウザが検索時に\n\
         \x20        遅延読み込みする。型付き関係（根拠文つきのConfirmed/\n\
         \x20        Grounded）はP2以降ここでは書き出さない——\n\
         \x20        `mathesis-provenance web-export`が証拠層から\n\
         \x20        relations.jsonを直接生成する（docs/P2_STATUS.md）。\n\
         search:  exact / same concept / specialization / related の4段階で\n\
         \x20        概念候補を検索する（Phase 7）。クエリが既知の候補と完全一致しない\n\
         \x20        場合はOllamaでその場embedding化してrelated段階に使う\n\
         \x20        （失敗してもrelatedを空にするだけで他の段階は返す）。\n\
         llm-judge: `Proposed`（根拠文の無い分布的関係）候補に、ローカルLLMの\n\
         \x20        3B→7B cascadeで判定を付ける検証用コマンド（`llm_judge.rs`\n\
         \x20        冒頭参照）。DBは変更しない——結果を画面に表示するだけの\n\
         \x20        精度検証用。事前に `ollama pull qwen2.5:3b-instruct` と\n\
         \x20        `ollama pull qwen2.5:7b-instruct`、`ollama serve`起動が必要。\n\
         \x20        --limit N（既定200）件をstride抽出して判定する。"
    );
    std::process::exit(1);
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("extract") => run_extract(&args[2..]),
        Some("embed") => run_embed(&args[2..]),
        Some("context") => run_context(&args[2..]),
        Some("cluster") => run_cluster(&args[2..]),
        Some("relations") => run_relations(&args[2..]),
        Some("align") => run_align(&args[2..]),
        Some("export") => run_export(&args[2..]),
        Some("search") => run_search(&args[2..]),
        Some("llm-judge") => run_llm_judge(&args[2..]),
        _ => usage(),
    }
}

fn flag_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn run_extract(args: &[String]) -> Result<()> {
    let db_path = PathBuf::from(args.first().unwrap_or_else(|| usage()));
    let min_df: usize = flag_value(args, "--min-df")
        .map(str::parse)
        .transpose()
        .context("--min-df の値が数値ではありません")?
        .unwrap_or(3);
    let dry_run = args.iter().any(|a| a == "--dry-run");

    let paper_store = PaperStore::open(&db_path)?;
    let papers = paper_store.list_all()?;
    if papers.is_empty() {
        return Err(anyhow!(
            "{} に論文が0件です（先に mathesis-ingest で収集してください）",
            db_path.display()
        ));
    }
    println!(
        "{}件の論文からConcept候補を抽出します（min_df={min_df}{}）…",
        papers.len(),
        if dry_run { "、--dry-run: DBへは保存しません" } else { "" }
    );

    let started = Instant::now();
    let result = concepts::extract_candidates(&papers, min_df);
    let elapsed = started.elapsed();

    if !dry_run {
        let mut taxonomy_store = TaxonomyStore::open(&db_path)?;
        taxonomy_store.replace_all(&result)?;

        // 候補集合が変わった場合、もう存在しないフレーズのembeddingが孤児として
        // 残ることがある（実際にこの環境で発見・修正したバグ）。extractのたびに
        // 掃除しておく。
        let pruned = taxonomy_store.prune_orphaned_embeddings()?;
        if pruned > 0 {
            println!("(以前のembeddingのうち、今回の候補集合に存在しない{pruned}件を削除しました)");
        }
    }

    let total = result.candidates.len();
    let grounded = result.candidates.iter().filter(|c| c.msc_code.is_some()).count();
    println!(
        "{:.1}秒で候補{total}件（MSC2020に既存: {grounded}件, {:.0}% / 未知の語彙: {}件）{}",
        elapsed.as_secs_f64(),
        100.0 * grounded as f64 / total.max(1) as f64,
        total - grounded,
        if dry_run {
            "（--dry-run: 保存していません）".to_string()
        } else {
            format!("を {} に保存しました", db_path.display())
        }
    );

    let scored = result.candidates.iter().filter(|c| c.field_concentration.is_some()).count();
    println!(
        "分野集中度: {}件を判定（分野ラベル付き論文が{}件以上の候補のみ）、うち{}件を定型句として除外（閾値 {}）",
        scored + result.dropped_as_boilerplate.len(),
        mathesis_taxonomy::concentration::MIN_LABELED_PAPERS,
        result.dropped_as_boilerplate.len(),
        mathesis_taxonomy::concentration::GENERIC_MAX_CONCENTRATION,
    );

    // 何を落としたかを必ず見せる。手書きのブロックリスト時代は「今どの
    // フレーズを除外しているか」がソースを読まないと分からなかった。
    println!("\n定型句として除外した候補の上位25件（分野集中度の低い順）:");
    for c in result.dropped_as_boilerplate.iter().take(25) {
        println!(
            "  [集中度{:.3}, {:>5}件] {}",
            c.field_concentration.unwrap_or(0.0),
            c.doc_freq,
            c.phrase
        );
    }

    // 単語数で分けて表示する: 1語の候補は「広い分野名」と「抄録の定型語の
    // 残り」が混じりやすく、複合語（2語以上）の方が専門用語らしさの精度が
    // 高い。両方見せた上で、複合語を主役として扱う。
    println!("\n複合語（2語以上）の文書頻度上位20件（専門用語らしさが高い候補）:");
    for c in result.candidates.iter().filter(|c| c.word_count >= 2).take(20) {
        let msc = c.msc_code.as_deref().unwrap_or("-");
        println!(
            "  [{:>4}件, score={:>5.1}, 集中度{}, msc={msc:<7}] {}",
            c.doc_freq,
            c.mean_score,
            format_concentration(c.field_concentration),
            c.phrase
        );
    }

    println!("\n単語1語の候補・文書頻度上位15件（広い分野名や汎用語が中心）:");
    for c in result.candidates.iter().filter(|c| c.word_count == 1).take(15) {
        let msc = c.msc_code.as_deref().unwrap_or("-");
        println!(
            "  [{:>4}件, 集中度{}, msc={msc:<7}] {}",
            c.doc_freq,
            format_concentration(c.field_concentration),
            c.phrase
        );
    }

    println!("\nMSC2020に未収載の複合語候補（文書頻度上位20件・新規terminologyの発見例）:");
    for c in result
        .candidates
        .iter()
        .filter(|c| c.word_count >= 2 && c.msc_code.is_none())
        .take(20)
    {
        println!("  [{:>4}件, score={:>5.1}] {}", c.doc_freq, c.mean_score, c.phrase);
    }

    Ok(())
}

/// 分野集中度の表示。証拠不足でスコアが無い候補は「-」。
fn format_concentration(value: Option<f32>) -> String {
    match value {
        Some(v) => format!("{v:.2}"),
        None => "   -".to_string(),
    }
}

fn run_embed(args: &[String]) -> Result<()> {
    let db_path = PathBuf::from(args.first().unwrap_or_else(|| usage()));
    let model = flag_value(args, "--model").unwrap_or(DEFAULT_MODEL).to_string();
    let limit: Option<usize> = flag_value(args, "--limit")
        .map(str::parse)
        .transpose()
        .context("--limit の値が数値ではありません")?;
    let workers: usize = flag_value(args, "--parallel")
        .map(str::parse)
        .transpose()
        .context("--parallel の値が数値ではありません")?
        .unwrap_or(DEFAULT_EMBED_WORKERS);

    let mut store = TaxonomyStore::open(&db_path)?;
    let mut phrases = store.list_candidate_phrases()?;
    if phrases.is_empty() {
        return Err(anyhow!(
            "{} にConcept候補が0件です（先に `mathesis-taxonomy extract` を実行してください）",
            db_path.display()
        ));
    }
    if let Some(n) = limit {
        phrases.truncate(n);
    }

    println!(
        "{}件のフレーズをOllama（model={model}, workers={workers}）でembedding化します…",
        phrases.len()
    );
    let started = Instant::now();

    let embeddings = embed::embed_all(&model, &phrases, workers, |done, total| {
        if done % 200 == 0 || done == total {
            println!("  … {done}/{total}件");
        }
    })?;
    let elapsed = started.elapsed();

    store.save_embeddings(&model, &embeddings)?;
    println!(
        "{:.1}秒で{}件のembeddingを {} に保存しました（1件あたり平均{:.1}ms）",
        elapsed.as_secs_f64(),
        embeddings.len(),
        db_path.display(),
        elapsed.as_secs_f64() * 1000.0 / embeddings.len().max(1) as f64
    );

    // 動作確認用に、いくつかの候補について最近傍を表示する。
    let all = store.load_embeddings()?;
    println!("\n近傍検索のサンプル（コサイン類似度上位5件）:");
    for (phrase, vector) in embeddings.iter().take(5) {
        let neighbors = embed::nearest(vector, &all, 6); // 自分自身を含むので6件取って先頭を除く
        println!("  \"{phrase}\" に近い概念:");
        for (neighbor, score) in neighbors.iter().filter(|(n, _)| n != phrase).take(5) {
            println!("    {score:.3}  {neighbor}");
        }
    }

    Ok(())
}

fn run_context(args: &[String]) -> Result<()> {
    let db_path = PathBuf::from(args.first().unwrap_or_else(|| usage()));
    let mut params = context::ContextParams::default();
    if let Some(v) = flag_value(args, "--dim") {
        params.dim = v.parse().context("--dim の値が数値ではありません")?;
    }
    if let Some(v) = flag_value(args, "--min-cooccur") {
        params.min_cooccur = v.parse().context("--min-cooccur の値が数値ではありません")?;
    }
    if let Some(v) = flag_value(args, "--alpha") {
        params.context_alpha = v.parse().context("--alpha の値が数値ではありません")?;
    }

    let mut store = TaxonomyStore::open(&db_path)?;
    let phrases = store.list_candidate_phrases()?;
    if phrases.is_empty() {
        return Err(anyhow!(
            "{} にConcept候補が0件です（先に `mathesis-taxonomy extract` を実行してください）",
            db_path.display()
        ));
    }
    let index: HashMap<&str, usize> = phrases.iter().enumerate().map(|(i, p)| (p.as_str(), i)).collect();
    let links = store.load_paper_concepts()?;
    let nodes: Vec<(String, usize)> = links
        .into_iter()
        .filter_map(|(arxiv_id, phrase)| index.get(phrase.as_str()).map(|&i| (arxiv_id, i)))
        .collect();

    println!(
        "{}件の概念 × {}本の論文-概念リンクから文脈ベクトルを構築します（dim={}, min_cooccur={}, alpha={}）…",
        phrases.len(),
        nodes.len(),
        params.dim,
        params.min_cooccur,
        params.context_alpha
    );
    let started = Instant::now();
    let (vectors, stats) = context::build_context_vectors(phrases.len(), &nodes, &params);
    let elapsed = started.elapsed();

    let rows: Vec<(String, Vec<f32>)> = phrases.iter().cloned().zip(vectors).collect();
    store.save_context_vectors(&rows)?;

    println!(
        "{:.1}秒で構築し {} に保存しました（共起ペア: 採用{} / min_cooccur未満で除外{} / PPMI≤0で除外{}）",
        elapsed.as_secs_f64(),
        db_path.display(),
        stats.kept_pairs,
        stats.dropped_pairs,
        stats.zero_ppmi_pairs
    );
    // 「何が表現できなかったか」を必ず出す——共起の証拠が無い概念は
    // 零ベクトルになり、下流のグラフでは孤立点として扱われる。黙って
    // 消えるより、件数が見えている方がよい（`extract` が除外内容を
    // 必ず表示するのと同じ方針）。
    println!(
        "文脈を持てた概念: {} / {}（共起が1件も無く零ベクトルになった概念: {}）",
        stats.covered,
        phrases.len(),
        stats.isolated
    );
    Ok(())
}

/// `run_cluster`と`run_relations`の両方が要る「表記ゆれを解決し、
/// 解決済み概念ごとにベクトルをプールし、論文-概念リンクを解決済み概念の
/// 添字へ付け替える」という前処理をまとめたもの。関係抽出はクラスタリングと
/// 同じ粒度（解決済み概念）・同じ論文-概念リンクで動く必要がある——
/// 別々に再実装すると、クラスタと関係が微妙に異なる概念集合に対して
/// 計算される事故を招く。
struct ResolvedConceptSet {
    /// 解決済み概念1件ごとの代表表記。
    phrases: Vec<String>,
    /// `phrases[i]`に対応するプール済みベクトル（メンバーのdoc_freq重み付き平均）。
    embeddings: Vec<Vec<f32>>,
    doc_freqs: Vec<usize>,
    msc_codes: Vec<Option<String>>,
    /// (arxiv_id, 解決済み概念の添字)。`context::ppmi_matrix`や
    /// `graph::build_concept_graph`にそのまま渡せる形。
    paper_concept_nodes: Vec<(String, usize)>,
    /// 表記ゆれ解決の結果そのもの（`members`が生候補の添字を持つ）。
    resolved: Vec<resolve::ResolvedConcept>,
    /// 解決前の生候補フレーズ（`resolved[g].members`の添字が指す配列）。
    raw_phrases: Vec<String>,
    raw_count: usize,
}

fn resolve_and_pool(store: &TaxonomyStore, vector_source: &str) -> Result<ResolvedConceptSet> {
    // 単語1語の候補（"theory"「existence"「terms"等）は文書頻度が極端に
    // 高く、ほぼ全ての論文に出現するため共起シグナルのハブになってしまう。
    // 実際にこの環境でword_count制限なしにクラスタリングしたところ、
    // 3176件中3027件（95%）が1つの巨大クラスタに吸収される結果になった。
    // extract側でも既に「単語1語は広い分野名・定型語が混じりやすい」と
    // 位置づけている（main.rsのレポート区分）ので、対象も複合語（2語以上）
    // に絞るのが一貫している。
    let raw: Vec<(concepts::ConceptCandidate, Vec<f32>)> = match vector_source {
        "context" => store.list_candidates_with_context_vectors()?,
        "string" => store.list_candidates_with_embeddings()?,
        other => {
            return Err(anyhow!(
                "--vectors は context / string のいずれかを指定してください（指定値: {other}）"
            ))
        }
    }
    .into_iter()
    .filter(|(c, _)| c.word_count >= 2)
    .collect();
    let hint = if vector_source == "context" { "context" } else { "embed" };
    if raw.is_empty() {
        return Err(anyhow!(
            "ベクトル済みの複合語Concept候補が0件です（先に `mathesis-taxonomy {hint}` を実行してください）"
        ));
    }

    // --- Entity Resolution（`resolve.rs`）---------------------------------
    let raw_phrases: Vec<String> = raw.iter().map(|(c, _)| c.phrase.clone()).collect();
    let raw_doc_freqs: Vec<usize> = raw.iter().map(|(c, _)| c.doc_freq).collect();
    let resolved = resolve::resolve(&raw_phrases, &raw_doc_freqs);

    let dim = raw.iter().map(|(_, v)| v.len()).max().unwrap_or(0);
    let mut phrases: Vec<String> = Vec::with_capacity(resolved.len());
    let mut embeddings: Vec<Vec<f32>> = Vec::with_capacity(resolved.len());
    let mut doc_freqs: Vec<usize> = Vec::with_capacity(resolved.len());
    let mut msc_codes: Vec<Option<String>> = Vec::with_capacity(resolved.len());
    for group in &resolved {
        // グループのベクトルは、メンバーの文書頻度で重み付けた平均。
        // 表記ゆれは同じ概念なので、証拠（共起）を合算した方が、
        // 稀な表記の細いベクトルをそのまま使うより安定する。
        let mut pooled = vec![0.0f32; dim];
        for &m in &group.members {
            let w = raw[m].0.doc_freq as f32;
            for (slot, value) in raw[m].1.iter().enumerate() {
                pooled[slot] += w * value;
            }
        }
        let norm = pooled.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 1e-12 {
            pooled.iter_mut().for_each(|x| *x /= norm);
        }
        phrases.push(group.representative.clone());
        embeddings.push(pooled);
        doc_freqs.push(group.doc_freq_sum);
        // MSCコードは代表を優先し、無ければメンバーのうち最初に付いている
        // ものを使う（`members` は代表が先頭の決定的な順序）。
        msc_codes.push(group.members.iter().find_map(|&m| raw[m].0.msc_code.clone()));
    }

    // 論文-概念リンクは、メンバーの表記のどれで来ても同じグループへ寄せる。
    let names = &raw_phrases;
    let member_to_group: HashMap<&str, usize> = resolved
        .iter()
        .enumerate()
        .flat_map(|(g, group)| group.members.iter().map(move |&m| (names[m].as_str(), g)))
        .collect();

    let paper_links = store.load_paper_concepts()?;
    let paper_concept_nodes: Vec<(String, usize)> = paper_links
        .into_iter()
        .filter_map(|(arxiv_id, phrase)| member_to_group.get(phrase.as_str()).map(|&idx| (arxiv_id, idx)))
        .collect();

    let raw_count = raw.len();
    Ok(ResolvedConceptSet { phrases, embeddings, doc_freqs, msc_codes, paper_concept_nodes, resolved, raw_phrases, raw_count })
}

/// `run_export`用。`resolve_and_pool`とほぼ同じプール規則（メンバーの
/// doc_freq重み付き平均→L2正規化）だが、`run_export`は`store.list_candidates()`
/// （word_count>=2の絞り込み無し、検索索引と揃える）を単位にしているため、
/// 別に用意する——`resolve_and_pool`をそのまま使うと単語1語の概念が
/// 近傍計算から漏れる。`vectors`に無い（context/string どちらの
/// ベクトルも無い）メンバーだけの解決済み概念は出力から除く——元々
/// `vectors`に無かった候補が近傍計算から漏れるのと同じ扱い。
fn pool_vectors_by_resolution(
    resolved: &[resolve::ResolvedConcept],
    candidates: &[concepts::ConceptCandidate],
    vectors: &[(String, Vec<f32>)],
) -> Vec<(String, Vec<f32>)> {
    let vector_by_phrase: HashMap<&str, &Vec<f32>> =
        vectors.iter().map(|(p, v)| (p.as_str(), v)).collect();
    let dim = vectors.iter().map(|(_, v)| v.len()).max().unwrap_or(0);

    let mut out = Vec::with_capacity(resolved.len());
    for group in resolved {
        let mut pooled = vec![0.0f32; dim];
        let mut any_member_has_a_vector = false;
        for &m in &group.members {
            let Some(v) = vector_by_phrase.get(candidates[m].phrase.as_str()) else { continue };
            any_member_has_a_vector = true;
            let w = candidates[m].doc_freq as f32;
            for (slot, value) in v.iter().enumerate() {
                pooled[slot] += w * value;
            }
        }
        if !any_member_has_a_vector {
            continue;
        }
        let norm = pooled.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 1e-12 {
            pooled.iter_mut().for_each(|x| *x /= norm);
        }
        out.push((group.representative.clone(), pooled));
    }
    out
}

fn run_cluster(args: &[String]) -> Result<()> {
    let db_path = PathBuf::from(args.first().unwrap_or_else(|| usage()));
    let mut params = graph::GraphParams::default();
    if let Some(v) = flag_value(args, "--top-k") {
        params.embed_top_k = v.parse().context("--top-k の値が数値ではありません")?;
    }
    if let Some(v) = flag_value(args, "--min-sim") {
        params.embed_min_sim = v.parse().context("--min-sim の値が数値ではありません")?;
    }
    if let Some(v) = flag_value(args, "--min-cooccur") {
        params.min_cooccur = v.parse().context("--min-cooccur の値が数値ではありません")?;
    }
    // 既定はLouvain。LPAは比較・回帰確認のために残してある（同じグラフに
    // 対して両方を走らせ、モジュラリティで優劣を数字で比べられる）。
    if flag_value(args, "--knn").is_some_and(|v| v == "union") {
        params.mutual_knn = false;
    }
    // 既定はCPM。モジュラリティ(louvain)とLPAは比較・回帰確認のために残して
    // ある——同じグラフに対して走らせ、モジュラリティ・NMI・純度で優劣を
    // 数字で比べられる。既定の解像度は目的関数ごとに意味が違うので分ける。
    let algorithm = flag_value(args, "--algorithm").unwrap_or("cpm").to_string();
    let default_resolution = if algorithm == "cpm" {
        louvain::DEFAULT_CPM_RESOLUTION
    } else {
        louvain::DEFAULT_RESOLUTION
    };
    let resolution: f32 = flag_value(args, "--resolution")
        .map(str::parse)
        .transpose()
        .context("--resolution の値が数値ではありません")?
        .unwrap_or(default_resolution);

    // ベクトルの出所。既定は文脈ベクトル（`context.rs`）——文字列embeddingは
    // 比較・回帰確認のために `--vectors string` で選べる形で残す。
    let vector_source = flag_value(args, "--vectors").unwrap_or("context").to_string();

    let store = TaxonomyStore::open(&db_path)?;
    let ResolvedConceptSet { phrases, embeddings, doc_freqs, msc_codes, paper_concept_nodes, resolved, raw_phrases, raw_count } =
        resolve_and_pool(&store, &vector_source).map_err(|e| anyhow!("{} … {e}", db_path.display()))?;
    let names = &raw_phrases;

    println!(
        "表記ゆれの解決: {}件の候補 → {}件の概念（{}件を代表表記へ吸収）",
        raw_count,
        resolved.len(),
        raw_count - resolved.len()
    );
    println!(
        "{}件の概念からグラフを構築します（vectors={}, top_k={}, min_sim={}, min_cooccur={}, knn={}）…",
        phrases.len(),
        vector_source,
        params.embed_top_k,
        params.embed_min_sim,
        params.min_cooccur,
        if params.mutual_knn { "mutual" } else { "union" }
    );

    let started = Instant::now();
    let concept_graph = graph::build_concept_graph(&embeddings, &doc_freqs, &msc_codes, &paper_concept_nodes, &params);
    let edge_count: usize = concept_graph.adjacency.iter().map(Vec::len).sum::<usize>() / 2;
    let graph_elapsed = started.elapsed();

    let cluster_started = Instant::now();
    let labels = match algorithm.as_str() {
        "lpa" => lpa::label_propagation(&concept_graph, 50),
        "louvain" => louvain::louvain(&concept_graph, louvain::Objective::Modularity, resolution, 20),
        "cpm" => louvain::louvain(&concept_graph, louvain::Objective::Cpm, resolution, 20),
        other => {
            return Err(anyhow!(
                "--algorithm は louvain / cpm / lpa のいずれかを指定してください（指定値: {other}）"
            ))
        }
    };
    let cluster_elapsed = cluster_started.elapsed();

    let groups = lpa::group_by_label(&labels);

    let mut store = store;
    // クラスタIDは概念（解決済みグループ）単位で決まるが、DBは表記ごとに
    // 持つ——下流（align/export/search）が今までどおりフレーズを鍵に
    // 引けるようにするため。同じグループのメンバーは必ず同じIDになる。
    let assignments: Vec<(String, usize)> = resolved
        .iter()
        .enumerate()
        .flat_map(|(g, group)| {
            let label = labels[g];
            group.members.iter().map(move |&m| (names[m].clone(), label))
        })
        .collect();
    store.save_clusters(&assignments)?;

    println!(
        "グラフ構築{:.1}秒（辺{edge_count}本）+ クラスタリング{:.1}秒 → {}クラスタ を {} に保存しました",
        graph_elapsed.as_secs_f64(),
        cluster_elapsed.as_secs_f64(),
        groups.len(),
        db_path.display()
    );

    // 品質指標をアルゴリズムに依らず同じ物差しで出す。これが無いと
    // 「クラスタ数が変わった」以上のことが言えず、LPAとLouvainのどちらが
    // 良い分割なのかを数字で比べられない。
    // モジュラリティは常に γ=1 で出す（アルゴリズム間で同じ物差しにするため。
    // 最適化に使った γ で測ると、γ を変えた実行同士が比べられなくなる）。
    let q = louvain::modularity(&concept_graph, &labels, 1.0);
    let purity = msc_purity(&groups, &msc_codes);
    let nmi = msc_nmi(&labels, &msc_codes);
    println!(
        "品質: モジュラリティ Q={q:.4} / MSC-NMI {:.4} / MSC純度 {:.1}%（評価対象{}件）",
        nmi.0,
        purity.0 * 100.0,
        nmi.1
    );
    let largest = groups.iter().map(Vec::len).max().unwrap_or(0);
    println!(
        "最大クラスタ {}件（全概念の{:.1}%）",
        largest,
        100.0 * largest as f64 / phrases.len().max(1) as f64
    );

    let singleton_count = groups.iter().filter(|g| g.len() == 1).count();
    let size_2_5 = groups.iter().filter(|g| (2..=5).contains(&g.len())).count();
    let size_6_20 = groups.iter().filter(|g| (6..=20).contains(&g.len())).count();
    let size_20_plus = groups.iter().filter(|g| g.len() > 20).count();
    println!(
        "クラスタサイズ内訳: 孤立{singleton_count} / 2〜5件{size_2_5} / 6〜20件{size_6_20} / 21件以上{size_20_plus}"
    );

    println!("\n大きいクラスタ上位15件（メンバーはdoc_freq降順で最大8件表示）:");
    for group in groups.iter().filter(|g| g.len() > 1).take(15) {
        let mut members: Vec<usize> = group.clone();
        members.sort_by_key(|&i| std::cmp::Reverse(doc_freqs[i]));

        let dominant_field = dominant_msc_top_level(&members, &msc_codes);
        println!("  クラスタ({}件, 主なMSC分野: {dominant_field}):", group.len());
        for &i in members.iter().take(8) {
            let msc = msc_codes[i].as_deref().unwrap_or("-");
            println!("    [{:>4}件, msc={msc:<7}] {}", doc_freqs[i], phrases[i]);
        }
    }

    Ok(())
}

/// 分割と、著者の自己申告MSCトップレベル分野との正規化相互情報量（NMI）。
///
/// **純度だけでは分割の良し悪しを判定できない**——概念を全部バラバラの
/// 単集合にすれば純度は100%になるので、細かく割るほど得をする指標だから。
/// 実際、実データでLPA（6,350クラスタ）とLouvain γ=1（2,181クラスタ）を
/// 比べたとき、純度は65.8%対29.1%でLPAが上に見えるが、これは主に
/// クラスタ数の差を見ているだけで、分割の質の比較になっていない。
///
/// NMI = 2·I(C;L) / (H(C) + H(L)) は、割りすぎ（H(C)が大きくなる）と
/// 融合しすぎ（I(C;L)が小さくなる）の両方を罰するので、粒度の違う分割
/// 同士を公平に比べられる。1.0が完全一致、0.0が無関係。
/// 評価対象は自己申告MSCコードを持つ概念のみ。
fn msc_nmi(labels: &[usize], msc_codes: &[Option<String>]) -> (f64, usize) {
    // (クラスタ, MSCトップレベル) の同時分布を数える
    let mut joint: HashMap<(usize, String), usize> = HashMap::new();
    let mut by_cluster: HashMap<usize, usize> = HashMap::new();
    let mut by_field: HashMap<String, usize> = HashMap::new();
    let mut total = 0usize;

    for (i, code) in msc_codes.iter().enumerate() {
        let Some(code) = code else { continue };
        // `ancestor_chain` は一時的なVecを返すので、コードだけ取り出して持つ。
        let Some(field) = mathesis_msc::ancestor_chain(code).first().map(|c| c.code.clone()) else {
            continue;
        };
        *joint.entry((labels[i], field.clone())).or_default() += 1;
        *by_cluster.entry(labels[i]).or_default() += 1;
        *by_field.entry(field).or_default() += 1;
        total += 1;
    }
    if total == 0 {
        return (0.0, 0);
    }
    let n = total as f64;
    let entropy = |counts: &mut dyn Iterator<Item = usize>| -> f64 {
        counts
            .map(|c| {
                let p = c as f64 / n;
                if p > 0.0 { -p * p.ln() } else { 0.0 }
            })
            .sum()
    };
    let h_cluster = entropy(&mut by_cluster.values().copied());
    let h_field = entropy(&mut by_field.values().copied());
    if h_cluster + h_field <= 0.0 {
        return (0.0, total);
    }
    let mutual: f64 = joint
        .iter()
        .map(|((c, f), &count)| {
            let p_cf = count as f64 / n;
            let p_c = by_cluster[c] as f64 / n;
            let p_f = by_field[f] as f64 / n;
            p_cf * (p_cf / (p_c * p_f)).ln()
        })
        .sum();
    (2.0 * mutual / (h_cluster + h_field), total)
}

/// 分割全体のMSC純度: 自己申告MSCコードを持つ概念のうち、自分の所属
/// クラスタの多数派MSCトップレベル分野と一致したものの割合。
/// 単独では細かく割るほど得をするので、必ず `msc_nmi` と併せて見ること。
/// 戻り値は (純度, 評価対象となった概念数)。
fn msc_purity(groups: &[Vec<usize>], msc_codes: &[Option<String>]) -> (f64, usize) {
    let mut matched = 0usize;
    let mut total = 0usize;
    for group in groups {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for &i in group {
            if let Some(code) = &msc_codes[i] {
                if let Some(top) = mathesis_msc::ancestor_chain(code).first() {
                    *counts.entry(top.code.as_str()).or_default() += 1;
                }
            }
        }
        let grounded: usize = counts.values().sum();
        if grounded == 0 {
            continue;
        }
        total += grounded;
        matched += counts.values().copied().max().unwrap_or(0);
    }
    if total == 0 {
        return (0.0, 0);
    }
    (matched as f64 / total as f64, total)
}

/// クラスタ内で最も多く出現したMSCトップレベル分野の名前を返す（該当が
/// 1つもなければ「-」）。クラスタが実際に意味的にまとまっているかを
/// 目視確認するための簡易指標。
fn dominant_msc_top_level(members: &[usize], msc_codes: &[Option<String>]) -> String {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for &i in members {
        if let Some(code) = &msc_codes[i] {
            if let Some(top) = mathesis_msc::ancestor_chain(code).first() {
                *counts.entry(top.name.as_str()).or_default() += 1;
            }
        }
    }
    match counts.into_iter().max_by_key(|&(_, count)| count) {
        Some((name, count)) => format!("{name} ({count}/{}件)", members.len()),
        None => "-".to_string(),
    }
}

fn run_relations(args: &[String]) -> Result<()> {
    let db_path = PathBuf::from(args.first().unwrap_or_else(|| usage()));
    // クラスタリングと同じ`--min-sim`の教訓（文字列embedding時代の0.5は
    // 文脈ベクトルには強すぎる）がここにも当てはまるので、既定値を
    // 個別に持たせて実測しながら決められるようにする。
    let min_sim: f32 = flag_value(args, "--min-sim")
        .map(str::parse)
        .transpose()
        .context("--min-sim の値が数値ではありません")?
        .unwrap_or(0.35);

    let store = TaxonomyStore::open(&db_path)?;
    let pooled = resolve_and_pool(&store, "context").map_err(|e| anyhow!("{} … {e}", db_path.display()))?;
    println!(
        "{}件の解決済み概念のうち、cos類似度{min_sim}以上のペアを分布的非対称包含で判定します…",
        pooled.phrases.len()
    );

    // --- 経路1: 分布的非対称包含 -------------------------------------
    let (ppmi, ppmi_stats) =
        context::ppmi_matrix(pooled.phrases.len(), &pooled.paper_concept_nodes, &context::ContextParams::default());
    let adjacency = relations::build_adjacency(&ppmi);
    let dist_params = relations::DistributionalParams { equivalent_min: 0.85, specialization_min: 0.35, margin: 0.12 };

    let candidate_pairs =
        ann::candidate_pairs_above_threshold(pooled.phrases.len(), |i| pooled.embeddings[i].as_slice(), min_sim);
    let mut distributional: Vec<(String, String, relations::RelationKind, f32)> = Vec::new();
    for (i, j, _sim) in &candidate_pairs {
        if let Some((subject, object, kind, confidence)) = relations::classify_pair(&adjacency, *i, *j, &dist_params) {
            distributional.push((pooled.phrases[subject].clone(), pooled.phrases[object].clone(), kind, confidence));
        }
    }
    println!(
        "  分布的経路: 候補ペア{}件（共起グラフ辺{}本）→ 判定{}件",
        candidate_pairs.len(),
        ppmi.entries.len(),
        distributional.len()
    );

    // --- 経路2: Hearstパターン -----------------------------------------
    // 解決済み概念の代表表記へ辿るための、生フレーズ→代表表記の逆引き。
    // `resolve_and_pool`が既に計算した表と同じ規則で再構築する（`resolved`
    // と`raw_phrases`をそのまま持ち帰ってきているので計算し直す必要は
    // ない——単に逆引きの形にするだけ）。
    let raw_names = &pooled.raw_phrases;
    let phrase_to_representative: HashMap<&str, &str> = pooled
        .resolved
        .iter()
        .flat_map(|g| g.members.iter().map(move |&m| (raw_names[m].as_str(), g.representative.as_str())))
        .collect();

    // Hearst抽出は論文ごとに独立なので、その論文が実際にリンクしている
    // 複合語候補（`word_count >= 2`、`cluster`が対象を複合語に絞るのと
    // 同じ理由）だけを対象にする。
    let word_count_ge2: std::collections::HashSet<String> =
        store.list_candidates()?.into_iter().filter(|c| c.word_count >= 2).map(|c| c.phrase).collect();
    let mut phrases_by_paper: HashMap<String, Vec<String>> = HashMap::new();
    for (arxiv_id, phrase) in store.load_paper_concepts()? {
        if word_count_ge2.contains(&phrase) {
            phrases_by_paper.entry(arxiv_id).or_default().push(phrase);
        }
    }

    let papers = PaperStore::open(&db_path)?.list_all()?;
    let mut hearst: Vec<relations::HearstHit> = Vec::new();
    let mut papers_with_evidence = 0usize;
    for paper in &papers {
        let Some(links) = phrases_by_paper.get(&paper.arxiv_id) else { continue };
        if links.len() < 2 {
            continue;
        }
        let hits = relations::extract_hearst_hits(paper, links);
        if !hits.is_empty() {
            papers_with_evidence += 1;
        }
        for hit in hits {
            let (Some(&subject), Some(&object)) =
                (phrase_to_representative.get(hit.subject.as_str()), phrase_to_representative.get(hit.object.as_str()))
            else {
                continue;
            };
            if subject == object {
                continue; // 表記ゆれ同士（同じ解決済み概念）は関係にしない。
            }
            hearst.push(relations::HearstHit {
                subject: subject.to_string(),
                object: object.to_string(),
                kind: hit.kind,
                sentence: hit.sentence,
                arxiv_id: hit.arxiv_id,
            });
        }
    }
    println!("  Hearst経路: {}論文を走査、根拠文を持つ論文{papers_with_evidence}件・関係{}件", papers.len(), hearst.len());

    let edges = relations::merge(distributional, hearst);
    let confirmed = edges.iter().filter(|e| e.status == relations::RelationStatus::Confirmed).count();
    let grounded = edges.iter().filter(|e| e.status == relations::RelationStatus::Grounded).count();
    let proposed = edges.iter().filter(|e| e.status == relations::RelationStatus::Proposed).count();
    let specialization = edges.iter().filter(|e| e.kind == relations::RelationKind::SpecializationOf).count();
    let equivalent = edges.iter().filter(|e| e.kind == relations::RelationKind::EquivalentTo).count();

    let mut store = store;
    store.save_relations(&edges)?;
    println!(
        "関係{}件（特殊化{specialization}・同値{equivalent}）を{}に保存しました\n  内訳: Confirmed（両経路一致）{confirmed} / Grounded（根拠文あり）{grounded} / Proposed（統計のみ）{proposed}",
        edges.len(),
        db_path.display()
    );

    // 目視確認用に、Confirmed（最も信頼できる）を全件、Grounded・Proposedは
    // 上位だけ表示する——`extract`が除外内容を必ず見せるのと同じ方針で、
    // 「何を確信度付きで出力したか」をソースを読まずに確認できるように。
    println!("\nConfirmed（両経路が一致、最も信頼できる）:");
    for e in edges.iter().filter(|e| e.status == relations::RelationStatus::Confirmed) {
        print_relation(e);
    }
    println!("\nGrounded（根拠文はあるが分布的には未確認）上位10件:");
    for e in edges.iter().filter(|e| e.status == relations::RelationStatus::Grounded).take(10) {
        print_relation(e);
    }
    println!("\nProposed（統計のみ、根拠文なし）confidence上位10件:");
    let mut proposed_sorted: Vec<&relations::RelationEdge> =
        edges.iter().filter(|e| e.status == relations::RelationStatus::Proposed).collect();
    proposed_sorted.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
    for e in proposed_sorted.into_iter().take(10) {
        print_relation(e);
    }

    if ppmi_stats.isolated > 0 {
        println!(
            "\n(共起の証拠が無く分布的判定の対象外だった概念: {}件——`context`実行時と同じ孤立概念)",
            ppmi_stats.isolated
        );
    }

    Ok(())
}

/// `llm_judge.rs`冒頭コメント参照。DBは変更しない検証用コマンド——
/// 3B→7B cascadeの実データ精度をこの規模で再確認してから、Web側への
/// 反映（新しいstatusの追加）へ進むかどうかを判断するためのもの。
fn run_llm_judge(args: &[String]) -> Result<()> {
    let db_path = PathBuf::from(args.first().unwrap_or_else(|| usage()));
    let limit: usize =
        flag_value(args, "--limit").map(str::parse).transpose().context("--limit の値が数値ではありません")?.unwrap_or(200);

    let store = TaxonomyStore::open(&db_path)?;
    let all = store.load_relations()?;
    let proposed: Vec<&relations::RelationEdge> =
        all.iter().filter(|e| e.status == relations::RelationStatus::Proposed).collect();
    if proposed.is_empty() {
        return Err(anyhow!("`Proposed`の関係候補が0件です（先に `relations` を実行してください）"));
    }

    // 単純な無作為抽出ではなく、一定間隔で抜き出すstride抽出——外部の
    // 乱数生成器を導入せずに済ませつつ、DBの挿入順（`subject`のABC順、
    // `save_relations`参照）に対する偏りを避ける狙い。真の無作為性は
    // 求めていない——精度の検証用サンプルとして偏りが小さければ十分。
    let stride = (proposed.len() / limit.max(1)).max(1);
    let sample: Vec<&relations::RelationEdge> = proposed.iter().step_by(stride).take(limit).copied().collect();

    println!(
        "`Proposed`候補{}件からstride={stride}で{}件を抽出し、3B→7B cascadeで判定します…",
        proposed.len(),
        sample.len()
    );

    let started = Instant::now();
    let mut correct = 0usize;
    let mut incorrect = 0usize;
    let mut malformed = 0usize;
    let mut escalated = 0usize;
    for (i, e) in sample.iter().enumerate() {
        let outcome = llm_judge::judge_cascade(&e.subject, e.kind, &e.object)?;
        match outcome.verdict {
            llm_judge::Verdict::Correct => correct += 1,
            llm_judge::Verdict::Incorrect => incorrect += 1,
            llm_judge::Verdict::Malformed => malformed += 1,
        }
        if outcome.escalated {
            escalated += 1;
        }
        let kind = match e.kind {
            relations::RelationKind::SpecializationOf => "⊂",
            relations::RelationKind::EquivalentTo => "≡",
        };
        let verdict = match outcome.verdict {
            llm_judge::Verdict::Correct => "correct",
            llm_judge::Verdict::Incorrect => "incorrect",
            llm_judge::Verdict::Malformed => "malformed",
        };
        let escalation_mark = if outcome.escalated { "[7B]" } else { "[3B]" };
        println!(
            "  [{}/{}] {escalation_mark} {verdict:>9} — \"{}\" {kind} \"{}\"",
            i + 1,
            sample.len(),
            e.subject,
            e.object
        );
    }

    let elapsed = started.elapsed();
    println!(
        "\n{:.1}秒で{}件判定（{:.1}秒/件）: correct {correct}件・incorrect {incorrect}件・malformed {malformed}件\n\
         7Bへ再確認のため昇格した件数: {escalated}件（{:.1}%）\n\
         （DBへの保存は行っていません——精度の検証のみ）",
        elapsed.as_secs_f64(),
        sample.len(),
        elapsed.as_secs_f64() / sample.len().max(1) as f64,
        100.0 * escalated as f64 / sample.len().max(1) as f64,
    );

    Ok(())
}

fn print_relation(e: &relations::RelationEdge) {
    let kind = match e.kind {
        relations::RelationKind::SpecializationOf => "⊂",
        relations::RelationKind::EquivalentTo => "≡",
    };
    let status = match e.status {
        relations::RelationStatus::Confirmed => "confirmed",
        relations::RelationStatus::Grounded => "grounded",
        relations::RelationStatus::Proposed => "proposed",
    };
    print!("  {} {kind} {}  [{status}, conf={:.2}]", e.subject, e.object, e.confidence);
    if let (Some(sentence), Some(arxiv_id)) = (&e.evidence_sentence, &e.evidence_arxiv_id) {
        print!("\n      根拠 ({arxiv_id}): {sentence}");
    }
    println!();
}

fn run_align(args: &[String]) -> Result<()> {
    let db_path = PathBuf::from(args.first().unwrap_or_else(|| usage()));

    let mut store = TaxonomyStore::open(&db_path)?;
    let cluster_assignments = store.load_clusters()?;
    if cluster_assignments.is_empty() {
        return Err(anyhow!(
            "{} にクラスタが0件です（先に `mathesis-taxonomy cluster` を実行してください）",
            db_path.display()
        ));
    }

    let candidates = store.list_candidates()?;
    let msc_by_phrase: HashMap<&str, Option<&str>> =
        candidates.iter().map(|c| (c.phrase.as_str(), c.msc_code.as_deref())).collect();
    let doc_freq_by_phrase: HashMap<&str, usize> =
        candidates.iter().map(|c| (c.phrase.as_str(), c.doc_freq)).collect();

    // 生のcluster_id（連番とは限らない）でメンバーをまとめる。
    let mut members_by_cluster: HashMap<usize, Vec<String>> = HashMap::new();
    for (phrase, cid) in &cluster_assignments {
        members_by_cluster.entry(*cid).or_default().push(phrase.clone());
    }

    let mut alignments = Vec::with_capacity(members_by_cluster.len());
    let mut all_inferred = Vec::new();

    for (&cluster_id, phrases) in &members_by_cluster {
        let members: Vec<(String, Option<String>)> = phrases
            .iter()
            .map(|p| (p.clone(), msc_by_phrase.get(p.as_str()).copied().flatten().map(str::to_string)))
            .collect();
        let member_codes: Vec<Option<String>> = members.iter().map(|(_, c)| c.clone()).collect();

        let alignment = alignment::align_cluster(cluster_id, phrases.len(), &member_codes);
        let inferred = alignment::propagate(&alignment, &members);
        all_inferred.extend(inferred);
        alignments.push(alignment);
    }

    store.save_alignment(&alignments, &all_inferred)?;

    let novel_count = alignments.iter().filter(|a| a.is_novel()).count();
    let confident_count = alignments.iter().filter(|a| a.is_confident()).count();
    let scattered_count = alignments.len() - novel_count - confident_count;
    println!(
        "{}クラスタを評価: MSC整合{confident_count} / 割れている{scattered_count} / MSC対応なし（新語彙候補）{novel_count}",
        alignments.len()
    );
    println!("MSC未申告だった候補のうち{}件に、所属クラスタの多数派コードを推定として付与しました", all_inferred.len());

    let mut confident_sorted: Vec<&alignment::ClusterAlignment> = alignments.iter().filter(|a| a.is_confident()).collect();
    confident_sorted.sort_by_key(|a| std::cmp::Reverse(a.size));
    println!("\n整合が取れたクラスタ上位10件（サイズ降順）:");
    for a in confident_sorted.iter().take(10) {
        let name = a.dominant_name.as_deref().unwrap_or("?");
        let code = a.dominant_code.as_deref().unwrap_or("?");
        println!(
            "  クラスタ#{}（{}件, grounded {}/{}, 確信度{:.0}%）: {code} {name}",
            a.cluster_id, a.size, a.grounded_count, a.size, a.confidence * 100.0
        );
    }

    let mut novel_sorted: Vec<&alignment::ClusterAlignment> = alignments.iter().filter(|a| a.is_novel() && a.size > 1).collect();
    novel_sorted.sort_by_key(|a| std::cmp::Reverse(a.size));
    println!("\nMSC2020に対応の無い新語彙候補クラスタ上位10件（サイズ降順、メンバーはdoc_freq降順で最大6件表示）:");
    for a in novel_sorted.iter().take(10) {
        let mut phrases = members_by_cluster[&a.cluster_id].clone();
        phrases.sort_by_key(|p| std::cmp::Reverse(doc_freq_by_phrase.get(p.as_str()).copied().unwrap_or(0)));
        println!("  クラスタ#{}（{}件）:", a.cluster_id, a.size);
        for p in phrases.iter().take(6) {
            let df = doc_freq_by_phrase.get(p.as_str()).copied().unwrap_or(0);
            println!("    [{df:>4}件] {p}");
        }
    }

    Ok(())
}

fn run_export(args: &[String]) -> Result<()> {
    let db_path = PathBuf::from(args.first().unwrap_or_else(|| usage()));
    let output_path = PathBuf::from(args.get(1).unwrap_or_else(|| usage()));

    let paper_store = PaperStore::open(&db_path)?;
    let paper_count = paper_store.count()?;

    let store = TaxonomyStore::open(&db_path)?;
    let alignments = store.load_cluster_alignments()?;
    if alignments.is_empty() {
        return Err(anyhow!(
            "{} にalignment結果が0件です（先に `mathesis-taxonomy align` を実行してください）",
            db_path.display()
        ));
    }
    let candidates = store.list_candidates()?;

    // Entity Resolution（`resolve.rs`）を全ての書き出し（検索索引・related
    // 近傍・出典論文・表記ゆれ一覧）で共有する1回だけの計算にする。以前は
    // ここが無く、`search_index`・related近傍が`candidates`（解決前の生
    // 候補、実データで112,933件）をそのまま単位にしていたため、
    // "kahler manifold"/"kahler manifolds"のような表記ゆれが検索結果に
    // 別々の行として残り続けていた——クラスタリング・関係抽出はとっくに
    // 解決済み概念を単位にしていた（`resolve_and_pool`）のに、利用者が
    // 実際に触る検索索引だけが取り残されていた。`resolved[g].members`は
    // この`candidates`配列への添字なので、他の書き出し関数もこの
    // `candidates`と対にして渡す。
    let all_phrases: Vec<String> = candidates.iter().map(|c| c.phrase.clone()).collect();
    let all_doc_freqs: Vec<usize> = candidates.iter().map(|c| c.doc_freq).collect();
    let resolved = resolve::resolve(&all_phrases, &all_doc_freqs);

    let mut members_by_cluster: HashMap<usize, Vec<String>> = HashMap::new();
    for (phrase, cluster_id) in store.load_clusters()? {
        members_by_cluster.entry(cluster_id).or_default().push(phrase);
    }

    // Phase 7: ブラウザ側の「related」検索段階用に、近傍を事前計算して
    // 辺リストだけ埋め込む（ベクトル本体は載せない）。
    // ベクトルは文脈ベクトル（`context.rs`）を使う——文字列embeddingでは
    // 関連リストの86.5%が同じ語の綴り違いで埋まっていた。文脈ベクトルが
    // 無いDBでは従来の文字列embeddingへ自動で戻す。
    let (vectors, vector_source) = {
        let ctx = store.load_context_vectors()?;
        if ctx.is_empty() {
            (store.load_embeddings()?, "string")
        } else {
            (ctx, "context")
        }
    };
    // 近傍は解決済み概念ごとにプールしたベクトルで計算する（`resolve_and_pool`
    // と同じ、メンバーのdoc_freq重み付き平均→L2正規化）。生候補のまま近傍を
    // 取ると、表記ゆれの数だけほぼ同一の近傍リストを持つノードが重複して
    // 生成される——`search.rs::top_k_by_embedding_excluding`の
    // `same_concept`除外は「表記ゆれ同士が互いを近傍に出す」ことは防ぐが、
    // 表記ゆれの数だけノード自体が重複する問題までは防げない。
    let resolved_vectors = pool_vectors_by_resolution(&resolved, &candidates, &vectors);
    // 近傍の下限は文字列embedding時代の 0.5 を既定にしていたが、文脈
    // ベクトルはcos類似度の分布が違うので `--min-sim` で実測しながら
    // 決められるようにする。
    let min_sim: f32 = flag_value(args, "--min-sim")
        .map(str::parse)
        .transpose()
        .context("--min-sim の値が数値ではありません")?
        .unwrap_or(SEARCH_NEIGHBOR_MIN_SIM);
    let neighbor_k: usize = flag_value(args, "--neighbors")
        .map(str::parse)
        .transpose()
        .context("--neighbors の値が数値ではありません")?
        .unwrap_or(SEARCH_NEIGHBOR_K);
    println!(
        "{}件の{}ベクトルを{}件の解決済み概念へプールし、近傍を事前計算します（related検索用、k={neighbor_k}, min_sim={min_sim}）…",
        vectors.len(),
        vector_source,
        resolved_vectors.len()
    );
    let neighbor_started = Instant::now();
    // 表記ゆれ同士は近傍に出さない。それは「同一概念」段階の担当で、
    // 「関連概念」段階に重ねると本来出るべき別概念を押し出してしまう
    // （プール後は同じ鍵を持つノードが存在しないはずだが、念のため
    // 同じ判断基準をそのまま残す——コストはほぼゼロ）。
    let keys: Vec<String> = resolved_vectors.iter().map(|(p, _)| resolve::canonical_key(p)).collect();
    let neighbors = search::top_k_by_embedding_excluding(
        &resolved_vectors,
        neighbor_k,
        min_sim,
        |i, j| keys[i] == keys[j],
    );
    println!("  … {:.1}秒", neighbor_started.elapsed().as_secs_f64());

    let taxonomy = export::build_export(paper_count, &candidates, &resolved, &alignments, &members_by_cluster);
    // `to_string_pretty` ではなく `to_string`。この出力はブラウザが丸ごと
    // ダウンロードして `JSON.parse` する配信物であって、人が読むものでは
    // ない（読みたいときは `jq` を通せばよい）。100k論文規模の実測で
    // 整形用の空白だけが18.7MB——ファイル全体47.4MBの実に39%を占めており、
    // `fetch`の転送量とブラウザのパース時間の両方に直接効いていた。
    let json = serde_json::to_string(&taxonomy)?;
    std::fs::write(&output_path, &json)
        .with_context(|| format!("{} への書き込みに失敗しました", output_path.display()))?;

    // 近傍の辺リストは別ファイルへ。詳しい理由は
    // `export.rs::RelatedEdgesExport` のコメント——要するに、全利用者が
    // ページを開くたびにこの62%を待たされる理由が無い。
    let related_path = sidecar_output_path(&output_path, "related");
    let related = export::build_related_export(&neighbors);
    let related_json = serde_json::to_string(&related)?;
    std::fs::write(&related_path, &related_json)
        .with_context(|| format!("{} への書き込みに失敗しました", related_path.display()))?;

    // 概念ごとの出典論文（**題名つき**）も同じ理由で別ファイルへ。
    // 詳しくは `export.rs::PapersExport` のコメント。
    let papers_path = sidecar_output_path(&output_path, "papers");
    let paper_rows: Vec<export::PaperRow> = store
        .load_paper_metadata()?
        .into_iter()
        .map(|p| export::PaperRow {
            arxiv_id: p.arxiv_id,
            title: p.title,
            year: p.year,
            primary_category: p.categories.into_iter().next(),
        })
        .collect();
    let links = store.load_paper_concepts()?;
    let papers = export::build_papers_export(&resolved, &candidates, &paper_rows, &links);
    let papers_json = serde_json::to_string(&papers)?;
    std::fs::write(&papers_path, &papers_json)
        .with_context(|| format!("{} への書き込みに失敗しました", papers_path.display()))?;

    // 表記ゆれの一覧。畳んだ事実を画面に出せるようにする（黙って畳むと、
    // 利用者は自分が打った表記が出てこない理由が分からない）。上で
    // 計算済みの`resolved`をそのまま使う（この関数内での最後の使用なので
    // 借用ではなく`into_iter`で消費する）——以前はここで`resolve::resolve`
    // をもう一度呼んでおり、同じ計算を2回していた。
    let alias_path = sidecar_output_path(&output_path, "aliases");
    let alias_groups: Vec<(String, Vec<String>)> =
        resolved.into_iter().map(|g| (g.representative, g.aliases)).collect();
    let aliases = export::build_alias_export(&alias_groups);
    let aliases_json = serde_json::to_string(&aliases)?;
    std::fs::write(&alias_path, &aliases_json)
        .with_context(|| format!("{} への書き込みに失敗しました", alias_path.display()))?;

    // `relations.json`（型付き関係、根拠文を持つConfirmed/Groundedだけ）は
    // もうここでは書かない——P2（`docs/P2_STATUS.md`）以降、
    // `mathesis-provenance web-export`が証拠層から直接生成する。
    // `export::build_relations_export`/`RelationsExport`自体は残っている
    // （テスト済みの純粋関数として、他の用途に使える形で）が、web/publicへの
    // 書き出しはこのコマンドの役目ではなくなった。

    // ヘッドシャード（診断④「配信の不可分性」への対応）。文書頻度上位
    // `export::HEAD_SHARD_SIZE`件だけの小さな索引で、ブラウザは
    // フルの`searchIndex`をWorker上で読み込み終える前にこれで即答する
    // （`export.rs::build_head_shard`のコメント参照）。
    let head_path = sidecar_output_path(&output_path, "head");
    let head_shard = export::build_head_shard(&taxonomy.search_index);
    let head_json = serde_json::to_string(&head_shard)?;
    std::fs::write(&head_path, &head_json)
        .with_context(|| format!("{} への書き込みに失敗しました", head_path.display()))?;

    println!(
        "{}分野・{}クラスタ（新語彙候補{}クラスタ含む）・検索索引{}件を {} に書き出しました（{}バイト）",
        taxonomy.fields.len(),
        taxonomy.cluster_count,
        taxonomy.novel_clusters.len(),
        taxonomy.search_index.phrase.len(),
        output_path.display(),
        json.len()
    );
    println!(
        "related辺{}件（{}概念ぶん）を {} に書き出しました（{}バイト、検索時に遅延読み込みされる）",
        related.targets.iter().map(Vec::len).sum::<usize>(),
        related.source.len(),
        related_path.display(),
        related_json.len()
    );
    println!(
        "出典論文 {}件（題名つき）・概念→論文リンク{}本（{}概念ぶん）を {} に書き出しました（{}バイト、検索時に遅延読み込みされる）",
        papers.arxiv_id.len(),
        papers.papers.iter().map(Vec::len).sum::<usize>(),
        papers.source.len(),
        papers_path.display(),
        papers_json.len()
    );
    println!(
        "表記ゆれ {}グループ（別表記{}件）を {} に書き出しました（{}バイト）",
        aliases.representative.len(),
        aliases.aliases.iter().map(Vec::len).sum::<usize>(),
        alias_path.display(),
        aliases_json.len()
    );
    println!(
        "ヘッドシャード（文書頻度上位{}件）を {} に書き出しました（{}バイト、フル索引の読み込み中に即答用）",
        head_shard.phrase.len(),
        head_path.display(),
        head_json.len()
    );

    Ok(())
}

/// `taxonomy.json` → `taxonomy.related.json` / `taxonomy.papers.json`。
/// 拡張子の直前に `.<suffix>` を差し込むだけなので、出力先をどこに
/// 指定しても複数のサイドカーファイルが並んで置かれる。
fn sidecar_output_path(output: &std::path::Path, suffix: &str) -> PathBuf {
    let stem = output.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let extension = output.extension().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "json".to_string());
    output.with_file_name(format!("{stem}.{suffix}.{extension}"))
}

fn run_search(args: &[String]) -> Result<()> {
    let db_path = PathBuf::from(args.first().unwrap_or_else(|| usage()));
    let query = args.get(1).cloned().unwrap_or_else(|| usage());
    let top_k: usize =
        flag_value(args, "--top-k").map(str::parse).transpose().context("--top-k の値が数値ではありません")?.unwrap_or(10);

    let store = TaxonomyStore::open(&db_path)?;
    let candidates = store.list_candidates()?;
    if candidates.is_empty() {
        return Err(anyhow!(
            "{} にConcept候補が0件です（先に `mathesis-taxonomy extract` を実行してください）",
            db_path.display()
        ));
    }
    // クラスタリング（Phase 4）・embedding（Phase 3）は未実行でも検索自体は
    // 動く——その場合はそれぞれ same_concept / related 段階が空になるだけ
    // （extractだけ済んでいる状態でも exact / specialization は使える）。
    let cluster_of: HashMap<String, usize> = store.load_clusters()?.into_iter().collect();
    // 文脈ベクトルがあればそちらを使う。文脈ベクトルは「同じ論文に出るか」
    // から作るので、コーパスに存在しないクエリ文には作れない——その場合
    // related段階は空になる（無い証拠をでっち上げるより素直に空にする）。
    let context_vectors = store.load_context_vectors()?;
    let using_context = !context_vectors.is_empty();
    let embeddings = if using_context { context_vectors } else { store.load_embeddings()? };

    let query_lower = query.trim().to_lowercase();
    let has_exact = candidates.iter().any(|c| c.phrase == query_lower);
    let query_vector: Option<Vec<f32>> = if !has_exact && !using_context && !embeddings.is_empty() {
        match embed::embed_text(DEFAULT_MODEL, &query) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("(クエリ文をその場embedding化できなかったため、関連概念(related)の検索はスキップします: {e})");
                None
            }
        }
    } else {
        None
    };

    let result = search::hybrid_search(&query, &candidates, &cluster_of, query_vector.as_deref(), &embeddings, top_k);

    println!("クエリ: \"{query}\"");
    print_search_tier(
        "完全一致 (exact) — フレーズそのものが候補集合にあるか",
        &result.exact,
    );
    print_search_tier(
        "同一概念 (same concept) — 完全一致した候補と同じクラスタに属す概念",
        &result.same_concept,
    );
    print_search_tier(
        "特殊化 (specialization) — クエリの語をそのまま含むより具体的な複合語",
        &result.specialization,
    );
    print_search_tier(
        "関連概念 (related) — embeddingコサイン類似度による近傍",
        &result.related,
    );

    Ok(())
}

fn print_search_tier(label: &str, hits: &[search::SearchHit]) {
    println!("\n[{label}] {}件", hits.len());
    for h in hits {
        let msc = h.msc_code.as_deref().unwrap_or("-");
        println!("  [{:>4}件, score={:.3}, msc={msc:<7}] {}", h.doc_freq, h.score, h.phrase);
    }
}

//! `status: Proposed`（分布的非対称包含のみ、根拠文なし）の関係候補を、
//! ローカルLLM（Ollama経由）で判定するcascade。
//!
//! # 経緯と設計根拠
//!
//! セッション内のPython実験（本体には残していない、`relations.rs`
//! モジュールdocの2026-09-05節・[[llm-classification-rejects-reliably-but-not-accepts]]
//! 参照）で、qwen2.5:3b-instructは**却下側**（malformed/incorrectと
//! 判定すべき候補）は言い回しを変えた2種のpromptで28/28安定して正しく
//! 却下したが、**採用側**（correctと判定すべき候補）は言い回しで
//! 65%〜83%まで揺れた。ユーザー指摘を受けて追加検証したところ、
//! (a) 温度0・同一promptなら3種のseedで完全に同一の判定になる
//! （＝サンプリングノイズではなく言い回し依存の不安定さそのもの）、
//! (b) qwen2.5:7b-instructは採用側で2種の言い回し双方とも29/29
//! （安定・満点）、却下側も27/28（唯一の誤りは検証者自身が「境界例」と
//! 注記した1件）——しかも1件あたり約10秒と、前回の別課題（文単位検証、
//! 40〜60秒）ほど遅くない、ということが判明した。
//!
//! この非対称性（3Bは却下側だけ信頼できる、7Bは両側とも信頼できるが
//! 却下側にまで使うと計算コストが嵩む）を踏まえ、cascade設計にした:
//! 3Bが却下と判定したものはそのまま信頼し（実データの`Proposed`は
//! 大半が却下すべき候補——`relations.rs`モジュールdoc参照）、3Bが
//! 採用と判定したものだけ7Bに再確認させる。
//!
//! この設計は**再現率**（本当に正しい関係のうち何割を拾えるか）を
//! 3B単体の採用側精度（実測65〜83%、言い回し依存）で頭打ちにする——
//! 3Bが誤って却下したものは7Bに回らないため。一方で**適合率**
//! （cascadeが最終的に「採用」と判定したものの正しさ）は7Bの採用側
//! 精度（実測100%）にほぼ一致する。「誤った関係を作るより取りこぼす
//! 方が安全」という既存方針（`relations.rs`のHearst設計、`resolve.rs`
//! の畳み込み判断と同型）に沿った、意図的な非対称設計——取りこぼしは
//! 現状（Proposedは全件Web非表示）より悪化しないが、誤った採用を
//! 増やすことはない。

use crate::relations::RelationKind;
use anyhow::{anyhow, Context, Result};
use std::sync::LazyLock;
use std::time::Duration;

const OLLAMA_URL: &str = "http://127.0.0.1:11434/api/generate";
pub const MODEL_SMALL: &str = "qwen2.5:3b-instruct";
pub const MODEL_LARGE: &str = "qwen2.5:7b-instruct";

// `embed.rs::HTTP_AGENT`と同じ理由（127.0.0.1直指定でDNS解決コストを
// 避け、Keep-Aliveでコネクションを使い回す）。生成は埋め込みより遅い
// （1件約5〜10秒）ため、timeoutは長めに取る。
static HTTP_AGENT: LazyLock<ureq::Agent> =
    LazyLock::new(|| ureq::AgentBuilder::new().timeout(Duration::from_secs(120)).build());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 主張された関係は実際に正しい。
    Correct,
    /// 両フレーズとも実在の整った概念だが、主張された関係は誤り。
    Incorrect,
    /// 少なくとも一方のフレーズが実在の整った数学概念ではない
    /// （壊れた語句断片・人名・非英語テキスト等）。
    Malformed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CascadeOutcome {
    pub verdict: Verdict,
    /// 3Bの判定だけで確定した（却下）か、7Bまで確認した（採用候補）か。
    pub escalated: bool,
}

/// セッション内のPython実験で検証済みの構成（reasoning→末尾JSON、
/// few-shot例6件）をそのまま移植した。JSON強制出力（Ollamaの`format:
/// "json"`）は使わない——前回の別実験（文単位検証）で「常にfalseに
/// 潰れる」ことが分かっているため。
const PROMPT_TEMPLATE: &str = r#"You are checking candidate relationships in a mathematics knowledge graph built by an automated pipeline from arXiv papers. Each candidate claims one phrase is a SPECIALIZATION of another (the first is a strictly narrower/more specific case of the second) or EQUIVALENT to another (they name the same underlying concept, just different words for it).

These candidates come from statistics (word co-occurrence patterns), not from a human, and are frequently wrong in specific ways:
- Two real, well-formed concepts that are simply unrelated or only loosely topically related (they just happen to appear in the same papers).
- A real concept wrongly related to an attribute/property/method/invariant of it (a type mismatch, not a real specialization).
- A phrase that is not a real, well-formed mathematical concept at all: a broken fragment left over from sentence extraction, a generic descriptive phrase, a person's name, or non-English text.
- Occasionally, the claim is actually correct.

Think step by step in 1-2 short sentences using your own mathematical knowledge (do not assume the pipeline is right), then on the FINAL line output ONLY a JSON object matching exactly:
{"verdict": "correct" | "incorrect" | "malformed", "reason": "<under 12 words>"}

- "correct": the claimed relationship is actually true.
- "incorrect": both phrases are real, well-formed concepts, but the claimed relationship is false.
- "malformed": at least one phrase is not a real, well-formed mathematical concept.

Example 1.
Candidate: "elliptic curve" is a SPECIALIZATION of "abelian variety"
Reasoning: An elliptic curve is precisely a 1-dimensional abelian variety, so this is a standard true specialization.
{"verdict": "correct", "reason": "elliptic curves are 1-dim abelian varieties"}

Example 2.
Candidate: "brownian motion" is EQUIVALENT to "wiener process"
Reasoning: These are two standard names for exactly the same stochastic process.
{"verdict": "correct", "reason": "standard alternate names, same process"}

Example 3.
Candidate: "quantum group" is a SPECIALIZATION of "group"
Reasoning: Despite the name, a quantum group is a Hopf algebra, not an actual group, so this is a false specialization.
{"verdict": "incorrect", "reason": "quantum groups are Hopf algebras, not groups"}

Example 4.
Candidate: "riemann hypothesis" is a SPECIALIZATION of "prime number"
Reasoning: The Riemann hypothesis is a conjecture about the zeta function, topically about primes but not a kind of prime number itself.
{"verdict": "incorrect", "reason": "a conjecture is not a specialization of a number"}

Example 5.
Candidate: "paul erdos" is EQUIVALENT to "extremal graph theory"
Reasoning: Paul Erdos is a mathematician, a person's name, not a mathematical concept.
{"verdict": "malformed", "reason": "a person's name, not a concept"}

Example 6.
Candidate: "we prove that" is a SPECIALIZATION of "convergence rate"
Reasoning: "we prove that" is a leftover sentence fragment, not a mathematical concept.
{"verdict": "malformed", "reason": "sentence fragment, not a concept"}

Now judge this candidate. Follow the exact same format: 1-2 sentences of reasoning, then one final JSON line.

Candidate: "{subject}" is {relword} "{object}"
"#;

fn build_prompt(subject: &str, kind: RelationKind, object: &str) -> String {
    let relword = match kind {
        RelationKind::SpecializationOf => "a SPECIALIZATION of",
        RelationKind::EquivalentTo => "EQUIVALENT to",
    };
    PROMPT_TEMPLATE.replace("{subject}", subject).replace("{object}", object).replace("{relword}", relword)
}

/// モデルの生応答から末尾のJSON verdictブロックを読む。reasoning文の中に
/// 無関係な波括弧が紛れる可能性は低いが、`"verdict"`キーを含む`{...}`の
/// うち**最後に現れたもの**を採用することで、誤って早い位置のノイズを
/// 拾わないようにする。ネストは想定しない（verdictオブジェクトは
/// フラットなキー2つだけ）。
fn parse_verdict(response: &str) -> Option<Verdict> {
    let mut last: Option<&str> = None;
    let mut pos = 0usize;
    while let Some(rel) = response[pos..].find('{') {
        let start = pos + rel;
        let Some(end_rel) = response[start..].find('}') else { break };
        let end = start + end_rel + 1;
        let candidate = &response[start..end];
        if candidate.contains("\"verdict\"") {
            last = Some(candidate);
        }
        pos = end;
    }
    let candidate = last?;
    let parsed: serde_json::Value = serde_json::from_str(candidate).ok()?;
    match parsed.get("verdict")?.as_str()? {
        "correct" => Some(Verdict::Correct),
        "incorrect" => Some(Verdict::Incorrect),
        "malformed" => Some(Verdict::Malformed),
        _ => None,
    }
}

fn call_ollama(model: &str, prompt: &str) -> Result<String> {
    let body = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "stream": false,
        "options": { "temperature": 0.0, "seed": 1, "num_predict": 200 }
    })
    .to_string();

    let resp = HTTP_AGENT.post(OLLAMA_URL).set("Content-Type", "application/json").send_string(&body).map_err(|e| {
        anyhow!(
            "Ollamaへのリクエストに失敗しました: {e}\n\
             `ollama serve`が起動しているか、`ollama pull {model}`済みか確認してください。"
        )
    })?;

    let text_resp = resp.into_string().context("Ollama応答の読み取りに失敗")?;
    let parsed: serde_json::Value = serde_json::from_str(&text_resp).context("Ollama応答のJSONパースに失敗")?;
    parsed
        .get("response")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| anyhow!("Ollama応答に response フィールドがありません: {text_resp}"))
}

/// 1つの候補を指定モデルで判定する。Ollamaへの実通信を伴う。
pub fn classify(model: &str, subject: &str, kind: RelationKind, object: &str) -> Result<Option<Verdict>> {
    let prompt = build_prompt(subject, kind, object);
    let response = call_ollama(model, &prompt)?;
    Ok(parse_verdict(&response))
}

/// cascadeの判定ロジック本体。`classify_fn`に実際の判定手段（本番は
/// `classify`経由のOllama呼び出し、テストではスタブ）を注入できるように
/// してある——`embed.rs::embed_all_with`と同じ「I/Oを外から注入して
/// ロジックだけを単体テストする」設計。
fn judge_cascade_with<F>(subject: &str, kind: RelationKind, object: &str, mut classify_fn: F) -> CascadeOutcome
where
    F: FnMut(&str, &str, RelationKind, &str) -> Option<Verdict>,
{
    match classify_fn(MODEL_SMALL, subject, kind, object) {
        Some(Verdict::Incorrect) => CascadeOutcome { verdict: Verdict::Incorrect, escalated: false },
        Some(Verdict::Malformed) => CascadeOutcome { verdict: Verdict::Malformed, escalated: false },
        Some(Verdict::Correct) => {
            // 3Bが採用寄りの判定をした候補だけ、7Bに再確認させる
            // （モジュール冒頭コメント参照——3Bの採用側は言い回しで
            // 65〜83%まで揺れるため、そのまま信頼しない）。
            match classify_fn(MODEL_LARGE, subject, kind, object) {
                Some(v) => CascadeOutcome { verdict: v, escalated: true },
                // 7B応答のパース失敗は安全側（却下）に倒す——3Bが
                // 「採用」寄りだったというだけの弱い根拠で、確認が
                // 取れないまま採用扱いにはしない。
                None => CascadeOutcome { verdict: Verdict::Malformed, escalated: true },
            }
        }
        // 3B応答のパース失敗も安全側（却下）に倒す。
        None => CascadeOutcome { verdict: Verdict::Malformed, escalated: false },
    }
}

/// 1つの候補をcascadeで判定する。Ollamaへの実通信を伴う（3Bを1回、
/// 3Bが「採用」と判定した場合のみ追加で7Bを1回）。
pub fn judge_cascade(subject: &str, kind: RelationKind, object: &str) -> Result<CascadeOutcome> {
    // `classify`は`Result`を返すが、`judge_cascade_with`のクロージャは
    // `Option`を期待する——ネットワークエラー自体はここで即座に伝播させ、
    // 「パースできなかった」（`None`）とは区別する。
    let mut network_error: Option<anyhow::Error> = None;
    let outcome = judge_cascade_with(subject, kind, object, |model, s, k, o| match classify(model, s, k, o) {
        Ok(v) => v,
        Err(e) => {
            network_error = Some(e);
            None
        }
    });
    if let Some(e) = network_error {
        return Err(e);
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_prompt_renders_specialization_wording() {
        let prompt = build_prompt("elliptic curve", RelationKind::SpecializationOf, "abelian variety");
        assert!(prompt.contains(r#"Candidate: "elliptic curve" is a SPECIALIZATION of "abelian variety""#));
    }

    #[test]
    fn build_prompt_renders_equivalent_wording() {
        let prompt = build_prompt("vector space", RelationKind::EquivalentTo, "linear space");
        assert!(prompt.contains(r#"Candidate: "vector space" is EQUIVALENT to "linear space""#));
    }

    #[test]
    fn parse_verdict_reads_the_json_after_free_text_reasoning() {
        let response = "Reasoning: this is a standard fact.\n{\"verdict\": \"correct\", \"reason\": \"textbook fact\"}";
        assert_eq!(parse_verdict(response), Some(Verdict::Correct));
    }

    #[test]
    fn parse_verdict_picks_the_last_verdict_json_block_when_several_braces_appear() {
        // reasoning文中に無関係な波括弧が紛れても、`"verdict"`を含む
        // 最後のブロックを正しく拾う。
        let response = "Some {irrelevant} text.\n{\"verdict\": \"malformed\", \"reason\": \"not a concept\"}";
        assert_eq!(parse_verdict(response), Some(Verdict::Malformed));
    }

    #[test]
    fn parse_verdict_returns_none_when_no_json_is_present() {
        assert_eq!(parse_verdict("The model just rambled without ever producing JSON."), None);
    }

    #[test]
    fn parse_verdict_returns_none_for_an_unrecognized_verdict_value() {
        let response = "{\"verdict\": \"maybe\", \"reason\": \"unsure\"}";
        assert_eq!(parse_verdict(response), None);
    }

    #[test]
    fn cascade_trusts_a_small_model_rejection_without_escalating() {
        let outcome = judge_cascade_with("x", RelationKind::SpecializationOf, "y", |model, _, _, _| {
            assert_eq!(model, MODEL_SMALL, "must not call the large model when the small model already rejects");
            Some(Verdict::Incorrect)
        });
        assert_eq!(outcome, CascadeOutcome { verdict: Verdict::Incorrect, escalated: false });
    }

    #[test]
    fn cascade_trusts_a_small_model_malformed_verdict_without_escalating() {
        let outcome = judge_cascade_with("x", RelationKind::SpecializationOf, "y", |model, _, _, _| {
            assert_eq!(model, MODEL_SMALL);
            Some(Verdict::Malformed)
        });
        assert_eq!(outcome, CascadeOutcome { verdict: Verdict::Malformed, escalated: false });
    }

    #[test]
    fn cascade_escalates_a_small_model_acceptance_to_the_large_model() {
        let mut calls = Vec::new();
        let outcome = judge_cascade_with("x", RelationKind::SpecializationOf, "y", |model, _, _, _| {
            calls.push(model.to_string());
            if model == MODEL_SMALL { Some(Verdict::Correct) } else { Some(Verdict::Correct) }
        });
        assert_eq!(calls, vec![MODEL_SMALL, MODEL_LARGE]);
        assert_eq!(outcome, CascadeOutcome { verdict: Verdict::Correct, escalated: true });
    }

    #[test]
    fn cascade_overturns_a_small_model_acceptance_when_the_large_model_disagrees() {
        // 実データ検証で確認済みの非対称性: 3Bの採用側は信頼できない
        // ため、7Bが却下すればcascadeの最終判定も却下になるべき。
        let outcome = judge_cascade_with("x", RelationKind::SpecializationOf, "y", |model, _, _, _| {
            if model == MODEL_SMALL { Some(Verdict::Correct) } else { Some(Verdict::Incorrect) }
        });
        assert_eq!(outcome, CascadeOutcome { verdict: Verdict::Incorrect, escalated: true });
    }

    #[test]
    fn cascade_falls_back_to_malformed_when_the_large_model_response_is_unparseable() {
        let outcome = judge_cascade_with("x", RelationKind::SpecializationOf, "y", |model, _, _, _| {
            if model == MODEL_SMALL { Some(Verdict::Correct) } else { None }
        });
        assert_eq!(outcome, CascadeOutcome { verdict: Verdict::Malformed, escalated: true });
    }

    #[test]
    fn cascade_falls_back_to_malformed_when_the_small_model_response_is_unparseable() {
        let outcome = judge_cascade_with("x", RelationKind::SpecializationOf, "y", |_, _, _, _| None);
        assert_eq!(outcome, CascadeOutcome { verdict: Verdict::Malformed, escalated: false });
    }
}

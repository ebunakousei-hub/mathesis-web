//! `assertions.json`: assertion単位の詳細ビュー用エクスポート
//! （外部レビュー2026-09-05、提案4「ツールチップの1行では終わらない
//! provenanceパネル」への対応）。
//!
//! `reconcile`が既にサイドカーへ書き出すassertion idの集合ぶんだけ、
//! 述語・主語目的語・認識状態・Evidence（種別・locator・抽出元・
//! メトリック・ソースの由来）・ReviewDecision・既定トラバース対象かを
//! 1つのJSONへまとめる。フロントエンドはこれを1回fetchしてid引きの
//! 辞書として使う——クリックのたびに個別リクエストを飛ばさない。

use crate::model::AssertionId;
use crate::relation_policy::traversal_policy;
use crate::store::ProvenanceStore;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceDetail {
    pub evidence_kind: String,
    pub locator: Option<String>,
    pub locator_precision: String,
    pub extractor_or_model: Option<String>,
    pub metric_name: Option<String>,
    pub metric_value: Option<f64>,
    pub source_provider: String,
    pub source_provider_id: String,
    /// P6.1（`docs/LEAN_DEPENDENCY_POLICY.md`）: `"type"`/`"body"`/`"both"`
    /// ——`evidence_kind: formal_export`の依存辺がどちらから見つかったか。
    /// それ以外の種別は`None`。
    pub dependency_origin: Option<String>,
    /// P6.1: `evidence_kind: formal_export`だけが埋める、
    /// `SourceRecord.reproducibility_json`から組み立てた短い人間可読な
    /// 文字列("Lean 4.29.0-rc6, mathlib 5c8398d, filter policy
    /// mathesis-lean-dependency-filter-v1")。詳細パネルが「どのLean/
    /// フィルタ版がこの辺を作ったか」を生のJSONを見せずに説明できるように。
    pub formal_revision: Option<String>,
    /// P7.1（`docs/P7_1_STATUS.md`）: `source_provider`の生の識別子文字列
    /// ("lean-elaborator"/"math-graph"/...)だけでは、初見の読者に
    /// 「これはMathesis自身が抽出したのか、外部データセットの主張なのか」
    /// が伝わらない——`source_kind_label`(`source_kind`)へ委譲した
    /// 人間可読な短い説明を持たせる。既知でないproviderは
    /// 素通し(生のprovider文字列をそのまま見せる)——分類を捏造しない。
    pub source_kind_label: String,
}

/// P7.1: `SourceRecord.provider`から「どの経路の証拠か」への、既知の
/// providerだけを対象にした人間可読な説明。新しいアダプタを足すたびに
/// ここへ1行追加する規律——未知のprovider文字列をここで推測しない
/// (デフォルトは生の`provider`をそのまま返す、`_ => provider.to_string()`)。
pub fn source_kind_label(provider: &str) -> String {
    match provider {
        "lean-elaborator" => "Mathesis's own Lean build (mathesis-lean-extract)".to_string(),
        "mathesis-legacy-snapshot" => "Mathesis's own text extraction (mathesis-importer)".to_string(),
        "math-graph" => "External dataset: Math-Graph (uw-math-ai, CC BY 4.0) — not independently verified by Mathesis".to_string(),
        "openalex" => "External database: OpenAlex (CC0)".to_string(),
        "msc2020" => "External taxonomy: MSC2020".to_string(),
        "manual-review" => "A human reviewer's own judgment".to_string(),
        other => other.to_string(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewDecisionDetail {
    pub decision: String,
    pub reviewer_id: Option<String>,
    /// P6.3（`docs/P6_3_STATUS.md`）: 承認者が「どんな資格で」承認したか。
    /// `None`/空文字列はリリースゲートが本人確認済みと認めない
    /// （`review::is_authenticated_accept`）——UIもそれをそのまま表示する。
    pub authorization_level: Option<String>,
    pub scope: Option<String>,
    pub rationale: Option<String>,
    pub decided_at_unix: i64,
    /// P6.3: このレビューが「見た」リリースタグ。フロントエンドは
    /// `AssertionDetail.releaseTag`と突き合わせて「別リリース時点の
    /// レビューが今のリリースに横流しされていないか」(staleness)を
    /// 判定できる——サーバ側で真偽値化せず生の文字列のまま渡す。
    pub dataset_version: Option<String>,
    pub expires_at_unix: Option<i64>,
    /// P6.3: この判断が本人確認済みaccept/supersedeとしてリリースゲートを
    /// 通るかどうか——`review::is_authenticated_accept`をこの1件だけに
    /// 適用した結果。全ての判断行がこの意味で「有効」なわけではない
    /// (時系列で後から積まれたrevoke/rejectに効力を奪われた古いaccept等)
    /// ——`eligible_for_default_traversal`と同じ理由で、UIが同じロジックを
    /// JS側に再実装しなくて済むようサーバ側で計算して渡す。
    pub is_current_authenticated_accept: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssertionDetail {
    pub id: i64,
    pub subject_ref: String,
    pub predicate: String,
    pub object_ref: String,
    pub epistemic_state: String,
    pub score: Option<f64>,
    pub release_tag: String,
    pub evidence: Vec<EvidenceDetail>,
    pub review_decisions: Vec<ReviewDecisionDetail>,
    /// ARCHITECTURE_NEXT.md §7の既定トラバース方針
    /// ("observed formal dependencies and reviewed semantic assertions;
    /// proposed edges are opt-in")をそのままここで判定する——
    /// `extracted`/`proposed`/`rejected`は既定では対象外。
    pub eligible_for_default_traversal: bool,
    pub traversal_policy: String,
    /// P3, Increment 1（`docs/P3_STATUS.md`）: `build-catalog`済みなら
    /// `subject_ref`/`object_ref`の人間可読な表示名。カタログが無い/その
    /// 参照がまだ登録されていない場合は`null`——`subjectRef`のタグ付き
    /// 文字列自体は捏造ラベルより正直なので、無ければ黙ってそちらを見せる。
    pub subject_label: Option<String>,
    pub object_label: Option<String>,
    pub subject_label_origin: Option<String>,
    pub object_label_origin: Option<String>,
}

/// 1件のassertionのEvidence行をすべて`EvidenceDetail`へ組み立てる。
/// `export_assertion_details`（`assertions.json`用）と`web_export`
/// （`docs/P2_STATUS.md`、`kind`/`origin`/`status`/`confidence`の再構成に使う）
/// の両方から呼ばれる——2箇所で組み立て方がずれるとWeb側の表示と詳細パネルが
/// 食い違いかねないため、必ずここだけを通す。
pub fn evidence_details_for(prov: &ProvenanceStore, id: AssertionId) -> anyhow::Result<Vec<EvidenceDetail>> {
    prov.evidence_for(id)?
        .into_iter()
        .map(|e| {
            let source = prov.try_get_source_record(e.source_record_id)?;
            let locator_precision = match e.evidence_kind {
                crate::model::EvidenceKind::FormalExport => "formal_artifact",
                crate::model::EvidenceKind::ModelOutput => "model_output",
                crate::model::EvidenceKind::ReviewerNote => "reviewer_note",
                crate::model::EvidenceKind::SourceSpan => {
                    if e.locator.is_some() { "approximate_location" } else { "source_only" }
                }
            };
            // P6.1（`docs/LEAN_DEPENDENCY_POLICY.md`）: `reproducibility_json`
            // (Lean/mathlib版・フィルタポリシー版)を、生のJSONではなく
            // 詳細パネルにそのまま出せる短い1行へ組み立てる。
            // formal_exportでない、またはメタデータが無い場合は`None`
            // ——無いものを捏造しない。
            let formal_revision = if e.evidence_kind == crate::model::EvidenceKind::FormalExport {
                source.as_ref().and_then(|s| s.reproducibility_json.as_deref()).and_then(|raw| {
                    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
                    let lean = v.get("leanToolchain")?.as_str()?;
                    let mathlib = v.get("mathlibRev")?.as_str()?;
                    let policy = v.get("filteringPolicyVersion")?.as_str()?;
                    let mathlib_short: String = mathlib.chars().take(8).collect();
                    Some(format!("{lean}, mathlib {mathlib_short}, filter policy {policy}"))
                })
            } else {
                None
            };
            let source_provider = source.as_ref().map(|s| s.provider.clone()).unwrap_or_default();
            Ok(EvidenceDetail {
                evidence_kind: e.evidence_kind.as_str().to_string(),
                locator: e.locator,
                locator_precision: locator_precision.to_string(),
                extractor_or_model: e.extractor_or_model,
                metric_name: e.metric_name,
                metric_value: e.metric_value,
                source_kind_label: source_kind_label(&source_provider),
                source_provider,
                source_provider_id: source.as_ref().map(|s| s.provider_id.clone()).unwrap_or_default(),
                dependency_origin: e.dependency_origin,
                formal_revision,
            })
        })
        .collect()
}

/// 与えられた`EntityId`(=`subject_entity_id`/`object_entity_id`、FK)の
/// 表示名と由来を引く。`None`はFK自体が未解決(カタログ未構築、または
/// その参照がまだカタログに載っていない)——`try_get_entity`が実際に
/// 引けなかった場合と区別せず両方`None`にする、無いものを捏造しない方針
/// は変わらない。
///
/// P5, Item 2 step 4（`docs/P5_PLAN.md`）: 以前は`ref_string`を
/// `resolve_entity_ref`で都度引き直していた——今は呼び出し元
/// (`export_assertion_details`)が持つ`assertion.subject_entity_id`/
/// `object_entity_id`をそのまま渡す。FKが真実の記録なので、文字列の
/// 表記ゆれ（`concept:kahler manifolds`のようなalias形）を経由しても
/// 正しいエンティティへ辿り着く——`resolve_entity_ref`と等価だが、
/// 「このassertionが実際に指しているエンティティ」を再解決ではなく
/// 直接参照する点が異なる。
pub fn entity_label_with_origin(
    prov: &ProvenanceStore,
    entity_id: Option<crate::model::EntityId>,
) -> anyhow::Result<(Option<String>, Option<String>)> {
    let Some(entity_id) = entity_id else {
        return Ok((None, None));
    };
    let entity = prov.try_get_entity(entity_id)?;
    let origin = prov.label_origin(entity_id)?.map(|o| o.as_str().to_string());
    Ok((entity.map(|e| e.display_label), origin))
}

/// `evidence_details_for`と同じ理由で共有する。`release_tag`/`now_unix`は
/// `review::is_authenticated_accept`（P6.3、"no drift"・失効判定）にそのまま
/// 渡す——ここで真偽値化してしまえば、ゲートと詳細パネルのロジックが
/// 将来ずれる余地ができる。
pub fn review_decision_details_for(
    prov: &ProvenanceStore,
    id: AssertionId,
    release_tag: &str,
    now_unix: i64,
) -> anyhow::Result<Vec<ReviewDecisionDetail>> {
    let decisions = prov.review_decisions_for(id)?;
    // P6.3: 時系列で最後の1件だけが「今の実効判断」——`review::
    // effective_review_decision`と同じ定義をここでも使う(古いacceptに
    // 誤ってcurrent=trueを付けない)。
    let effective_id = decisions.last().map(|d| d.id);
    Ok(decisions
        .into_iter()
        .map(|r| {
            let is_current_authenticated_accept =
                Some(r.id) == effective_id && crate::review::is_authenticated_accept(&r, now_unix, release_tag);
            ReviewDecisionDetail {
                decision: r.decision.as_str().to_string(),
                reviewer_id: r.reviewer_id,
                authorization_level: r.authorization_level,
                scope: r.scope,
                rationale: r.rationale,
                decided_at_unix: r.decided_at_unix,
                dataset_version: r.dataset_version,
                expires_at_unix: r.expires_at_unix,
                is_current_authenticated_accept,
            }
        })
        .collect())
}

/// `assertion_ids`ぶんの詳細を、id(文字列化、JSON object keyのため)→詳細の
/// 辞書として組み立てる。存在しないidは黙ってスキップする
/// （`verify`が別途「サイドカーが指すidが実在するか」を検査する担当）。
pub fn export_assertion_details(
    prov: &ProvenanceStore,
    release_tag: &str,
    now_unix: i64,
    assertion_ids: impl IntoIterator<Item = i64>,
) -> anyhow::Result<BTreeMap<String, AssertionDetail>> {
    let mut out = BTreeMap::new();
    for raw_id in assertion_ids {
        let id = AssertionId(raw_id);
        let Some(assertion) = prov.try_get_assertion(id)? else { continue };
        let evidence = evidence_details_for(prov, id)?;
        let review_decisions = review_decision_details_for(prov, id, release_tag, now_unix)?;
        let (subject_label, subject_label_origin) = entity_label_with_origin(prov, assertion.subject_entity_id)?;
        let (object_label, object_label_origin) = entity_label_with_origin(prov, assertion.object_entity_id)?;
        out.insert(
            raw_id.to_string(),
            AssertionDetail {
                id: raw_id,
                subject_ref: assertion.subject_ref,
                predicate: assertion.predicate.as_str().to_string(),
                object_ref: assertion.object_ref,
                epistemic_state: assertion.epistemic_state.as_str().to_string(),
                score: assertion.score,
                release_tag: release_tag.to_string(),
                evidence,
                review_decisions,
                eligible_for_default_traversal: matches!(
                    traversal_policy(assertion.predicate, assertion.epistemic_state),
                    crate::relation_policy::TraversalPolicy::DefaultTraversal
                ),
                traversal_policy: traversal_policy(assertion.predicate, assertion.epistemic_state).as_str().to_string(),
                subject_label,
                object_label,
                subject_label_origin,
                object_label_origin,
            },
        );
    }
    Ok(out)
}

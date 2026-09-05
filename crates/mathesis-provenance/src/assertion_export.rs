//! `assertions.json`: assertion単位の詳細ビュー用エクスポート
//! （外部レビュー2026-09-05、提案4「ツールチップの1行では終わらない
//! provenanceパネル」への対応）。
//!
//! `reconcile`が既にサイドカーへ書き出すassertion idの集合ぶんだけ、
//! 述語・主語目的語・認識状態・Evidence（種別・locator・抽出元・
//! メトリック・ソースの由来）・ReviewDecision・既定トラバース対象かを
//! 1つのJSONへまとめる。フロントエンドはこれを1回fetchしてid引きの
//! 辞書として使う——クリックのたびに個別リクエストを飛ばさない。

use crate::model::{AssertionId, EpistemicState};
use crate::store::ProvenanceStore;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceDetail {
    pub evidence_kind: String,
    pub locator: Option<String>,
    pub extractor_or_model: Option<String>,
    pub metric_name: Option<String>,
    pub metric_value: Option<f64>,
    pub source_provider: String,
    pub source_provider_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewDecisionDetail {
    pub decision: String,
    pub reviewer_id: Option<String>,
    pub scope: Option<String>,
    pub rationale: Option<String>,
    pub decided_at_unix: i64,
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
    /// P3, Increment 1（`docs/P3_STATUS.md`）: `build-catalog`済みなら
    /// `subject_ref`/`object_ref`の人間可読な表示名。カタログが無い/その
    /// 参照がまだ登録されていない場合は`null`——`subjectRef`のタグ付き
    /// 文字列自体は捏造ラベルより正直なので、無ければ黙ってそちらを見せる。
    pub subject_label: Option<String>,
    pub object_label: Option<String>,
}

/// ARCHITECTURE_NEXT.md §7の既定トラバース方針("observed formal dependencies
/// and reviewed semantic assertions; proposed edges are opt-in")の判定。
/// `web_export`もこれを再利用する——「詳細パネルで見る既定トラバース対象か」と
/// 「一覧に既定で出すか」が別の基準になってはいけない。
pub(crate) fn is_eligible_for_default_traversal(state: EpistemicState) -> bool {
    matches!(state, EpistemicState::Observed | EpistemicState::Reviewed | EpistemicState::Verified)
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
            Ok(EvidenceDetail {
                evidence_kind: e.evidence_kind.as_str().to_string(),
                locator: e.locator,
                extractor_or_model: e.extractor_or_model,
                metric_name: e.metric_name,
                metric_value: e.metric_value,
                source_provider: source.as_ref().map(|s| s.provider.clone()).unwrap_or_default(),
                source_provider_id: source.as_ref().map(|s| s.provider_id.clone()).unwrap_or_default(),
            })
        })
        .collect()
}

/// 1件のassertionのReviewDecision行をすべて`ReviewDecisionDetail`へ組み立てる。
/// `ref_string`（`subject_ref`/`object_ref`のタグ付き文字列）が指す
/// エンティティの表示名を引く。カタログ未構築、またはその参照がまだ
/// カタログに載っていなければ`None`——`try_get_entity`ではなく
/// `resolve_entity_ref`から辿るのは、conceptの表記ゆれ（aliasのref文字列）
/// も直接引けるようにするため。
pub fn entity_label_for(prov: &ProvenanceStore, ref_string: &str) -> anyhow::Result<Option<String>> {
    let Some(entity_id) = prov.resolve_entity_ref(ref_string)? else { return Ok(None) };
    Ok(prov.try_get_entity(entity_id)?.map(|e| e.display_label))
}

/// `evidence_details_for`と同じ理由で共有する。
pub fn review_decision_details_for(prov: &ProvenanceStore, id: AssertionId) -> anyhow::Result<Vec<ReviewDecisionDetail>> {
    Ok(prov
        .review_decisions_for(id)?
        .into_iter()
        .map(|r| ReviewDecisionDetail {
            decision: r.decision.as_str().to_string(),
            reviewer_id: r.reviewer_id,
            scope: r.scope,
            rationale: r.rationale,
            decided_at_unix: r.decided_at_unix,
        })
        .collect())
}

/// `assertion_ids`ぶんの詳細を、id(文字列化、JSON object keyのため)→詳細の
/// 辞書として組み立てる。存在しないidは黙ってスキップする
/// （`verify`が別途「サイドカーが指すidが実在するか」を検査する担当）。
pub fn export_assertion_details(
    prov: &ProvenanceStore,
    release_tag: &str,
    assertion_ids: impl IntoIterator<Item = i64>,
) -> anyhow::Result<BTreeMap<String, AssertionDetail>> {
    let mut out = BTreeMap::new();
    for raw_id in assertion_ids {
        let id = AssertionId(raw_id);
        let Some(assertion) = prov.try_get_assertion(id)? else { continue };
        let evidence = evidence_details_for(prov, id)?;
        let review_decisions = review_decision_details_for(prov, id)?;
        let subject_label = entity_label_for(prov, &assertion.subject_ref)?;
        let object_label = entity_label_for(prov, &assertion.object_ref)?;
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
                eligible_for_default_traversal: is_eligible_for_default_traversal(assertion.epistemic_state),
                subject_label,
                object_label,
            },
        );
    }
    Ok(out)
}

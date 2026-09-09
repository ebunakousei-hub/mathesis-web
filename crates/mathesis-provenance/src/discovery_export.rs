//! P7.4（`docs/P7_4_STATUS.md`）: 「Math-Graph比較/発見モード」のUIが
//! 読む、自己完結した1本のJSON。既存の`web_export.rs`
//! (`dependencies.json`)には意図的に触れない——`judgment_id_for_entity`
//! の数値id契約(P7.1で判明)により、`judgment:mathgraph:`名前空間の
//! assertionはそもそもそこへ出せない。ここでは代わりに
//! `subject_entity_id`/`object_entity_id`の表示ラベルをそのまま出す、
//! 独立した読み取りモデルを作る——本番の系譜ビュー・検索・既定の
//! 信頼グラフには一切触れない、追加専用のレイヤー。
//!
//! 4種の出典を明示的に区別する(ユーザー指示):
//! - `mathesis-checker`: Mathesis自身のLean elaborator実行結果
//!   (`evidence_kind: formal_export`、provider: `lean-elaborator`)
//! - `mathesis-text`: Mathesis自身のテキスト抽出(`mathesis-importer`)
//! - `math-graph-literal`: Math-Graphの実在Lean宣言どうしの依存
//!   (`external_classification: external_literal_dependency`)
//! - `math-graph-hierarchy`: Math-Graphの型クラス階層合成ノード
//!   (`external_classification: external_typeclass_hierarchy`)

use crate::assertion_export::{entity_label_with_origin, evidence_details_for};
use crate::model::ReleaseId;
use crate::relation_policy::traversal_policy;
use crate::store::ProvenanceStore;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiscoverySource {
    MathesisChecker,
    MathesisText,
    MathGraphLiteral,
    MathGraphHierarchy,
}

impl DiscoverySource {
    pub fn is_external(self) -> bool {
        matches!(self, Self::MathGraphLiteral | Self::MathGraphHierarchy)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryEdge {
    pub assertion_id: i64,
    pub subject: String,
    pub object: String,
    pub source: DiscoverySource,
    pub epistemic_state: String,
    pub traversal_policy: String,
    pub edge_type: Option<String>,
    /// P7.1のパターンをそのまま再利用——`assertion_export::source_kind_label`
    /// と同じ文言。
    pub source_kind_label: String,
    pub license: Option<String>,
    pub locator: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryCounts {
    pub mathesis_checker: usize,
    pub mathesis_text: usize,
    pub math_graph_literal: usize,
    pub math_graph_hierarchy: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryExport {
    pub project_label: String,
    pub release_tag: String,
    pub edges: Vec<DiscoveryEdge>,
    pub counts: DiscoveryCounts,
}

/// 1リリースぶんの全assertionを歩き、4種の出典へ分類する。既存の
/// `dependencies.json`とは完全に独立——同じDBを読むだけで、既定の
/// 信頼グラフ/検索インデックスには一切書き込まない。
pub fn build_discovery_export(
    prov: &ProvenanceStore,
    release: ReleaseId,
    release_tag: &str,
    project_label: &str,
) -> anyhow::Result<DiscoveryExport> {
    let mut edges = Vec::new();
    let mut counts = DiscoveryCounts::default();

    for a in prov.list_assertions_for_release(release)? {
        // 表示用(source_kind_label等)は`evidence_details_for`、分類の根拠
        // (`external_classification`)は生の`Evidence`から——前者はまだ
        // その列を露出していないため、両方引く(1件のevidenceに対し2回
        // クエリするが、pilot規模(数百件)では気にする量ではない)。
        let evidence = evidence_details_for(prov, a.id)?;
        let Some(ev) = evidence.first() else { continue };
        let raw_evidence = prov.evidence_for(a.id)?;
        let Some(raw_ev) = raw_evidence.first() else { continue };

        let (subject_label, _) = entity_label_with_origin(prov, a.subject_entity_id)?;
        let (object_label, _) = entity_label_with_origin(prov, a.object_entity_id)?;
        let subject = subject_label.unwrap_or_else(|| a.subject_ref.clone());
        let object = object_label.unwrap_or_else(|| a.object_ref.clone());

        let source = match raw_ev.external_classification.as_deref() {
            Some("external_literal_dependency") => DiscoverySource::MathGraphLiteral,
            Some("external_typeclass_hierarchy") => DiscoverySource::MathGraphHierarchy,
            _ if raw_ev.evidence_kind == crate::model::EvidenceKind::FormalExport => DiscoverySource::MathesisChecker,
            _ => DiscoverySource::MathesisText,
        };

        match source {
            DiscoverySource::MathesisChecker => counts.mathesis_checker += 1,
            DiscoverySource::MathesisText => counts.mathesis_text += 1,
            DiscoverySource::MathGraphLiteral => counts.math_graph_literal += 1,
            DiscoverySource::MathGraphHierarchy => counts.math_graph_hierarchy += 1,
        }

        // ライセンス表記は外部由来の行だけ埋める——Mathesis自身の証拠に
        // "licence"は無意味(自分のデータに自分でライセンスを主張しない)。
        let license = if source.is_external() {
            prov.try_get_source_record(raw_ev.source_record_id)?.and_then(|sr| sr.licence.clone())
        } else {
            None
        };

        edges.push(DiscoveryEdge {
            assertion_id: a.id.0,
            subject,
            object,
            source,
            epistemic_state: a.epistemic_state.as_str().to_string(),
            traversal_policy: traversal_policy(a.predicate, a.epistemic_state).as_str().to_string(),
            edge_type: ev.dependency_origin.clone(),
            source_kind_label: ev.source_kind_label.clone(),
            license,
            locator: ev.locator.clone(),
        });
    }

    Ok(DiscoveryExport { project_label: project_label.to_string(), release_tag: release_tag.to_string(), edges, counts })
}

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
use crate::msc_classification::ClassificationStatus;
use crate::relation_policy::traversal_policy;
use crate::store::ProvenanceStore;
use serde::Serialize;
use std::collections::BTreeMap;

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
    /// P8.2（`docs/P8_2_STATUS.md`）: 外部由来の辺だけ、その宣言の
    /// `SourceRecord.reproducibility_json`から`repoSlug`を引いたもの
    /// （`math_graph_adapter::import_statement`が埋めた値）。Mathesis自身の
    /// 辺（checker-derived/text-extracted）は`None`——プロジェクト概念が
    /// そもそも無い。
    pub source_project: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryCounts {
    pub mathesis_checker: usize,
    pub mathesis_text: usize,
    pub math_graph_literal: usize,
    pub math_graph_hierarchy: usize,
}

/// P8.2: 1プロジェクト（`repo_slug`）ぶんの外部辺カバレッジ集計——「連結先の
/// 索引全体ではなく、このpanelが実際に持っているものだけ」を見せるための
/// 内訳（"coverage metrics"、Stage 3ディレクティブ項目）。`literal`/
/// `hierarchy`は`ExternalClassification`の2値のみ——3件目の"excluded"は
/// そもそもこのDBに一度もimportされていないので、ここには出しようがない
/// （`docs/P8_1_STATUS.md`「What actually landed」参照）。
#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEdgeCount {
    pub repo_slug: String,
    pub literal_count: usize,
    pub hierarchy_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryExport {
    pub project_label: String,
    pub release_tag: String,
    pub edges: Vec<DiscoveryEdge>,
    pub counts: DiscoveryCounts,
    /// `repo_slug`降順ではなく件数降順（同数はrepo_slug昇順）——PA.3
    /// `export.rs::fields`と同じ理由で、HashMapの反復順に頼ると実行ごとに
    /// 順序が変わりうるため明示的にソートする。
    pub by_project: Vec<ProjectEdgeCount>,
    /// P8.2 Stage 3ディレクティブの「MSC ancestor lookup / Unclassified /
    /// unavailable」要求への誠実な回答: MSC整合（`mathesis-taxonomy::
    /// alignment`）はarXiv概念クラスタにしか走らず、Lean宣言には一度も
    /// 走っていない（`docs/PA_3_STATUS.md`）。したがってこのexportが持つ
    /// 外部宣言はすべて`ClassificationStatus::Unavailable`——0件のクラスタを
    /// 意味する空の`Unclassified`フィルタUIを作るのではなく、この1行で
    /// 状態を明示する。外部辺が無いexportでは空文字列（該当なし）。
    pub msc_classification_note: String,
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
    let mut project_counts: BTreeMap<String, ProjectEdgeCount> = BTreeMap::new();

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

        // P8.2: `repoSlug`は辺自身のsource_record（`import_pilot`が
        // reproducibility_jsonを持たせずに作る）ではなく、subject宣言の
        // 実体（`import_statement`が`repoSlug`込みで作る）から引く。
        let source_project = if source.is_external() { repo_slug_for_entity(prov, a.subject_entity_id)? } else { None };
        if let Some(slug) = &source_project {
            let entry = project_counts.entry(slug.clone()).or_insert_with(|| ProjectEdgeCount { repo_slug: slug.clone(), ..Default::default() });
            match source {
                DiscoverySource::MathGraphLiteral => entry.literal_count += 1,
                DiscoverySource::MathGraphHierarchy => entry.hierarchy_count += 1,
                _ => {}
            }
        }

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
            source_project,
        });
    }

    let external_edge_count = counts.math_graph_literal + counts.math_graph_hierarchy;
    let msc_classification_note = if external_edge_count == 0 {
        String::new()
    } else {
        format!(
            "MSC classification status: {} ({} external dependency edges reference Math-Graph Lean \
             declarations) — MSC alignment (mathesis-taxonomy::alignment) only ever runs on arXiv \
             concept clusters, never on Lean declarations, so these are honestly unclassified rather \
             than evaluated and found non-matching. See docs/DATA_DICTIONARY.md.",
            ClassificationStatus::Unavailable.as_str(),
            external_edge_count,
        )
    };

    // 件数降順・同数はrepo_slug昇順——HashMap/BTreeMapの反復順そのものに頼らず
    // 明示的にソートする(PA.3 export.rs::fieldsと同じ理由、実データ2回の
    // exportを突き合わせて確認済みの非決定性バグの再発防止)。
    let mut by_project: Vec<ProjectEdgeCount> = project_counts.into_values().collect();
    by_project.sort_by(|a, b| {
        let total = |p: &ProjectEdgeCount| p.literal_count + p.hierarchy_count;
        total(b).cmp(&total(a)).then_with(|| a.repo_slug.cmp(&b.repo_slug))
    });

    Ok(DiscoveryExport {
        project_label: project_label.to_string(),
        release_tag: release_tag.to_string(),
        edges,
        counts,
        by_project,
        msc_classification_note,
    })
}

/// 外部由来の辺のsubject宣言が属するプロジェクト名（`repoSlug`）を、その
/// 宣言自身のsource_record（`math_graph_adapter::import_statement`が
/// `reproducibility_json`込みで作る）から引く。Mathesis自身の宣言や、
/// 何らかの理由でsource_recordを持たないentityでは`None`——無い情報を
/// 捏造しない。P8.4（`docs/P8_4_STATUS.md`）: `math_graph_adapter::remove_project`
/// も同じ抽出が要るため`pub(crate)`にして共有する（2つ目のJSON解析を
/// 書かない）。
pub(crate) fn repo_slug_for_entity(prov: &ProvenanceStore, entity_id: Option<crate::model::EntityId>) -> anyhow::Result<Option<String>> {
    let Some(entity_id) = entity_id else { return Ok(None) };
    let Some(entity) = prov.try_get_entity(entity_id)? else { return Ok(None) };
    let Some(source_record_id) = entity.source_record_id else { return Ok(None) };
    let Some(source_record) = prov.try_get_source_record(source_record_id)? else { return Ok(None) };
    let Some(repro_json) = source_record.reproducibility_json else { return Ok(None) };
    let parsed: serde_json::Value = serde_json::from_str(&repro_json)?;
    Ok(parsed.get("repoSlug").and_then(|v| v.as_str()).map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math_graph_adapter::{self, ExternalClassification, PilotEdge, PilotStatement};
    use crate::model::NewRelease;

    fn stmt(id: &str, decl_name: &str, repo_slug: &str, classification: ExternalClassification) -> PilotStatement {
        PilotStatement {
            statement_id: id.into(),
            decl_name: decl_name.into(),
            module: format!("{repo_slug}.Mod"),
            kind: "theorem".into(),
            file_path: format!("{repo_slug}/Mod.lean"),
            is_instance: false,
            repo_slug: repo_slug.into(),
            lean_toolchain: None,
            mathlib_rev: None,
            git_commit: None,
            classification,
        }
    }

    /// 2プロジェクト（`ProjectA`: literal×2、`ProjectB`: hierarchy×1）を
    /// 1つの孤立DBへ取り込み、P8.2で足した3つのフィールド（`sourceProject`/
    /// `byProject`/`mscClassificationNote`）が実データから正しく組み立つ
    /// ことを確認する——`docs/P8_2_STATUS.md`。
    #[test]
    fn discovery_export_carries_per_project_coverage_and_an_honest_msc_note() {
        let prov = ProvenanceStore::open_in_memory().unwrap();
        let release = prov
            .get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None })
            .unwrap();
        let statements = vec![
            stmt("a1", "A.one", "ProjectA", ExternalClassification::ExternalLiteralDependency),
            stmt("a2", "A.two", "ProjectA", ExternalClassification::ExternalLiteralDependency),
            stmt("a3", "A.three", "ProjectA", ExternalClassification::ExternalLiteralDependency),
            stmt("b1", "B.one", "ProjectB", ExternalClassification::ExternalTypeclassHierarchy),
            stmt("b2", "B.two", "ProjectB", ExternalClassification::ExternalTypeclassHierarchy),
        ];
        let edges = vec![
            PilotEdge { src_id: "a1".into(), dep_id: "a2".into(), edge_type: "sig".into(), role: None, via_proj: false },
            PilotEdge { src_id: "a2".into(), dep_id: "a3".into(), edge_type: "sig".into(), role: None, via_proj: false },
            PilotEdge { src_id: "b1".into(), dep_id: "b2".into(), edge_type: "def".into(), role: None, via_proj: false },
        ];
        prov.transaction(|| math_graph_adapter::import_pilot(&prov, release, &statements, &edges, "rev-1")).unwrap();

        let export = build_discovery_export(&prov, release, "t", "combined pilot").unwrap();

        assert_eq!(export.edges.len(), 3);
        for edge in &export.edges {
            assert_eq!(edge.source_project.as_deref(), Some(if edge.subject.starts_with("A.") { "ProjectA" } else { "ProjectB" }));
        }

        // 件数降順: ProjectA(2件)がProjectB(1件)より先に来る。
        assert_eq!(export.by_project.len(), 2);
        assert_eq!(export.by_project[0].repo_slug, "ProjectA");
        assert_eq!(export.by_project[0].literal_count, 2);
        assert_eq!(export.by_project[0].hierarchy_count, 0);
        assert_eq!(export.by_project[1].repo_slug, "ProjectB");
        assert_eq!(export.by_project[1].hierarchy_count, 1);

        assert!(export.msc_classification_note.contains("unavailable"));
        assert!(export.msc_classification_note.contains("3 external dependency edges"));
    }

    #[test]
    fn msc_classification_note_is_empty_when_there_are_no_external_edges() {
        let prov = ProvenanceStore::open_in_memory().unwrap();
        let release = prov
            .get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None })
            .unwrap();
        let export = build_discovery_export(&prov, release, "t", "empty").unwrap();
        assert_eq!(export.msc_classification_note, "");
        assert!(export.by_project.is_empty());
    }
}

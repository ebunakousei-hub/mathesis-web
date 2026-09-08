//! Phase 1, Increment 2 (started 2026-09-05, in response to a second
//! external review): satisfies the literal P1 exit criterion — "each
//! current visible edge can be traced to a legacy source record and
//! release ID" — for the edges the running Web app actually displays,
//! without touching `mathesis-graph::export`, `mathesis-taxonomy::export`,
//! or `mathesis-server` (no risk of changing the shipped JSON's shape or
//! the app's behavior).
//!
//! Approach: re-derive the exact same `legacy_ref` keys the adapter used
//! (via `legacy_adapter`'s builder functions, so the two can never drift
//! apart), look each one up in an already-populated `ProvenanceStore`, and
//! emit small sidecar JSON files the frontend can additively fetch and
//! cross-reference by id — `judgments.provenance.json` and
//! `taxonomy.relations.provenance.json`. This is the same "small additive
//! file, don't touch the existing shape" move already used for
//! `generated_at_unix`.

use crate::legacy_adapter::{
    concept_relation_legacy_ref, judgment_dependency_legacy_ref, morphism_legacy_ref, paper_citation_legacy_ref,
};
use crate::store::ProvenanceStore;
use mathesis_graph::GraphStore;
use mathesis_taxonomy::relations::RelationStatus as TaxRelationStatus;
use mathesis_taxonomy::store::TaxonomyStore;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Debug, Default)]
pub struct ReconcileReport {
    pub release_tag: String,
    pub dependencies_total: usize,
    pub dependencies_traced: usize,
    pub citations_total: usize,
    pub citations_traced: usize,
    pub morphisms_total: usize,
    pub morphisms_traced: usize,
    pub relations_total: usize,
    pub relations_traced: usize,
}

impl ReconcileReport {
    pub fn is_fully_traced(&self) -> bool {
        self.dependencies_total == self.dependencies_traced
            && self.citations_total == self.citations_traced
            && self.morphisms_total == self.morphisms_traced
            && self.relations_total == self.relations_traced
    }

    pub fn print(&self) {
        println!("reconciliation against release {}", self.release_tag);
        println!("  dependencies: {}/{} traced", self.dependencies_traced, self.dependencies_total);
        println!("  citations:    {}/{} traced", self.citations_traced, self.citations_total);
        println!("  morphisms:    {}/{} traced", self.morphisms_traced, self.morphisms_total);
        println!("  relations:    {}/{} traced", self.relations_traced, self.relations_total);
        if self.is_fully_traced() {
            println!("  -> every currently-displayed edge resolves to a RelationAssertion in this release.");
        } else {
            println!("  -> INCOMPLETE: some displayed edges have no matching assertion (see counts above).");
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DependencyProvenance {
    pub from: i64,
    pub to: i64,
    pub assertion_id: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CitationProvenance {
    pub from: String,
    pub to: String,
    pub assertion_id: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MorphismProvenance {
    pub morphism_id: i64,
    pub assertion_id: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RelationProvenance {
    pub subject: String,
    pub object: String,
    pub kind: String,
    pub assertion_id: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct JudgmentsProvenanceExport {
    pub release_tag: String,
    pub release_git_commit: Option<String>,
    pub dependencies: Vec<DependencyProvenance>,
    pub citations: Vec<CitationProvenance>,
    pub morphisms: Vec<MorphismProvenance>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RelationsProvenanceExport {
    pub release_tag: String,
    pub release_git_commit: Option<String>,
    pub relations: Vec<RelationProvenance>,
}

/// `mathesis-graph`側（judgment_dependencies/paper_citations/morphisms）を
/// 突き合わせ、`judgments.provenance.json`相当の構造とレポートを返す。
pub fn reconcile_graph(
    graph: &GraphStore,
    prov: &ProvenanceStore,
    release_tag: &str,
) -> anyhow::Result<(JudgmentsProvenanceExport, ReconcileReport)> {
    let release = prov
        .get_release_by_tag(release_tag)?
        .ok_or_else(|| anyhow::anyhow!("release '{release_tag}' not found in provenance DB — run import-legacy first"))?;

    let mut report = ReconcileReport { release_tag: release_tag.to_string(), ..Default::default() };
    let mut dependencies = Vec::new();
    let mut morphisms = Vec::new();

    for j in graph.list_judgments()? {
        for dep in graph.dependencies_of(j.id)? {
            report.dependencies_total += 1;
            let legacy_ref = judgment_dependency_legacy_ref(j.id.0, dep.0);
            if let Some(assertion_id) = prov.get_assertion_by_legacy_ref(release.id, &legacy_ref)? {
                report.dependencies_traced += 1;
                dependencies.push(DependencyProvenance { from: j.id.0, to: dep.0, assertion_id: assertion_id.0 });
            }
        }
    }

    // P6.1（`docs/LEAN_DEPENDENCY_POLICY.md`）: checker-derived depends_on
    // assertions（`lean_manifest_adapter`、`legacy_ref: "lean-manifest:..."`）
    // には`mathesis-graph`側の`judgment_dependencies`行が最初から無い
    // ——`import-lean-manifest`は証拠層だけへ書き込み、レガシーのnode-graph
    // テーブルには一切触れない（意図的な設計、`docs/P6_STATUS.md`参照）。
    // 上のループは`graph.dependencies_of`だけを回るので、この種の辺は
    // 素通りして`assertions.json`から漏れ、実際に発見した実害
    // (Webの依存チップから根拠パネルを開くと「見つかりません」になる)
    // ——ここで直接ProvenanceStoreから拾って同じ`dependencies`配列へ足す。
    for a in prov.list_assertions_for_release(release.id)? {
        if a.predicate != crate::model::RelationKind::DependsOn {
            continue;
        }
        let is_checker_derived = a.legacy_ref.as_deref().is_some_and(|r| r.starts_with("lean-manifest:"));
        if !is_checker_derived {
            continue;
        }
        report.dependencies_total += 1;
        let (Some(subject_entity_id), Some(object_entity_id)) = (a.subject_entity_id, a.object_entity_id) else { continue };
        let (Some(from), Some(to)) = (prov.judgment_id_for_entity(subject_entity_id)?, prov.judgment_id_for_entity(object_entity_id)?) else {
            continue;
        };
        report.dependencies_traced += 1;
        dependencies.push(DependencyProvenance { from, to, assertion_id: a.id.0 });
    }

    let mut citations = Vec::new();
    let papers = graph.list_papers()?;
    let arxiv_by_paper_id: std::collections::HashMap<i64, String> =
        papers.iter().map(|p| (p.id.0, p.arxiv_id.clone())).collect();
    for p in &papers {
        for target in graph.citations_of(p.id)? {
            report.citations_total += 1;
            let legacy_ref = paper_citation_legacy_ref(p.id.0, target.0);
            if let Some(assertion_id) = prov.get_assertion_by_legacy_ref(release.id, &legacy_ref)? {
                if let Some(target_arxiv_id) = arxiv_by_paper_id.get(&target.0) {
                    report.citations_traced += 1;
                    citations.push(CitationProvenance {
                        from: p.arxiv_id.clone(),
                        to: target_arxiv_id.clone(),
                        assertion_id: assertion_id.0,
                    });
                }
            }
        }
    }

    for m in graph.list_morphisms()? {
        report.morphisms_total += 1;
        let legacy_ref = morphism_legacy_ref(m.id.0);
        if let Some(assertion_id) = prov.get_assertion_by_legacy_ref(release.id, &legacy_ref)? {
            report.morphisms_traced += 1;
            morphisms.push(MorphismProvenance { morphism_id: m.id.0, assertion_id: assertion_id.0 });
        }
    }

    Ok((
        JudgmentsProvenanceExport {
            release_tag: release.tag.clone(),
            release_git_commit: release.git_commit.clone(),
            dependencies,
            citations,
            morphisms,
        },
        report,
    ))
}

/// `mathesis-taxonomy`側（concept_relations）を突き合わせる。戻り値の
/// `ReconcileReport`は`reconcile_graph`のものと`relations_*`フィールドだけ
/// 合算して使う想定（呼び出し側で足し合わせる）。
pub fn reconcile_taxonomy(
    taxonomy: &TaxonomyStore,
    prov: &ProvenanceStore,
    release_tag: &str,
) -> anyhow::Result<(RelationsProvenanceExport, ReconcileReport)> {
    let release = prov
        .get_release_by_tag(release_tag)?
        .ok_or_else(|| anyhow::anyhow!("release '{release_tag}' not found in provenance DB — run import-legacy first"))?;

    let mut report = ReconcileReport { release_tag: release_tag.to_string(), ..Default::default() };
    let mut relations = Vec::new();

    // `mathesis-taxonomy::export::build_relations_export`はProposedを
    // 意図的に落として出荷している（distributionalのみ・根拠文なし——
    // web/README.md参照）。「今表示されている辺」の定義に合わせ、ここでも
    // Grounded/Confirmedだけを対象にする——Proposedまで含めると
    // taxonomy.relations.provenance.jsonが実際に表示される情報の100倍近く
    // 膨らみ、フロントエンドが使わないデータを配信することになる。
    for edge in taxonomy.load_relations()?.into_iter().filter(|e| e.status != TaxRelationStatus::Proposed) {
        report.relations_total += 1;
        let legacy_ref = concept_relation_legacy_ref(&edge.subject, &edge.object, edge.kind);
        if let Some(assertion_id) = prov.get_assertion_by_legacy_ref(release.id, &legacy_ref)? {
            report.relations_traced += 1;
            relations.push(RelationProvenance {
                subject: edge.subject,
                object: edge.object,
                kind: crate::legacy_adapter::tax_kind_str(edge.kind).to_string(),
                assertion_id: assertion_id.0,
            });
        }
    }

    Ok((
        RelationsProvenanceExport {
            release_tag: release.tag.clone(),
            release_git_commit: release.git_commit.clone(),
            relations,
        },
        report,
    ))
}

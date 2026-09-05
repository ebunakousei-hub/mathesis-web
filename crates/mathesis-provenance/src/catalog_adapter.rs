//! P3, Increment 1 (`docs/P3_STATUS.md`): the typed entity catalog
//! (ARCHITECTURE_NEXT.md §5.2's `Paper`/`Statement`/`Concept`, minimal
//! version). Builds `entities`/`entity_refs` additively from the SAME
//! legacy sources `legacy_adapter.rs` already trusts — no new heuristics,
//! no invented labels. A judgment's display label is its own `name` (or a
//! generic placeholder naming its kind, never a guessed title); a paper's
//! is its own `title` (or its arXiv id if none was ever ingested); a
//! concept's is the representative phrase `mathesis-taxonomy`'s own Entity
//! Resolution (`resolve.rs`) already computed, with every alias in that
//! same group mapped to the same entity.
//!
//! This does **not** yet replace `subject_ref`/`object_ref` as the wire
//! format on `RelationAssertion` — that cutover is deliberately deferred
//! (see `docs/P3_STATUS.md`). What it does today: give every reference a
//! resolvable identity, and let `assertion_coverage` measure how many
//! existing assertions' `subject_ref`/`object_ref` already resolve to a
//! cataloged entity.

use crate::model::{EntityKind, LabelOrigin, NewEntity};
use crate::catalog_metadata::{CATALOG_SCHEMA_VERSION, ENTITY_RESOLUTION_VERSION};
use crate::store::ProvenanceStore;
use mathesis_graph::GraphStore;
use mathesis_taxonomy::resolve;
use mathesis_taxonomy::store::TaxonomyStore;

/// `import_graph`（`legacy_adapter.rs`）の`ImportStats`と同じ「+N件
/// （skip M件）」形式。`entities`テーブル自体は同じ入力に対して増え
/// 続けないが、再実行のたびに「+4052」とだけ表示すると新規追加の
/// ように見えてしまう——`get_or_insert_entity*`が返す`was_new`を
/// そのまま集計して、実際に増えた件数を正直に報告する。
#[derive(Debug, Default, Clone, Copy)]
pub struct CatalogStats {
    pub judgments: usize,
    pub judgments_skipped_existing: usize,
    pub papers: usize,
    pub papers_skipped_existing: usize,
    pub concepts: usize,
    pub concepts_skipped_existing: usize,
    pub concept_aliases: usize,
}

pub const CATALOG_BUILD_VERSION: &str = "mathesis-provenance::catalog-v1";

pub fn catalog_metadata(
    prov: &ProvenanceStore,
    graph_input_sha256: String,
    taxonomy_input_sha256: String,
) -> anyhow::Result<crate::model::CatalogMetadata> {
    Ok(crate::model::CatalogMetadata {
        schema_version: CATALOG_SCHEMA_VERSION,
        build_version: CATALOG_BUILD_VERSION.to_string(),
        entity_resolution_version: ENTITY_RESOLUTION_VERSION.to_string(),
        graph_input_sha256,
        taxonomy_input_sha256,
        entity_count: prov.entity_count()?,
        alias_count: prov
            .conn
            .query_row("SELECT COUNT(*) FROM entity_refs", [], |row| row.get(0))?,
    })
}

/// `judgment:<id>`/`paper:<arxiv_id>`エンティティを`mathesis-graph`から作る。
/// 表示名は既存フィールドの転記のみ——`name`が無いjudgmentは種別だけの
/// プレースホルダ（"(anonymous theorem)"等）にする。無い情報を補わない。
pub fn build_judgment_paper_catalog(graph: &GraphStore, prov: &ProvenanceStore) -> anyhow::Result<CatalogStats> {
    let mut stats = CatalogStats::default();
    for j in graph.list_judgments()? {
        let label_origin = if j.name.is_some() { LabelOrigin::SourceProvided } else { LabelOrigin::FallbackIdentifier };
        let label = j.name.clone().unwrap_or_else(|| format!("(anonymous {})", j.kind.as_str()));
        let (entity_id, was_new) = prov.get_or_insert_entity(
            &NewEntity { kind: EntityKind::Judgment, display_label: label, source_record_id: None },
            &format!("judgment:{}", j.id.0),
        )?;
        prov.set_label_origin(entity_id, label_origin)?;
        if was_new { stats.judgments += 1 } else { stats.judgments_skipped_existing += 1 };
    }
    for p in graph.list_papers()? {
        let label_origin = if p.title.is_some() { LabelOrigin::SourceProvided } else { LabelOrigin::FallbackIdentifier };
        let label = p.title.clone().unwrap_or_else(|| p.arxiv_id.clone());
        let (entity_id, was_new) = prov.get_or_insert_entity(
            &NewEntity { kind: EntityKind::Paper, display_label: label, source_record_id: None },
            &format!("paper:{}", p.arxiv_id),
        )?;
        prov.set_label_origin(entity_id, label_origin)?;
        if was_new { stats.papers += 1 } else { stats.papers_skipped_existing += 1 };
    }
    Ok(stats)
}

/// `concept:<representative>`エンティティを`mathesis-taxonomy`から作る。
/// `resolve::resolve`は`mathesis-taxonomy export`が検索索引を組み立てる
/// ときと**同じ**呼び出し規約（候補配列そのままの順序で渡す）——ここだけ
/// 別のEntity Resolution結果を作ってしまうと、`web/`が見ている代表表記と
/// 食い違いかねない。
pub fn build_concept_catalog(taxonomy: &TaxonomyStore, prov: &ProvenanceStore) -> anyhow::Result<CatalogStats> {
    let mut stats = CatalogStats::default();
    let candidates = taxonomy.list_candidates()?;
    let phrases: Vec<String> = candidates.iter().map(|c| c.phrase.clone()).collect();
    let doc_freqs: Vec<usize> = candidates.iter().map(|c| c.doc_freq).collect();
    let resolved = resolve::resolve(&phrases, &doc_freqs);

    for group in &resolved {
        let mut refs = Vec::with_capacity(1 + group.aliases.len());
        refs.push(format!("concept:{}", group.representative));
        refs.extend(group.aliases.iter().map(|a| format!("concept:{a}")));
        let (entity_id, was_new) = prov.get_or_insert_entity_with_refs(
            &NewEntity { kind: EntityKind::Concept, display_label: group.representative.clone(), source_record_id: None },
            &refs,
        )?;
        prov.set_label_origin(entity_id, LabelOrigin::Canonicalized)?;
        if was_new { stats.concepts += 1 } else { stats.concepts_skipped_existing += 1 };
        stats.concept_aliases += group.aliases.len();
    }
    Ok(stats)
}

#[derive(Debug, Default)]
pub struct CoverageReport {
    pub total_refs: usize,
    pub resolved_refs: usize,
    pub unresolved_examples: Vec<String>,
}

impl CoverageReport {
    pub fn print(&self) {
        println!("assertion reference coverage: {}/{} subject/object refs resolve to a cataloged entity", self.resolved_refs, self.total_refs);
        if !self.unresolved_examples.is_empty() {
            println!("  unresolved examples (up to 10):");
            for r in &self.unresolved_examples {
                println!("    {r}");
            }
        }
    }
}

/// 今のDBにある全assertionの`subject_ref`/`object_ref`が、カタログへ実際に
/// 引けるかを集計する。**リリースゲートではない**——`web-export`/`verify-release`
/// はまだこれを見ない（`docs/P3_STATUS.md`「まだやっていないこと」参照）。
/// `stats`が参考情報として表示するための、素朴な集計。
pub fn assertion_reference_coverage(prov: &ProvenanceStore) -> anyhow::Result<CoverageReport> {
    let mut report = CoverageReport::default();
    for a in prov.list_assertions()? {
        for r in [&a.subject_ref, &a.object_ref] {
            report.total_refs += 1;
            if prov.resolve_entity_ref(r)?.is_some() {
                report.resolved_refs += 1;
            } else if report.unresolved_examples.len() < 10 {
                report.unresolved_examples.push(r.clone());
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EpistemicState, NewRelationAssertion, NewRelease, RelationKind};

    fn judgment(store: &GraphStore, name: &str) -> mathesis_graph::model::JudgmentId {
        let expr = mathesis_ast::parse_expr("True").unwrap().expr;
        let statement = store.intern_expr(&expr).unwrap();
        store
            .insert_judgment(&mathesis_graph::model::NewJudgment {
                kind: mathesis_graph::model::JudgmentKind::Theorem,
                name: Some(name.into()),
                context: vec![],
                statement,
                definition_body_raw: None,
                source: mathesis_graph::model::SourceRef { file: "t.lean".into(), line: 1 },
                raw_text: format!("theorem {name} : True"),
                parse_status: mathesis_graph::model::ParseStatus::Full,
                source_paper: None,
            })
            .unwrap()
    }

    #[test]
    fn judgment_and_paper_catalog_uses_existing_labels_without_inventing_any() {
        let graph = GraphStore::open_in_memory().unwrap();
        let named = judgment(&graph, "add_assoc");
        let anon_expr = mathesis_ast::parse_expr("True").unwrap().expr;
        let anon_statement = graph.intern_expr(&anon_expr).unwrap();
        let anon = graph
            .insert_judgment(&mathesis_graph::model::NewJudgment {
                kind: mathesis_graph::model::JudgmentKind::Theorem,
                name: None,
                context: vec![],
                statement: anon_statement,
                definition_body_raw: None,
                source: mathesis_graph::model::SourceRef { file: "t.lean".into(), line: 2 },
                raw_text: "theorem : True".into(),
                parse_status: mathesis_graph::model::ParseStatus::Full,
                source_paper: None,
            })
            .unwrap();
        graph.intern_paper("math/0001", Some("A real title")).unwrap();
        graph.intern_paper("math/0002", None).unwrap();

        let prov = ProvenanceStore::open_in_memory().unwrap();
        let stats = build_judgment_paper_catalog(&graph, &prov).unwrap();
        assert_eq!(stats.judgments, 2);
        assert_eq!(stats.papers, 2);

        let named_id = prov.resolve_entity_ref(&format!("judgment:{}", named.0)).unwrap().unwrap();
        assert_eq!(prov.get_entity(named_id).unwrap().display_label, "add_assoc");

        let anon_id = prov.resolve_entity_ref(&format!("judgment:{}", anon.0)).unwrap().unwrap();
        assert!(prov.get_entity(anon_id).unwrap().display_label.contains("anonymous"), "無い名前を捏造しない");

        let titled = prov.resolve_entity_ref("paper:math/0001").unwrap().unwrap();
        assert_eq!(prov.get_entity(titled).unwrap().display_label, "A real title");
        let untitled = prov.resolve_entity_ref("paper:math/0002").unwrap().unwrap();
        assert_eq!(prov.get_entity(untitled).unwrap().display_label, "math/0002", "題名が無ければarXiv idで代用、捏造しない");
    }

    #[test]
    fn concept_catalog_folds_spelling_variants_the_same_way_taxonomy_export_does() {
        let mut taxonomy = TaxonomyStore::open(std::path::Path::new(":memory:")).unwrap();
        taxonomy
            .replace_all(&mathesis_taxonomy::concepts::ExtractionResult {
                candidates: vec![
                    mathesis_taxonomy::concepts::ConceptCandidate {
                        phrase: "kahler manifolds".into(),
                        word_count: 2,
                        doc_freq: 3,
                        mean_score: 0.0,
                        msc_code: None,
                        sample_arxiv_ids: vec![],
                        field_concentration: None,
                    },
                    mathesis_taxonomy::concepts::ConceptCandidate {
                        phrase: "kahler manifold".into(),
                        word_count: 2,
                        doc_freq: 7,
                        mean_score: 0.0,
                        msc_code: None,
                        sample_arxiv_ids: vec![],
                        field_concentration: None,
                    },
                ],
                paper_links: vec![],
                dropped_as_boilerplate: vec![],
            })
            .unwrap();

        let prov = ProvenanceStore::open_in_memory().unwrap();
        let stats = build_concept_catalog(&taxonomy, &prov).unwrap();
        assert_eq!(stats.concepts, 1, "表記ゆれ2件は1概念に畳まれるべき");
        assert_eq!(stats.concept_aliases, 1);

        let singular = prov.resolve_entity_ref("concept:kahler manifold").unwrap().unwrap();
        let plural = prov.resolve_entity_ref("concept:kahler manifolds").unwrap().unwrap();
        assert_eq!(singular, plural, "代表表記と表記ゆれは同じエンティティへ解決されるべき");
        assert_eq!(prov.get_entity(singular).unwrap().display_label, "kahler manifold", "文書頻度が高い方が代表になる");
    }

    #[test]
    fn coverage_report_counts_resolved_and_unresolved_references() {
        let prov = ProvenanceStore::open_in_memory().unwrap();
        let release = prov.get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None }).unwrap();
        prov.get_or_insert_entity(&NewEntity { kind: EntityKind::Judgment, display_label: "known".into(), source_record_id: None }, "judgment:1").unwrap();

        prov.insert_assertion(&NewRelationAssertion {
            subject_ref: "judgment:1".into(), // カタログ済み
            predicate: RelationKind::DependsOn,
            object_ref: "judgment:2".into(), // カタログ未登録
            epistemic_state: EpistemicState::Extracted,
            score: None,
            policy_version: None,
            created_by_run_id: None,
            supersedes_id: None,
            release_id: release,
            legacy_ref: None,
        })
        .unwrap();

        let report = assertion_reference_coverage(&prov).unwrap();
        assert_eq!(report.total_refs, 2);
        assert_eq!(report.resolved_refs, 1);
        assert_eq!(report.unresolved_examples, vec!["judgment:2".to_string()]);
    }
}

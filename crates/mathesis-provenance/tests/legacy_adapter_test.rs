//! `docs/DATA_DICTIONARY.md`のマッピング表の各行を、実際にアダプタへ通して検証する。

use mathesis_ast::Expr;
use mathesis_graph::{GraphStore, JudgmentKind, MorphismKind, NewJudgment, NewMorphism, ParseStatus, SourceRef};
use mathesis_provenance::legacy_adapter::{import_graph, import_taxonomy_relations};
use mathesis_provenance::model::{EpistemicState, EvidenceKind, NewRelease, RelationKind, ReviewOutcome};
use mathesis_provenance::ProvenanceStore;
use mathesis_taxonomy::relations::{RelationEdge, RelationKind as TaxRelationKind, RelationStatus as TaxRelationStatus};
use mathesis_taxonomy::store::TaxonomyStore;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// `TaxonomyStore::open_in_memory`は`#[cfg(test)]`でtaxonomyクレート自身の
/// テストにしか公開されていないため、外部クレートのテストからは使えない
/// ——一意なパスの一時ファイルで代用する。
fn temp_taxonomy_db() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!("mathesis_provenance_test_{}_{n}.sqlite", std::process::id()))
}

fn insert_judgment(store: &GraphStore, name: &str, paper: Option<mathesis_graph::PaperId>) -> mathesis_graph::JudgmentId {
    let statement = store.intern_expr(&Expr::Unparsed("placeholder".into())).unwrap();
    store
        .insert_judgment(&NewJudgment {
            kind: JudgmentKind::Theorem,
            name: Some(name.into()),
            context: vec![],
            statement,
            definition_body_raw: None,
            source: SourceRef { file: format!("{name}.lean"), line: 1 },
            raw_text: format!("theorem {name} : True"),
            parse_status: ParseStatus::Full,
            source_paper: paper,
        })
        .unwrap()
}

fn open_release(prov: &ProvenanceStore, tag: &str) -> mathesis_provenance::ReleaseId {
    prov.get_or_insert_release(&NewRelease { tag: tag.into(), git_commit: None, generated_at_unix: 0, notes: None })
        .unwrap()
}

#[test]
fn judgment_dependency_becomes_depends_on_extracted_not_observed() {
    // 名前一致はraw Lean文からの決定的抽出であって、elaborator検証ではない
    // ——`observed`ではなく`extracted`（`docs/DATA_DICTIONARY.md`「Resolved
    // decisions #3」）。
    let graph = GraphStore::open_in_memory().unwrap();
    let paper = graph.intern_paper("math/0001", None).unwrap();
    let a = insert_judgment(&graph, "a", Some(paper));
    let b = insert_judgment(&graph, "b", Some(paper));
    graph.record_judgment_dependency(a, b).unwrap();

    let prov = ProvenanceStore::open_in_memory().unwrap();
    let release = open_release(&prov, "t");
    let stats = import_graph(&graph, &prov, release, "t").unwrap();
    assert_eq!(stats.dependencies_imported, 1);

    let assertions = prov.list_assertions().unwrap();
    let a = assertions.iter().find(|x| x.predicate == RelationKind::DependsOn).unwrap();
    assert_eq!(a.epistemic_state, EpistemicState::Extracted);
    let evidence = prov.evidence_for(a.id).unwrap();
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0].evidence_kind, EvidenceKind::SourceSpan);
}

#[test]
fn paper_citation_becomes_cites_observed() {
    let graph = GraphStore::open_in_memory().unwrap();
    let citing = graph.intern_paper("math/0002", None).unwrap();
    let cited = graph.intern_paper("math/0001", None).unwrap();
    insert_judgment(&graph, "citing_thm", Some(citing));
    insert_judgment(&graph, "cited_thm", Some(cited));
    graph.record_paper_citation(citing, cited).unwrap();

    let prov = ProvenanceStore::open_in_memory().unwrap();
    let release = open_release(&prov, "t");
    let stats = import_graph(&graph, &prov, release, "t").unwrap();
    assert_eq!(stats.citations_imported, 1);

    let assertions = prov.list_assertions().unwrap();
    let a = assertions.iter().find(|x| x.predicate == RelationKind::Cites).unwrap();
    assert_eq!(a.epistemic_state, EpistemicState::Observed);
    assert_eq!(a.subject_ref, "paper:math/0002");
    assert_eq!(a.object_ref, "paper:math/0001");
}

#[test]
fn manual_accepted_implication_becomes_implies_proposed_with_preserved_review_decision() {
    // reviewer_idが無いのでReviewedには昇格しない（`docs/DATA_DICTIONARY.md`
    // 「Resolved decisions #4」）——だが旧Acceptedという事実はReviewDecision
    // 行として残る。
    let graph = GraphStore::open_in_memory().unwrap();
    let a = insert_judgment(&graph, "a", None);
    let b = insert_judgment(&graph, "b", None);
    graph.annotate(a, b, MorphismKind::Implication, Some("A trivially implies B".into())).unwrap();

    let prov = ProvenanceStore::open_in_memory().unwrap();
    let release = open_release(&prov, "t");
    let stats = import_graph(&graph, &prov, release, "t").unwrap();
    assert_eq!(stats.morphisms_imported, 1);
    assert_eq!(stats.review_decisions_created, 1, "Acceptedという事実はReviewDecisionとして保存する");

    let assertions = prov.list_assertions().unwrap();
    let assertion = assertions.iter().find(|x| x.predicate == RelationKind::Implies).unwrap();
    assert_eq!(assertion.epistemic_state, EpistemicState::Proposed, "ImplicationはdependsOnではなくimpliesへ。Acceptedでもreviewer_id不明なのでProposedのまま");

    let evidence = prov.evidence_for(assertion.id).unwrap();
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0].evidence_kind, EvidenceKind::ReviewerNote);

    let decisions = prov.review_decisions_for(assertion.id).unwrap();
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].decision, ReviewOutcome::Accept);
    assert!(decisions[0].reviewer_id.is_none(), "誰が承認したかは記録が無いので捏造しない");
}

#[test]
fn heuristic_proposed_specialization_becomes_specializes_proposed_without_review_decision() {
    let graph = GraphStore::open_in_memory().unwrap();
    let a = insert_judgment(&graph, "a", None);
    let b = insert_judgment(&graph, "b", None);
    graph
        .insert_morphism(&NewMorphism::heuristic_proposed(a, b, MorphismKind::Specialization, "naming heuristic".into()))
        .unwrap();

    let prov = ProvenanceStore::open_in_memory().unwrap();
    let release = open_release(&prov, "t");
    let stats = import_graph(&graph, &prov, release, "t").unwrap();
    assert_eq!(stats.morphisms_imported, 1);
    assert_eq!(stats.review_decisions_created, 0, "Proposedはreview_decisionを作らない");

    let assertions = prov.list_assertions().unwrap();
    let assertion = assertions.iter().find(|x| x.predicate == RelationKind::Specializes).unwrap();
    assert_eq!(assertion.epistemic_state, EpistemicState::Proposed);
    let evidence = prov.evidence_for(assertion.id).unwrap();
    assert_eq!(evidence[0].evidence_kind, EvidenceKind::ModelOutput);
}

#[test]
fn rerunning_import_graph_is_idempotent() {
    let graph = GraphStore::open_in_memory().unwrap();
    let a = insert_judgment(&graph, "a", None);
    let b = insert_judgment(&graph, "b", None);
    graph.record_judgment_dependency(a, b).unwrap();

    let prov = ProvenanceStore::open_in_memory().unwrap();
    let release = open_release(&prov, "t");
    import_graph(&graph, &prov, release, "t").unwrap();
    let second = import_graph(&graph, &prov, release, "t").unwrap();

    assert_eq!(second.dependencies_imported, 0);
    assert_eq!(second.dependencies_skipped_existing, 1);
    assert_eq!(prov.assertion_count().unwrap(), 1);
}

fn taxonomy_edge(
    subject: &str,
    object: &str,
    kind: TaxRelationKind,
    status: TaxRelationStatus,
    confidence: f32,
    sentence: Option<&str>,
    arxiv_id: Option<&str>,
) -> RelationEdge {
    RelationEdge {
        subject: subject.into(),
        object: object.into(),
        kind,
        status,
        confidence,
        evidence_sentence: sentence.map(String::from),
        evidence_arxiv_id: arxiv_id.map(String::from),
    }
}

#[test]
fn proposed_relation_gets_one_model_output_evidence_with_invcl_metric_and_no_score() {
    // invCLは較正済み確率ではないので、assertion.scoreではなく
    // Evidence.metric_name/metric_valueに置く
    // （`docs/DATA_DICTIONARY.md`「Resolved decisions #1, #5」）。
    let mut taxonomy = TaxonomyStore::open(&temp_taxonomy_db()).unwrap();
    taxonomy
        .save_relations(&[taxonomy_edge("a", "b", TaxRelationKind::SpecializationOf, TaxRelationStatus::Proposed, 0.62, None, None)])
        .unwrap();

    let prov = ProvenanceStore::open_in_memory().unwrap();
    let release = open_release(&prov, "t");
    let stats = import_taxonomy_relations(&taxonomy, &prov, release, "t").unwrap();
    assert_eq!(stats.relations_imported, 1);

    let assertion = &prov.list_assertions().unwrap()[0];
    assert_eq!(assertion.epistemic_state, EpistemicState::Proposed);
    assert_eq!(assertion.score, None, "非較正のinvCLはassertion.scoreに置かない");
    let evidence = prov.evidence_for(assertion.id).unwrap();
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0].evidence_kind, EvidenceKind::ModelOutput);
    assert!(evidence[0].locator.is_none());
    assert_eq!(evidence[0].metric_name.as_deref(), Some("invCL"));
    assert_eq!(evidence[0].metric_value, Some(0.62_f32 as f64));
}

#[test]
fn grounded_relation_gets_one_source_span_evidence_and_no_score() {
    let mut taxonomy = TaxonomyStore::open(&temp_taxonomy_db()).unwrap();
    taxonomy
        .save_relations(&[taxonomy_edge(
            "a",
            "b",
            TaxRelationKind::SpecializationOf,
            TaxRelationStatus::Grounded,
            1.0,
            Some("a is a special case of b."),
            Some("math/0003"),
        )])
        .unwrap();

    let prov = ProvenanceStore::open_in_memory().unwrap();
    let release = open_release(&prov, "t");
    import_taxonomy_relations(&taxonomy, &prov, release, "t").unwrap();

    let assertion = &prov.list_assertions().unwrap()[0];
    assert_eq!(assertion.epistemic_state, EpistemicState::Extracted);
    assert_eq!(assertion.score, None, "Grounded の confidence=1.0 は較正値ではないので score に持ち込まない");
    let evidence = prov.evidence_for(assertion.id).unwrap();
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0].evidence_kind, EvidenceKind::SourceSpan);
    assert_eq!(evidence[0].locator.as_deref(), Some("a is a special case of b."));
}

#[test]
fn confirmed_relation_gets_two_evidence_rows_and_retains_its_real_distributional_score() {
    // 0.81という1.0でない値を使うことで、Confirmedがrelations.rs::merge()の
    // Occupiedブランチ（実測値を保持、1.0に上書きしない）を模していることを
    // 確認する——`docs/DATA_DICTIONARY.md`「Resolved decisions #5」。
    let mut taxonomy = TaxonomyStore::open(&temp_taxonomy_db()).unwrap();
    taxonomy
        .save_relations(&[taxonomy_edge(
            "a",
            "b",
            TaxRelationKind::EquivalentTo,
            TaxRelationStatus::Confirmed,
            0.81,
            Some("a is also known as b."),
            Some("math/0004"),
        )])
        .unwrap();

    let prov = ProvenanceStore::open_in_memory().unwrap();
    let release = open_release(&prov, "t");
    import_taxonomy_relations(&taxonomy, &prov, release, "t").unwrap();

    let assertion = &prov.list_assertions().unwrap()[0];
    assert_eq!(assertion.predicate, RelationKind::EquivalentTo);
    assert_eq!(assertion.epistemic_state, EpistemicState::Extracted);
    assert_eq!(assertion.score, None, "invCLはassertion.scoreに置かない");
    let evidence = prov.evidence_for(assertion.id).unwrap();
    assert_eq!(evidence.len(), 2, "Confirmed は Hearst + distributional の2本");
    let source_span = evidence.iter().find(|e| e.evidence_kind == EvidenceKind::SourceSpan).unwrap();
    assert_eq!(source_span.locator.as_deref(), Some("a is also known as b."));
    let model_output = evidence.iter().find(|e| e.evidence_kind == EvidenceKind::ModelOutput).unwrap();
    assert_eq!(model_output.metric_name.as_deref(), Some("invCL"));
    assert_eq!(model_output.metric_value, Some(0.81_f32 as f64), "1.0固定ではなく実測値が保持されている");
}

#[test]
fn rerunning_import_taxonomy_relations_is_idempotent() {
    let mut taxonomy = TaxonomyStore::open(&temp_taxonomy_db()).unwrap();
    taxonomy
        .save_relations(&[taxonomy_edge("a", "b", TaxRelationKind::SpecializationOf, TaxRelationStatus::Proposed, 0.5, None, None)])
        .unwrap();

    let prov = ProvenanceStore::open_in_memory().unwrap();
    let release = open_release(&prov, "t");
    import_taxonomy_relations(&taxonomy, &prov, release, "t").unwrap();
    let second = import_taxonomy_relations(&taxonomy, &prov, release, "t").unwrap();

    assert_eq!(second.relations_imported, 0);
    assert_eq!(second.relations_skipped_existing, 1);
    assert_eq!(prov.assertion_count().unwrap(), 1);
}

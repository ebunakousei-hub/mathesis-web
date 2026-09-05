//! Phase 9: 論文（arXiv）ノードと判断ノード間の依存関係辺。
//! `crates/mathesis-taxonomy`/`mathesis-fulltext`の概念タクソノミー側と、
//! この判断グラフ（Lean/Coqの形式証明）を「合流」させる橋渡し。

use mathesis_ast::parse_expr;
use mathesis_graph::{GraphStore, JudgmentId, JudgmentKind, NewJudgment, ParseStatus, SourceRef};

fn insert_named(store: &GraphStore, name: &str, stmt: &str) -> JudgmentId {
    let expr = parse_expr(stmt).unwrap().expr;
    let statement = store.intern_expr(&expr).unwrap();
    store
        .insert_judgment(&NewJudgment {
            kind: JudgmentKind::Theorem,
            name: Some(name.into()),
            context: vec![],
            statement,
            definition_body_raw: None,
            source: SourceRef { file: "phase9.lean".into(), line: 1 },
            raw_text: format!("theorem {name} : {stmt}"),
            parse_status: ParseStatus::Full,
            source_paper: None,
        })
        .unwrap()
}

#[test]
fn interning_a_paper_twice_by_arxiv_id_returns_the_same_id() {
    let store = GraphStore::open_in_memory().unwrap();
    let a = store.intern_paper("2604.05984", Some("Formalization of De Giorgi--Nash--Moser Theory in Lean")).unwrap();
    let b = store.intern_paper("2604.05984", None).unwrap();
    assert_eq!(a, b, "同じarxiv_idは同じPaperIdへ解決されるべき");
}

#[test]
fn find_paper_by_arxiv_id_round_trips() {
    let store = GraphStore::open_in_memory().unwrap();
    let id = store.intern_paper("2604.05984", Some("DeGiorgi")).unwrap();
    let found = store.find_paper_by_arxiv_id("2604.05984").unwrap().unwrap();
    assert_eq!(found.id, id);
    assert_eq!(found.title.as_deref(), Some("DeGiorgi"));
    assert!(store.find_paper_by_arxiv_id("nonexistent").unwrap().is_none());
}

#[test]
fn a_judgment_can_be_linked_to_its_source_paper() {
    let store = GraphStore::open_in_memory().unwrap();
    let paper = store.intern_paper("2604.05984", Some("DeGiorgi")).unwrap();

    let expr = parse_expr("True").unwrap().expr;
    let statement = store.intern_expr(&expr).unwrap();
    let judgment_id = store
        .insert_judgment(&NewJudgment {
            kind: JudgmentKind::Theorem,
            name: Some("weak_harnack".into()),
            context: vec![],
            statement,
            definition_body_raw: None,
            source: SourceRef { file: "WeakHarnack.lean".into(), line: 42 },
            raw_text: "theorem weak_harnack : True := trivial".into(),
            parse_status: ParseStatus::Full,
            source_paper: Some(paper),
        })
        .unwrap();

    let record = store.get_judgment(judgment_id).unwrap();
    assert_eq!(record.source_paper, Some(paper));
    assert_eq!(store.judgments_of_paper(paper).unwrap(), vec![judgment_id]);
}

#[test]
fn judgments_without_a_paper_link_default_to_none() {
    let store = GraphStore::open_in_memory().unwrap();
    let id = insert_named(&store, "standalone", "True");
    let record = store.get_judgment(id).unwrap();
    assert_eq!(record.source_paper, None);
}

#[test]
fn judgment_dependency_edges_round_trip_in_both_directions() {
    let store = GraphStore::open_in_memory().unwrap();
    let base = insert_named(&store, "base_lemma", "True");
    let derived = insert_named(&store, "derived_thm", "True");

    store.record_judgment_dependency(derived, base).unwrap();

    assert_eq!(store.dependencies_of(derived).unwrap(), vec![base]);
    assert_eq!(store.dependents_of(base).unwrap(), vec![derived]);
    assert!(store.dependencies_of(base).unwrap().is_empty(), "逆方向には辺が無い");
    assert_eq!(store.judgment_dependency_count().unwrap(), 1);
}

#[test]
fn recording_the_same_dependency_twice_is_idempotent() {
    let store = GraphStore::open_in_memory().unwrap();
    let a = insert_named(&store, "a", "True");
    let b = insert_named(&store, "b", "True");

    store.record_judgment_dependency(a, b).unwrap();
    store.record_judgment_dependency(a, b).unwrap();

    assert_eq!(store.judgment_dependency_count().unwrap(), 1, "重ねて呼んでも冪等であるべき");
}

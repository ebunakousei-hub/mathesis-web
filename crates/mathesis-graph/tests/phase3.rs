use mathesis_ast::parse_expr;
use mathesis_graph::{
    analyze_dependencies, GraphStore, JudgmentId, JudgmentKind, MorphismKind, NewJudgment,
    ParseStatus, ProofTerm, SourceRef,
};

fn insert_test_judgment(store: &GraphStore, name: &str, stmt: &str) -> JudgmentId {
    let expr = parse_expr(stmt).unwrap().expr;
    let statement = store.intern_expr(&expr).unwrap();
    store
        .insert_judgment(&NewJudgment {
            kind: JudgmentKind::Theorem,
            name: Some(name.into()),
            context: vec![],
            statement,
            definition_body_raw: None,
            source: SourceRef {
                file: "phase3.lean".into(),
                line: 1,
            },
            raw_text: format!("theorem {name} : {stmt}"),
            parse_status: ParseStatus::Full,
            source_paper: None,
        })
        .unwrap()
}

#[test]
fn proof_term_interning_and_deduplication() {
    let store = GraphStore::open_in_memory().unwrap();

    let p1 = ProofTerm::TacticScript("induction a with | zero => simp | succ a ih => rw [ih]".into());
    let p2 = ProofTerm::TacticScript("induction a with | zero => simp | succ a ih => rw [ih]".into());

    let h1 = store.intern_proof_term(&p1).unwrap();
    let h2 = store.intern_proof_term(&p2).unwrap();

    assert_eq!(h1, h2, "同一構造の証明項は同じハッシュに解決される");
    assert_eq!(store.proof_term_count().unwrap(), 1, "DB上に1つのみノードが作成される");

    let retrieved = store.get_proof_term(&h1).unwrap();
    assert_eq!(retrieved, p1);
}

#[test]
fn static_dependency_analysis_and_signature() {
    let term = ProofTerm::App(
        Box::new(ProofTerm::Ref("group_hom_preserves_identity".into())),
        vec![
            ProofTerm::Ref("group_mul_one".into()),
            ProofTerm::Ref("group_inv_mul".into()),
        ],
    );

    let analysis = analyze_dependencies(&term);
    assert_eq!(analysis.must_have_refs.len(), 3);
    assert!(analysis.must_have_refs.contains("group_hom_preserves_identity"));
    assert!(analysis.must_have_refs.contains("group_mul_one"));
    assert!(analysis.must_have_refs.contains("group_inv_mul"));

    let sig1 = analysis.compute_signature();

    // 順序を変えても確定的なシグネチャが生成されること
    let term2 = ProofTerm::App(
        Box::new(ProofTerm::Ref("group_inv_mul".into())),
        vec![
            ProofTerm::Ref("group_hom_preserves_identity".into()),
            ProofTerm::Ref("group_mul_one".into()),
        ],
    );
    let analysis2 = analyze_dependencies(&term2);
    let sig2 = analysis2.compute_signature();

    assert_eq!(sig1, sig2, "依存シグネチャは順序非依存で決定論的である");
}

#[test]
fn attach_proof_term_to_morphism() {
    let store = GraphStore::open_in_memory().unwrap();
    let a = insert_test_judgment(&store, "add_comm", "a + b = b + a");
    let b = insert_test_judgment(&store, "add_assoc", "a + b + c = a + (b + c)");

    let mid = store
        .annotate(a, b, MorphismKind::Implication, Some("proof test".into()))
        .unwrap();

    let rec_before = store.get_morphism(mid).unwrap();
    assert!(rec_before.proof_term_hash.is_none());
    assert!(rec_before.dependency_signature.is_none());

    let proof = ProofTerm::TacticScript("by exact add_comm a b; simp [add_zero]".into());
    store.attach_proof_term(mid, &proof).unwrap();

    let rec_after = store.get_morphism(mid).unwrap();
    assert!(rec_after.proof_term_hash.is_some());
    assert!(rec_after.dependency_signature.is_some());

    let proof_hash = rec_after.proof_term_hash.unwrap();
    let dep_sig = rec_after.dependency_signature.unwrap();

    let found_by_hash = store.find_morphisms_by_proof_hash(&proof_hash).unwrap();
    assert_eq!(found_by_hash.len(), 1);
    assert_eq!(found_by_hash[0].id, mid);

    let found_by_sig = store.find_morphisms_by_dependency_signature(&dep_sig).unwrap();
    assert_eq!(found_by_sig.len(), 1);
    assert_eq!(found_by_sig[0].id, mid);
}

#[test]
fn tactic_script_dependency_extraction() {
    let script = ProofTerm::TacticScript("rw [abelian_mul_comm]; exact lagrange_theorem H".into());
    let analysis = analyze_dependencies(&script);
    assert!(analysis.must_have_refs.contains("abelian_mul_comm"));
    assert!(analysis.must_have_refs.contains("lagrange_theorem"));
}

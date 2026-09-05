use mathesis_ast::parse_expr;
use mathesis_graph::{
    strategy_names, FailurePattern, GraphError, GraphStore, JudgmentId, JudgmentKind,
    MorphismKind, NewFailedAttempt, NewJudgment, ParseStatus, SourceRef, ValidationError,
};

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
            source: SourceRef {
                file: "phase4.lean".into(),
                line: 1,
            },
            raw_text: format!("theorem {name} : {stmt}"),
            parse_status: ParseStatus::Full,
            source_paper: None,
        })
        .unwrap()
}

#[test]
fn intern_strategy_deduplicates_by_name() {
    let store = GraphStore::open_in_memory().unwrap();
    let a = store
        .intern_strategy(strategy_names::INDUCTION, Some("数学的帰納法"))
        .unwrap();
    let b = store.intern_strategy(strategy_names::INDUCTION, None).unwrap();
    assert_eq!(a, b, "同名の戦略は同じノードに解決される");
    assert_eq!(store.list_strategies().unwrap().len(), 1);

    let rec = store.get_strategy(a).unwrap();
    assert_eq!(rec.name, "induction");
    assert_eq!(rec.description.as_deref(), Some("数学的帰納法"));
}

#[test]
fn tag_morphism_with_multiple_strategies_and_query_by_strategy() {
    let store = GraphStore::open_in_memory().unwrap();
    let a = insert_named(&store, "add_comm", "a + b = b + a");
    let b = insert_named(&store, "add_assoc", "a + b + c = a + (b + c)");
    let mid = store
        .annotate(a, b, MorphismKind::Implication, Some("sketch".into()))
        .unwrap();

    let induction = store.intern_strategy(strategy_names::INDUCTION, None).unwrap();
    let case_split = store.intern_strategy(strategy_names::CASE_SPLIT, None).unwrap();

    store.tag_morphism_strategy(mid, induction).unwrap();
    store.tag_morphism_strategy(mid, case_split).unwrap();
    // 同じ組を重ねて付けても冪等
    store.tag_morphism_strategy(mid, induction).unwrap();

    let tags = store.strategies_of_morphism(mid).unwrap();
    assert_eq!(tags.len(), 2);
    assert!(tags.iter().any(|s| s.name == "induction"));
    assert!(tags.iter().any(|s| s.name == "case_split"));

    let by_induction = store.morphisms_by_strategy(induction, None).unwrap();
    assert_eq!(by_induction.len(), 1);
    assert_eq!(by_induction[0].id, mid);

    let contradiction = store
        .intern_strategy(strategy_names::CONTRADICTION, None)
        .unwrap();
    assert!(store.morphisms_by_strategy(contradiction, None).unwrap().is_empty());
}

#[test]
fn tagging_unknown_morphism_or_strategy_errors() {
    let store = GraphStore::open_in_memory().unwrap();
    let a = insert_named(&store, "a", "P");
    let b = insert_named(&store, "b", "Q");
    let mid = store.annotate(a, b, MorphismKind::Implication, None).unwrap();
    let strat = store.intern_strategy(strategy_names::INDUCTION, None).unwrap();

    let bogus_morphism = mathesis_graph::MorphismId(999_999);
    let err = store
        .tag_morphism_strategy(bogus_morphism, strat)
        .unwrap_err();
    assert!(matches!(
        err,
        GraphError::Validation(ValidationError::MissingMorphism(_))
    ));

    let bogus_strategy = mathesis_graph::StrategyId(999_999);
    let err = store.tag_morphism_strategy(mid, bogus_strategy).unwrap_err();
    assert!(matches!(
        err,
        GraphError::Validation(ValidationError::StrategyNotFound(_))
    ));
}

#[test]
fn record_failed_attempt_and_query_by_target() {
    let store = GraphStore::open_in_memory().unwrap();
    let goal = insert_named(&store, "fermat_like_conjecture", "P");

    let first = store
        .record_failed_attempt(
            &NewFailedAttempt::new("直接帰納法で試みたが2段階目で崩れた", FailurePattern::InductionStep)
                .with_target(goal)
                .with_detail("step 2"),
        )
        .unwrap();
    let second = store
        .record_failed_attempt(
            &NewFailedAttempt::new("使える戦略を使い果たした", FailurePattern::StrategyExhausted)
                .with_target(goal),
        )
        .unwrap();

    let induction = store.intern_strategy(strategy_names::INDUCTION, None).unwrap();
    store.tag_failed_attempt_strategy(first, induction).unwrap();

    let history = store.failed_attempts_for_target(goal).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].id, first);
    assert_eq!(history[1].id, second);
    assert_eq!(history[0].pattern, FailurePattern::InductionStep);
    assert_eq!(history[0].detail.as_deref(), Some("step 2"));

    let tags = store.strategies_of_failed_attempt(first).unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].name, "induction");

    let by_pattern = store
        .failed_attempts_by_pattern(FailurePattern::StrategyExhausted)
        .unwrap();
    assert_eq!(by_pattern.len(), 1);
    assert_eq!(by_pattern[0].id, second);

    let fetched = store.get_failed_attempt(first).unwrap();
    assert_eq!(fetched.goal_text, "直接帰納法で試みたが2段階目で崩れた");
    assert_eq!(fetched.target, Some(goal));
}

#[test]
fn counterexample_marks_target_as_refuted() {
    let store = GraphStore::open_in_memory().unwrap();
    let conjecture = insert_named(&store, "false_conjecture", "P");
    let untouched = insert_named(&store, "unrelated", "Q");

    assert!(!store.is_refuted(conjecture).unwrap());

    store
        .record_failed_attempt(
            &NewFailedAttempt::new("n=4 で反例が見つかった", FailurePattern::Counterexample)
                .with_target(conjecture),
        )
        .unwrap();

    assert!(store.is_refuted(conjecture).unwrap());
    assert!(!store.is_refuted(untouched).unwrap());
    assert!(FailurePattern::Counterexample.refutes_target());
    assert!(!FailurePattern::InductionStep.refutes_target());
}

#[test]
fn failed_attempt_without_target_is_allowed() {
    let store = GraphStore::open_in_memory().unwrap();
    let id = store
        .record_failed_attempt(&NewFailedAttempt::new(
            "まだ定式化していない探索的な予想でうまくいかなかった",
            FailurePattern::Other,
        ))
        .unwrap();
    let rec = store.get_failed_attempt(id).unwrap();
    assert_eq!(rec.target, None);
    assert_eq!(rec.pattern, FailurePattern::Other);
}

#[test]
fn record_failed_attempt_missing_target_errors() {
    let store = GraphStore::open_in_memory().unwrap();
    let bogus = JudgmentId(999_999);
    let err = store
        .record_failed_attempt(
            &NewFailedAttempt::new("goal", FailurePattern::Other).with_target(bogus),
        )
        .unwrap_err();
    assert!(matches!(
        err,
        GraphError::Validation(ValidationError::MissingTargetJudgment(_))
    ));
}

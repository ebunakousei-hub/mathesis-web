use mathesis_ast::parse_expr;
use mathesis_graph::{
    EdgeStatus, GraphError, GraphStore, Hypothesis, JudgmentId, JudgmentKind,
    MorphismKind, NewJudgment, ParseStatus, SourceRef, ValidationError,
};

fn insert_named(
    store: &GraphStore,
    name: &str,
    stmt: &str,
    context: Vec<Hypothesis>,
) -> JudgmentId {
    let expr = parse_expr(stmt).unwrap().expr;
    let statement = store.intern_expr(&expr).unwrap();
    store
        .insert_judgment(&NewJudgment {
            kind: JudgmentKind::Theorem,
            name: Some(name.into()),
            context,
            statement,
            definition_body_raw: None,
            source: SourceRef {
                file: "phase2.lean".into(),
                line: 1,
            },
            raw_text: format!("theorem {name} : {stmt}"),
            parse_status: ParseStatus::Full,
            source_paper: None,
        })
        .unwrap()
}

#[test]
fn annotate_implication_and_list_incident_edges() {
    let store = GraphStore::open_in_memory().unwrap();
    let nat = store.intern_expr(&parse_expr("ℕ").unwrap().expr).unwrap();
    let ctx = vec![
        Hypothesis {
            name: "a".into(),
            ty: nat,
        },
        Hypothesis {
            name: "b".into(),
            ty: nat,
        },
    ];
    let comm = insert_named(&store, "add_comm", "a + b = b + a", ctx.clone());
    let assoc = insert_named(&store, "add_assoc", "a + b + c = a + (b + c)", ctx);

    let id = store
        .annotate(
            comm,
            assoc,
            MorphismKind::Implication,
            Some("manual: comm used in assoc sketch".into()),
        )
        .unwrap();

    let rec = store.get_morphism(id).unwrap();
    assert_eq!(rec.kind, MorphismKind::Implication);
    assert_eq!(rec.status, EdgeStatus::Accepted);
    assert_eq!(rec.origin, mathesis_graph::EdgeOrigin::Manual);
    assert!(rec.proof_term_hash.is_none());

    let out = store
        .outgoing(comm, Some(MorphismKind::Implication), Some(EdgeStatus::Accepted))
        .unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].dst, assoc);

    let inn = store.incoming(assoc, None, Some(EdgeStatus::Accepted)).unwrap();
    assert_eq!(inn.len(), 1);
}

#[test]
fn rejects_self_loop_and_missing_endpoint() {
    let store = GraphStore::open_in_memory().unwrap();
    let a = insert_named(&store, "t", "a = a", vec![]);

    let err = store
        .annotate(a, a, MorphismKind::Implication, None)
        .unwrap_err();
    assert!(matches!(err, GraphError::Validation(ValidationError::SelfLoop)));

    let missing = JudgmentId(999);
    let err = store
        .annotate(a, missing, MorphismKind::Equivalence, None)
        .unwrap_err();
    assert!(matches!(
        err,
        GraphError::Validation(ValidationError::MissingEndpoint { .. })
    ));
}

#[test]
fn exclusive_kinds_on_the_same_directed_pair() {
    let store = GraphStore::open_in_memory().unwrap();
    let a = insert_named(&store, "group_assoc", "a * b * c = a * (b * c)", vec![]);
    let b = insert_named(&store, "abelian_assoc", "a * b * c = a * (b * c)", vec![]);

    store
        .annotate(a, b, MorphismKind::Specialization, Some("group ⇒ abelian".into()))
        .unwrap();

    let err = store
        .annotate(a, b, MorphismKind::Implication, None)
        .unwrap_err();
    assert!(matches!(
        err,
        GraphError::Validation(ValidationError::ExclusiveKindConflict { .. })
    ));

    // Dual direction is allowed: specialization's inverse is generalization.
    store
        .annotate(b, a, MorphismKind::Generalization, Some("inverse".into()))
        .unwrap();
    assert_eq!(store.morphism_count().unwrap(), 2);
}

#[test]
fn heuristics_propose_but_do_not_accept() {
    let store = GraphStore::open_in_memory().unwrap();
    let nat = store.intern_expr(&parse_expr("ℕ").unwrap().expr).unwrap();
    let ctx = vec![Hypothesis {
        name: "a".into(),
        ty: nat,
    }];
    insert_named(&store, "add_zero", "a + 0 = a", ctx.clone());
    insert_named(&store, "add_zero_dup", "a + 0 = a", ctx);

    let ids = store.propose_morphisms().unwrap();
    assert!(!ids.is_empty());

    let proposed = store
        .list_morphisms_filtered(Some(MorphismKind::Equivalence), Some(EdgeStatus::Proposed))
        .unwrap();
    assert_eq!(proposed.len(), 1);
    assert_eq!(proposed[0].origin, mathesis_graph::EdgeOrigin::Heuristic);

    let accepted = store
        .list_morphisms_filtered(None, Some(EdgeStatus::Accepted))
        .unwrap();
    assert!(accepted.is_empty(), "heuristics must not auto-accept");

    store.accept_morphism(proposed[0].id).unwrap();
    let class = store.equivalence_class(proposed[0].src).unwrap();
    assert_eq!(class.len(), 2);
    assert_eq!(
        store.representative(proposed[0].dst).unwrap(),
        store.representative(proposed[0].src).unwrap()
    );
}

#[test]
fn context_extension_is_specialization() {
    let store = GraphStore::open_in_memory().unwrap();
    let g = store.intern_expr(&parse_expr("G").unwrap().expr).unwrap();
    let group = store
        .intern_expr(&parse_expr("Group G").unwrap().expr)
        .unwrap();
    let comm = store
        .intern_expr(&parse_expr("Comm G").unwrap().expr)
        .unwrap();

    let group_ctx = vec![
        Hypothesis {
            name: "a".into(),
            ty: g,
        },
        Hypothesis {
            name: "inst".into(),
            ty: group,
        },
    ];
    let abelian_ctx = vec![
        Hypothesis {
            name: "a".into(),
            ty: g,
        },
        Hypothesis {
            name: "inst".into(),
            ty: group,
        },
        Hypothesis {
            name: "comm".into(),
            ty: comm,
        },
    ];

    let general = insert_named(&store, "mul_one", "a * 1 = a", group_ctx);
    let special = insert_named(&store, "mul_one_abelian", "a * 1 = a", abelian_ctx);

    store.propose_morphisms().unwrap();
    let specs = store
        .list_morphisms_filtered(
            Some(MorphismKind::Specialization),
            Some(EdgeStatus::Proposed),
        )
        .unwrap();
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].src, general);
    assert_eq!(specs[0].dst, special);
}

#[test]
fn iff_name_and_qualifier_heuristics() {
    let store = GraphStore::open_in_memory().unwrap();
    insert_named(&store, "add_comm", "a + b = b + a", vec![]);
    insert_named(&store, "add_left_comm", "a + (b + c) = b + (a + c)", vec![]);
    insert_named(
        &store,
        "add_comm_iff_add_left_comm",
        "True",
        vec![],
    );
    insert_named(&store, "group", "True", vec![]);
    insert_named(&store, "abelian_group", "True", vec![]);

    store.propose_morphisms().unwrap();

    let equivs = store
        .list_morphisms_filtered(Some(MorphismKind::Equivalence), Some(EdgeStatus::Proposed))
        .unwrap();
    assert!(
        equivs.iter().any(|e| {
            let names = (
                store.get_judgment(e.src).unwrap().name,
                store.get_judgment(e.dst).unwrap().name,
            );
            matches!(
                (names.0.as_deref(), names.1.as_deref()),
                (Some("add_comm"), Some("add_left_comm"))
                    | (Some("add_left_comm"), Some("add_comm"))
            )
        }),
        "iff name should propose equivalence; got {equivs:?}"
    );

    let specs = store
        .list_morphisms_filtered(
            Some(MorphismKind::Specialization),
            Some(EdgeStatus::Proposed),
        )
        .unwrap();
    assert!(specs.iter().any(|e| {
        store.get_judgment(e.src).unwrap().name.as_deref() == Some("group")
            && store.get_judgment(e.dst).unwrap().name.as_deref() == Some("abelian_group")
    }));
}

#[test]
fn new_name_pattern_heuristics() {
    let store = GraphStore::open_in_memory().unwrap();
    let foo = insert_named(&store, "foo", "True", vec![]);
    let bar = insert_named(&store, "bar", "True", vec![]);
    insert_named(&store, "bar_of_foo", "True", vec![]);
    insert_named(&store, "foo_implies_bar", "True", vec![]);
    insert_named(&store, "foo_generalizes_bar", "True", vec![]);
    let cor = insert_named(&store, "corollary_foo", "True", vec![]);

    store.propose_morphisms().unwrap();

    let proposed = store
        .list_morphisms_filtered(None, Some(EdgeStatus::Proposed))
        .unwrap();

    // Check bar_of_foo or foo_implies_bar -> foo -> bar (Implication)
    assert!(proposed.iter().any(|m| m.src == foo && m.dst == bar && m.kind == MorphismKind::Implication));

    // Check foo_generalizes_bar -> foo -> bar (Specialization)
    assert!(proposed.iter().any(|m| m.src == foo && m.dst == bar && m.kind == MorphismKind::Specialization));

    // Check corollary_foo -> foo -> corollary_foo (Implication)
    assert!(proposed.iter().any(|m| m.src == foo && m.dst == cor && m.kind == MorphismKind::Implication));
}

#[test]
fn rejected_heuristics_are_not_reprised() {
    let store = GraphStore::open_in_memory().unwrap();
    insert_named(&store, "dup_a", "x = x", vec![]);
    insert_named(&store, "dup_b", "x = x", vec![]);

    let ids = store.propose_morphisms().unwrap();
    assert_eq!(ids.len(), 1);
    store.reject_morphism(ids[0]).unwrap();

    let again = store.propose_morphisms().unwrap();
    assert!(again.is_empty());
    assert_eq!(
        store
            .list_morphisms_filtered(None, Some(EdgeStatus::Rejected))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn quotient_collapses_equivalence_and_keeps_implication() {
    let store = GraphStore::open_in_memory().unwrap();
    let a = insert_named(&store, "a", "P", vec![]);
    let b = insert_named(&store, "b", "P", vec![]);
    let c = insert_named(&store, "c", "Q", vec![]);

    store
        .annotate(a, b, MorphismKind::Equivalence, Some("same P".into()))
        .unwrap();
    store
        .annotate(a, c, MorphismKind::Implication, Some("P → Q".into()))
        .unwrap();

    let q = store.quotient_graph().unwrap();
    assert_eq!(q.class_of(a).len(), 2);
    assert_eq!(q.morphisms.len(), 1);
    assert_eq!(q.morphisms[0].kind, MorphismKind::Implication);
    assert_eq!(q.morphisms[0].dst, c);
}

#[test]
fn rejecting_accepted_equivalence_splits_the_class_back_apart() {
    // Union-Find は「分割」ができないので、一度受理した同値エッジを reject したときは
    // 永続キャッシュを全部作り直す必要がある（sync_quotient の差分更新では対応できない）。
    let store = GraphStore::open_in_memory().unwrap();
    let a = insert_named(&store, "a", "P", vec![]);
    let b = insert_named(&store, "b", "P", vec![]);

    let eq = store
        .annotate(a, b, MorphismKind::Equivalence, Some("same P".into()))
        .unwrap();
    assert_eq!(store.quotient_graph().unwrap().class_of(a).len(), 2);
    assert_eq!(store.representative(a).unwrap(), store.representative(b).unwrap());

    store.reject_morphism(eq).unwrap();

    let q = store.quotient_graph().unwrap();
    assert_eq!(q.class_of(a).len(), 1, "reject後はaは自分だけの単集合に戻る");
    assert_eq!(q.class_of(b).len(), 1);
    assert_eq!(store.representative(a).unwrap(), a);
    assert_eq!(store.representative(b).unwrap(), b);
}

#[test]
fn out_of_order_acceptance_is_still_absorbed_correctly() {
    // 単純な「最大IDウォーターマーク」方式だと、先に挿入された（IDが小さい）
    // Proposed な同値射が、後から受理された（IDが大きい）同値射より後に
    // accept_morphism されたとき、ウォーターマークを既に追い越しているとして
    // 取りこぼしてしまう。射ID集合で「取り込み済みか」を管理しているので
    // この順序でも正しく取り込めることを確認する。
    let store = GraphStore::open_in_memory().unwrap();
    let a = insert_named(&store, "a", "P", vec![]);
    let b = insert_named(&store, "b", "P", vec![]);
    let c = insert_named(&store, "c", "P", vec![]);
    let d = insert_named(&store, "d", "P", vec![]);

    // 先に b-c の同値を Proposed として登録（このAPIには直接 propose がないので、
    // ヒューリスティック経由ではなく insert_morphism を Proposed 状態で使う）。
    let bc = store
        .insert_morphism(&mathesis_graph::NewMorphism::heuristic_proposed(
            b,
            c,
            MorphismKind::Equivalence,
            "same statement".into(),
        ))
        .unwrap();

    // 後から挿入した c-d の同値は即座に Accepted（annotate）とし、
    // ウォーターマークを bc より先に進ませる。
    store
        .annotate(c, d, MorphismKind::Equivalence, Some("same P".into()))
        .unwrap();
    assert_eq!(store.quotient_graph().unwrap().class_of(c).len(), 2);

    // 満を持して、IDが小さい bc を後から承認する。
    store.accept_morphism(bc).unwrap();

    let q = store.quotient_graph().unwrap();
    assert_eq!(q.class_of(b).len(), 3, "b, c, d が同じクラスになるはず");
    assert_eq!(store.representative(a).unwrap(), a, "aは無関係のまま");
}

#[test]
fn incremental_proposal_cache_still_compares_new_against_old() {
    // propose_morphisms() は呼び出しのたびに新規判断だけをDBから読み足す
    // （フェーズ3.2「インクリメンタル提案」）。ここでは「1回目の呼び出しの後に
    // 追加された判断」が「1回目より前からある判断」とちゃんと比較されることを
    // 確認する——キャッシュが新規分しか見ていなければ見逃すはずの組み合わせ。
    let store = GraphStore::open_in_memory().unwrap();
    insert_named(&store, "add_zero", "a + 0 = a", vec![]);

    let first = store.propose_morphisms().unwrap();
    assert!(first.is_empty(), "この時点では比較対象が1件しかない");

    // 1回目の呼び出しより後に、同じステートメントを持つ判断を追加する。
    insert_named(&store, "add_zero_dup", "a + 0 = a", vec![]);
    let second = store.propose_morphisms().unwrap();
    assert_eq!(
        second.len(),
        1,
        "新規判断と、1回目の呼び出しより前からある判断の組が見つかるはず"
    );

    let rec = store.get_morphism(second[0]).unwrap();
    assert_eq!(rec.kind, MorphismKind::Equivalence);
}

#[test]
fn incremental_proposal_cache_resolves_old_trigger_with_later_arriving_half() {
    // "foo_implies_bar" のような命名パターンは、パターン自体を持つ判断
    // （トリガー）が先に挿入され、参照先の片方（bar）が後から挿入される順序も
    // あり得る。ヒューリスティック本体は毎回「蓄積済みの全判断」に対して走る
    // ため、キャッシュに新規分しか読み足していなくても、古いトリガーが
    // 新しい参照先を正しく拾えることを確認する。
    // それぞれ別のステートメントにして、命名パターン以外のヒューリスティック
    // （同一ステートメント同士の同値提案など）が紛れ込まないようにする。
    let store = GraphStore::open_in_memory().unwrap();
    let foo = insert_named(&store, "foo", "P", vec![]);
    insert_named(&store, "foo_implies_bar", "Q", vec![]);

    let first = store.propose_morphisms().unwrap();
    assert!(
        first.is_empty(),
        "barがまだ存在しないので、この時点では何も提案されない"
    );

    let bar = insert_named(&store, "bar", "R", vec![]);
    let second = store.propose_morphisms().unwrap();

    assert!(second
        .iter()
        .any(|id| {
            let m = store.get_morphism(*id).unwrap();
            m.src == foo && m.dst == bar && m.kind == MorphismKind::Implication
        }));
}

#[test]
fn validate_counts_morphisms() {
    let store = GraphStore::open_in_memory().unwrap();
    let a = insert_named(&store, "a", "P", vec![]);
    let b = insert_named(&store, "b", "Q", vec![]);
    store
        .annotate(a, b, MorphismKind::Implication, None)
        .unwrap();
    let report = store.validate().unwrap();
    assert!(report.is_valid());
    assert_eq!(report.total_morphisms, 1);
    assert_eq!(report.accepted_morphisms, 1);
}

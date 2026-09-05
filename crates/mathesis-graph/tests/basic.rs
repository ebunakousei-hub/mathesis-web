use mathesis_ast::{parse_expr, Parser};
use mathesis_graph::{
    intern_hypotheses, GraphStore, JudgmentKind, NewJudgment, ParseStatus, SourceRef,
};

#[test]
fn interns_alpha_equivalent_expressions_to_the_same_node() {
    let store = GraphStore::open_in_memory().unwrap();
    let e1 = parse_expr("∀ x, x^2 ≥ 0").unwrap().expr;
    let e2 = parse_expr("∀ y, y^2 ≥ 0").unwrap().expr;
    let id1 = store.intern_expr(&e1).unwrap();
    let id2 = store.intern_expr(&e2).unwrap();
    assert_eq!(id1, id2, "α同値な式は同じ式ノードに解決されるべき");
    assert_eq!(store.expr_count().unwrap(), 1);
}

#[test]
fn same_theorem_name_under_different_contexts_are_distinct_judgments() {
    // 「ペアノ公理下での加法の結合律」と「群論下での結合律」は
    // 前提コンテキストが異なるので別ノードになる、という設計文書の例をなぞる。
    let store = GraphStore::open_in_memory().unwrap();

    // group 版: (a b c : G) [Group G] : a * b * c = a * (b * c)
    let toks1a = mathesis_ast::lex("(a b c : G) [Group G]");
    let mut p1 = Parser::new(&toks1a);
    let binders1 = p1.parse_binder_group_list();
    let toks1b = mathesis_ast::lex("a * b * c = a * (b * c)");
    let mut p1b = Parser::with_seed_scope(
        &toks1b,
        binders1
            .iter()
            .map(|b| (b.hint.clone(), b.var))
            .collect(),
        p1.next_var_counter(),
    );
    let stmt1 = p1b.parse_bp(0);

    // peano 版: (a b c : ℕ) : a + b + c = a + (b + c)
    let toks2a = mathesis_ast::lex("(a b c : ℕ)");
    let mut p2 = Parser::new(&toks2a);
    let binders2 = p2.parse_binder_group_list();
    let toks2b = mathesis_ast::lex("a + b + c = a + (b + c)");
    let mut p2b = Parser::with_seed_scope(
        &toks2b,
        binders2
            .iter()
            .map(|b| (b.hint.clone(), b.var))
            .collect(),
        p2.next_var_counter(),
    );
    let stmt2 = p2b.parse_bp(0);

    let ctx1_exprs: Vec<(String, mathesis_ast::Expr)> = binders1
        .iter()
        .map(|b| (b.hint.clone(), (*b.ty.clone().unwrap())))
        .collect();
    let ctx2_exprs: Vec<(String, mathesis_ast::Expr)> = binders2
        .iter()
        .map(|b| (b.hint.clone(), (*b.ty.clone().unwrap())))
        .collect();

    let ctx1 = intern_hypotheses(&store, &ctx1_exprs).unwrap();
    let ctx2 = intern_hypotheses(&store, &ctx2_exprs).unwrap();

    let stmt1_id = store.intern_expr(&stmt1).unwrap();
    let stmt2_id = store.intern_expr(&stmt2).unwrap();

    let j1 = store
        .insert_judgment(&NewJudgment {
            kind: JudgmentKind::Theorem,
            name: Some("assoc".into()),
            context: ctx1,
            statement: stmt1_id,
            definition_body_raw: None,
            source: SourceRef {
                file: "group.lean".into(),
                line: 1,
            },
            raw_text: "theorem assoc (a b c : G) [Group G] : a * b * c = a * (b * c)".into(),
            parse_status: ParseStatus::Full,
            source_paper: None,
        })
        .unwrap();

    let j2 = store
        .insert_judgment(&NewJudgment {
            kind: JudgmentKind::Theorem,
            name: Some("assoc".into()),
            context: ctx2,
            statement: stmt2_id,
            definition_body_raw: None,
            source: SourceRef {
                file: "peano.lean".into(),
                line: 1,
            },
            raw_text: "theorem assoc (a b c : ℕ) : a + b + c = a + (b + c)".into(),
            parse_status: ParseStatus::Full,
            source_paper: None,
        })
        .unwrap();

    assert_ne!(j1, j2);
    let rec1 = store.get_judgment(j1).unwrap();
    let rec2 = store.get_judgment(j2).unwrap();
    assert_eq!(rec1.name.as_deref(), Some("assoc"));
    assert_eq!(rec2.name.as_deref(), Some("assoc"));
    // 名前は同じでも文脈（Γ）が異なるので statement のハッシュも異なる
    assert_ne!(rec1.statement_hash, rec2.statement_hash);
    assert_eq!(rec1.context.len(), 4); // a,b,c,inst(Group G)
    assert_eq!(rec2.context.len(), 3); // a,b,c

    let all = store.list_judgments().unwrap();
    assert_eq!(all.len(), 2);
}

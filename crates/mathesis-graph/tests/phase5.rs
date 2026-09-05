use mathesis_ast::parse_expr;
use mathesis_graph::{
    is_reachable, shortest_derivation, GraphStore, JudgmentId, JudgmentKind, MorphismKind,
    NewJudgment, ParseStatus, SourceRef,
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
                file: "phase5.lean".into(),
                line: 1,
            },
            raw_text: format!("theorem {name} : {stmt}"),
            parse_status: ParseStatus::Full,
            source_paper: None,
        })
        .unwrap()
}

#[test]
fn specialization_then_implication_composes_to_implication() {
    // アーキテクチャ文書のクエリ例そのもの:
    // 「群論の定義から、アーベル群の性質Pを導く最短の依存パスは？」
    let store = GraphStore::open_in_memory().unwrap();
    let group = insert_named(&store, "group", "True");
    let abelian_group = insert_named(&store, "abelian_group", "True");
    let property_p = insert_named(&store, "commutator_vanishes", "True");

    store
        .annotate(
            group,
            abelian_group,
            MorphismKind::Specialization,
            Some("群 ⇒ アーベル群".into()),
        )
        .unwrap();
    store
        .annotate(
            abelian_group,
            property_p,
            MorphismKind::Implication,
            Some("アーベル群 → 交換子が消える".into()),
        )
        .unwrap();

    let path = shortest_derivation(&store, group, property_p)
        .unwrap()
        .expect("group から property_p への導出パスが見つかるはず");
    assert_eq!(path.composed_kind, MorphismKind::Implication);
    assert_eq!(path.hops.len(), 2);
    assert_eq!(path.hops[0].kind, MorphismKind::Specialization);
    assert_eq!(path.hops[1].kind, MorphismKind::Implication);
    assert_eq!(path.hops.last().unwrap().dst, property_p);
}

#[test]
fn implication_chain_is_transitive() {
    let store = GraphStore::open_in_memory().unwrap();
    let a = insert_named(&store, "a", "P");
    let b = insert_named(&store, "b", "Q");
    let c = insert_named(&store, "c", "R");
    let d = insert_named(&store, "d", "S");

    store.annotate(a, b, MorphismKind::Implication, None).unwrap();
    store.annotate(b, c, MorphismKind::Implication, None).unwrap();
    store.annotate(c, d, MorphismKind::Implication, None).unwrap();

    let path = shortest_derivation(&store, a, d).unwrap().unwrap();
    assert_eq!(path.composed_kind, MorphismKind::Implication);
    assert_eq!(path.hops.len(), 3);
}

#[test]
fn reachable_but_not_composable_path_returns_none_for_derivation() {
    let store = GraphStore::open_in_memory().unwrap();
    let a = insert_named(&store, "a", "P");
    let b = insert_named(&store, "b", "Q");
    let c = insert_named(&store, "c", "R");

    // a → b (Implication), b から c への一般化: この組み合わせは合成できない
    // （一般化は「より弱い命題へ戻る」向きなので、含意の後にそのまま繋げられない）。
    store.annotate(a, b, MorphismKind::Implication, None).unwrap();
    store.annotate(b, c, MorphismKind::Generalization, None).unwrap();

    // 素朴な到達可能性としては a → c に経路がある
    assert!(is_reachable(&store, a, c).unwrap());
    // しかし単一の導出関係には還元できない
    assert!(shortest_derivation(&store, a, c).unwrap().is_none());
}

#[test]
fn equivalence_edges_are_collapsed_before_pathfinding() {
    let store = GraphStore::open_in_memory().unwrap();
    let a = insert_named(&store, "a", "P");
    let b = insert_named(&store, "b", "P"); // 同じ命題、別ノード
    let c = insert_named(&store, "c", "Q");

    store
        .annotate(a, b, MorphismKind::Equivalence, Some("same P".into()))
        .unwrap();
    store.annotate(b, c, MorphismKind::Implication, None).unwrap();

    // a は b と同値なので、b 発の含意エッジを経由して c に届く
    let path = shortest_derivation(&store, a, c)
        .unwrap()
        .expect("同値経由での導出が見つかるはず");
    assert_eq!(path.composed_kind, MorphismKind::Implication);
}

#[test]
fn unrelated_nodes_have_no_path() {
    let store = GraphStore::open_in_memory().unwrap();
    let a = insert_named(&store, "a", "P");
    let b = insert_named(&store, "b", "Q");

    assert!(!is_reachable(&store, a, b).unwrap());
    assert!(shortest_derivation(&store, a, b).unwrap().is_none());
}

#[test]
fn self_query_is_trivially_reachable() {
    let store = GraphStore::open_in_memory().unwrap();
    let a = insert_named(&store, "a", "P");
    assert!(is_reachable(&store, a, a).unwrap());
}

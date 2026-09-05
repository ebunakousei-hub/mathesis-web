use criterion::{black_box, criterion_group, criterion_main, Criterion};
use mathesis_ast::parse_expr;
use mathesis_graph::{
    analyze_dependencies, heuristics, GraphStore, JudgmentId, JudgmentKind, JudgmentLite,
    MorphismKind, NewMorphism, ProofTerm,
};

fn benchmark_alpha_hashing(c: &mut Criterion) {
    c.bench_function("canonical_hash_simple", |b| {
        b.iter(|| {
            let expr = black_box(parse_expr("a + b").unwrap().expr);
            expr.canonical_hash()
        })
    });

    c.bench_function("canonical_hash_complex_binding", |b| {
        b.iter(|| {
            let expr = black_box(
                parse_expr("∀ x y (z : T) (w : U → V), f x y z w").unwrap().expr,
            );
            expr.canonical_hash()
        })
    });

    c.bench_function("canonical_hash_deep_nesting", |b| {
        b.iter(|| {
            let expr = black_box(
                parse_expr("((((a + b) * c) - d) / e) ^ f").unwrap().expr,
            );
            expr.canonical_hash()
        })
    });
}

fn benchmark_expr_interning(c: &mut Criterion) {
    let store = GraphStore::open_in_memory().unwrap();

    c.bench_function("intern_expr_new", |b| {
        b.iter(|| {
            let expr = black_box(parse_expr("a + b = c * d").unwrap().expr);
            let _ = store.intern_expr(&expr);
        })
    });

    c.bench_function("intern_expr_duplicate", |b| {
        let expr = parse_expr("a + b = c * d").unwrap().expr;
        store.intern_expr(&expr).unwrap();

        b.iter(|| {
            let expr = black_box(parse_expr("a + b = c * d").unwrap().expr);
            let _ = store.intern_expr(&expr);
        })
    });

    c.bench_function("intern_expr_alpha_equivalent", |b| {
        let expr1 = parse_expr("∀ x, x + 0 = x").unwrap().expr;
        store.intern_expr(&expr1).unwrap();

        b.iter(|| {
            let expr = black_box(parse_expr("∀ y, y + 0 = y").unwrap().expr);
            let _ = store.intern_expr(&expr);
        })
    });
}

fn benchmark_heuristics(c: &mut Criterion) {
    let mut group = c.benchmark_group("heuristics_propose");

    for size in [50, 200, 500] {
        let lites: Vec<JudgmentLite> = (0..size)
            .map(|i| JudgmentLite {
                id: JudgmentId(i as i64),
                kind: if i % 2 == 0 {
                    JudgmentKind::Theorem
                } else {
                    JudgmentKind::Definition
                },
                name: Some(match i % 5 {
                    0 => format!("abelian_group_{i}"),
                    1 => format!("group_{i}"),
                    2 => format!("foo_{i}_iff_bar_{i}"),
                    3 => format!("bar_{i}_of_foo_{i}"),
                    _ => format!("theorem_{i}"),
                }),
                statement_hash: format!("stmt_hash_{}", i % (size / 10 + 1)),
                context_hashes: vec![
                    format!("ctx_hash_{}", i % 3),
                    format!("ctx_hash_{}", (i + 1) % 3),
                ],
            })
            .collect();

        group.bench_with_input(format!("propose_N_{size}"), &lites, |b, lites| {
            b.iter(|| heuristics::propose(black_box(lites)))
        });
    }
    group.finish();
}

fn benchmark_quotient(c: &mut Criterion) {
    let mut group = c.benchmark_group("quotient_graph");

    let num_judgments = 500;

    // 100 accepted equivalence morphisms, 200 implication morphisms
    let store = GraphStore::open_in_memory().unwrap();
    // Add fake judgments
    let expr = parse_expr("P").unwrap().expr;
    let expr_id = store.intern_expr(&expr).unwrap();
    for i in 1..=num_judgments {
        let _ = store.insert_judgment(&mathesis_graph::NewJudgment {
            kind: JudgmentKind::Theorem,
            name: Some(format!("j{i}")),
            context: vec![],
            statement: expr_id,
            definition_body_raw: None,
            source: mathesis_graph::SourceRef {
                file: "bench.lean".to_string(),
                line: i as u32,
            },
            raw_text: format!("theorem j{i}"),
            parse_status: mathesis_graph::ParseStatus::Full,
            source_paper: None,
        });
    }

    for i in 1..100 {
        let _ = store.insert_morphism(&NewMorphism::manual_accepted(
            JudgmentId(i),
            JudgmentId(i + 1),
            MorphismKind::Equivalence,
            None,
        ));
    }
    for i in 1..200 {
        let _ = store.insert_morphism(&NewMorphism::manual_accepted(
            JudgmentId(i),
            JudgmentId(i + 200),
            MorphismKind::Implication,
            None,
        ));
    }

    // 永続 Union-Find キャッシュを一度温めておく（以降の呼び出しは差分のみ）。
    store.quotient_graph().unwrap();

    group.bench_function("sync_quotient_incremental_no_new_edges", |b| {
        // 新規の受理済み同値エッジがない定常状態での呼び出し。フェーズ3.2
        // 「Union-Find永続化」が主に速くしたいのはこのケース——判断・射を
        // 読み直すコストは残るが、Union-Find 自体は温存されるため union 演算は発生しない。
        b.iter(|| store.quotient_graph())
    });

    group.bench_function("rebuild_quotient_full", |b| {
        // reject 相当の全再構築パス（キャッシュを空にしてから作り直す）。
        b.iter(|| store.rebuild_quotient())
    });

    group.finish();
}

fn benchmark_proof_analysis(c: &mut Criterion) {
    let proof = ProofTerm::App(
        Box::new(ProofTerm::Ref("group_hom_preserves_identity".into())),
        vec![
            ProofTerm::App(
                Box::new(ProofTerm::Ref("group_mul_one".into())),
                vec![
                    ProofTerm::Ref("abelian_group_comm".into()),
                    ProofTerm::TacticScript("by rw [add_comm]; exact lagrange_theorem H".into()),
                ],
            ),
            ProofTerm::Ref("group_inv_mul".into()),
        ],
    );

    c.bench_function("proof_term_canonical_hash", |b| {
        b.iter(|| black_box(&proof).canonical_hash_hex())
    });

    c.bench_function("proof_term_analyze_dependencies", |b| {
        b.iter(|| analyze_dependencies(black_box(&proof)))
    });
}

fn benchmark_parsing(c: &mut Criterion) {
    c.bench_function("parse_simple_expr", |b| {
        b.iter(|| parse_expr(black_box("a + b")).unwrap())
    });

    c.bench_function("parse_binding_expr", |b| {
        b.iter(|| parse_expr(black_box("∀ x y (z : T), f x y z")).unwrap())
    });

    c.bench_function("parse_complex_statement", |b| {
        b.iter(|| {
            parse_expr(black_box(
                "∀ (a b : ℕ) [Group G] (h : P a b), a * b * c = a * (b * c)",
            ))
            .unwrap()
        })
    });
}

criterion_group!(
    benches,
    benchmark_alpha_hashing,
    benchmark_expr_interning,
    benchmark_heuristics,
    benchmark_quotient,
    benchmark_proof_analysis,
    benchmark_parsing
);
criterion_main!(benches);

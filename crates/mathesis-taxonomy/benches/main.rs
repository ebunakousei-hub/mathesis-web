use criterion::{black_box, criterion_group, criterion_main, Criterion};
use mathesis_ingest::model::Paper;
use mathesis_taxonomy::{concepts, rake};

/// 実データ（arXiv抄録）の言い回しに寄せた、複数分野にまたがる合成コーパス。
/// 同じ文をそのまま繰り返すのではなく、複数のテンプレートを巡回させることで
/// `extract_candidates` のdoc_freq集計・MSC grounding・並べ替えが実際に
/// 意味のある負荷になるようにする。
const TEMPLATES: &[(&str, &str)] = &[
    (
        "Moduli spaces of vector bundles on Kähler manifolds",
        "We study the moduli space of stable vector bundles on a compact Kähler manifold. \
         Using quantum field theory techniques we construct a natural compactification and \
         relate it to the Hilbert space of holomorphic sections. Our main theorem gives \
         sufficient conditions for the moduli space to be smooth of the expected dimension.",
    ),
    (
        "Representation theory of Lie algebras and quantum groups",
        "This paper develops the representation theory of a class of Lie algebras arising \
         from quantum groups. We classify the irreducible representations and compute the \
         associated character formulas. The results extend earlier work on Hopf algebras \
         to the setting of algebraically closed fields of positive characteristic.",
    ),
    (
        "Existence and uniqueness for a nonlinear Schrodinger equation",
        "We prove existence and uniqueness of solutions to a nonlinear Schr\\\"odinger \
         equation on a bounded domain with boundary conditions of Dirichlet type. The proof \
         relies on a fixed point argument in a suitable Hilbert space together with a priori \
         estimates for the conservation laws satisfied by the equation.",
    ),
    (
        "Category theory and homological algebra of abelian categories",
        "We investigate derived categories of abelian categories from the point of view of \
         category theory. A spectral sequence relating Ext groups to the cohomology of the \
         associated chain complex is constructed, generalizing classical homological algebra \
         to this categorical setting.",
    ),
];

fn synthetic_papers(n: usize) -> Vec<Paper> {
    (0..n)
        .map(|i| {
            let (title, abstract_text) = TEMPLATES[i % TEMPLATES.len()];
            Paper {
                arxiv_id: format!("bench.{i:05}"),
                title: title.to_string(),
                abstract_text: abstract_text.to_string(),
                authors: vec!["A. Bench".to_string()],
                categories: vec!["math.XX".to_string()],
                msc_codes: vec![],
                submitted: String::new(),
            }
        })
        .collect()
}

fn benchmark_rake(c: &mut Criterion) {
    let (title, abstract_text) = TEMPLATES[0];
    let text = format!("{title} {abstract_text}");

    c.bench_function("rake_score_single_abstract", |b| {
        b.iter(|| rake::score_document(black_box(&text)))
    });
}

fn benchmark_extract_candidates(c: &mut Criterion) {
    let mut group = c.benchmark_group("extract_candidates");

    // Phase 2の最適化（MSC groundingのO(全MSC件数)線形走査 →
    // LazyLockでの事前構築+O(1)引きへの置き換え）が退行していないかを
    // ここで継続的に検出する。実データ(5,000論文)では10.6秒→0.6秒。
    for size in [100, 500, 2000] {
        let papers = synthetic_papers(size);
        group.bench_with_input(format!("N_{size}"), &papers, |b, papers| {
            b.iter(|| concepts::extract_candidates(black_box(papers), 2))
        });
    }
    group.finish();
}

criterion_group!(benches, benchmark_rake, benchmark_extract_candidates);
criterion_main!(benches);

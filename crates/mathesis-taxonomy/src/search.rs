//! Phase 7: hybrid search（アーキテクチャ.txt 5.7 — 「キーワード一致・同一
//! concept・特殊化・関連concept の4段階」）。
//!
//! Entity resolution（"Kähler manifold" と "kahler manifold" を同一IDへ
//! 寄せる、5.4 Step 3）はまだ実装していない——Phase 2のRAKE抽出は表記ゆれを
//! そのままフレーズとして扱っており、正規化は小文字化・LaTeX記法除去止まり。
//! そのため「same concept」は文字通りの同義語統合ではなく、Phase 4の
//! クラスタリングが実際に同じクラスタへ落とした概念群として近似する
//! （embedding+共起+MSCで重み付けしたグラフをLPAが一つにまとめた候補は、
//! 実務上「ほぼ同じ話題」とみなせる）。

use crate::ann::candidate_pairs_above_threshold;
use crate::concepts::ConceptCandidate;
use crate::embed::cosine_similarity;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub phrase: String,
    pub doc_freq: usize,
    pub msc_code: Option<String>,
    pub score: f32,
}

#[derive(Debug, Clone, Default)]
pub struct HybridSearchResult {
    pub exact: Vec<SearchHit>,
    pub same_concept: Vec<SearchHit>,
    pub specialization: Vec<SearchHit>,
    pub related: Vec<SearchHit>,
}

fn to_hit(c: &ConceptCandidate, score: f32) -> SearchHit {
    SearchHit { phrase: c.phrase.clone(), doc_freq: c.doc_freq, msc_code: c.msc_code.clone(), score }
}

/// クエリの単語列が候補フレーズの単語列に連続部分列として含まれ、かつ
/// 候補の方が単語数で厳密に多い場合を「specialization」とみなす
/// （"manifold" に対して "kähler manifold" は特殊化、"manifold" 自身や
/// "the manifold of solutions" のように語順が飛ぶものは対象外）。
fn is_specialization(query_words: &[&str], candidate_words: &[&str]) -> bool {
    if query_words.is_empty() || candidate_words.len() <= query_words.len() {
        return false;
    }
    candidate_words.windows(query_words.len()).any(|w| w == query_words)
}

/// exact / same concept / specialization / related の4段階でヒットを返す。
///
/// - `cluster_of`: フレーズ → クラスタID（Phase 4 `cluster` 実行後の
///   `TaxonomyStore::load_clusters()` をそのまま渡せる想定。未実行なら
///   空でよく、その場合 same_concept は常に空になる）。
/// - `query_vector`: クエリ文自体をembedding化した結果（呼び出し側が
///   Ollamaで生成——ここでは通信しない）。`None` の場合、exact一致した
///   候補自身のembeddingを`embeddings`から探してフォールバックに使う
///   （既知の概念名で検索する分にはOllama呼び出しなしで related まで動く）。
/// - `embeddings`: `TaxonomyStore::load_embeddings()` の全件。未実行なら
///   空でよく、その場合 related は常に空になる。
pub fn hybrid_search(
    query: &str,
    candidates: &[ConceptCandidate],
    cluster_of: &HashMap<String, usize>,
    query_vector: Option<&[f32]>,
    embeddings: &[(String, Vec<f32>)],
    top_k: usize,
) -> HybridSearchResult {
    let query_lower = query.trim().to_lowercase();
    let mut shown: HashSet<String> = HashSet::new();

    let exact: Vec<&ConceptCandidate> = candidates.iter().filter(|c| c.phrase == query_lower).collect();
    for c in &exact {
        shown.insert(c.phrase.clone());
    }

    let exact_clusters: HashSet<usize> = exact.iter().filter_map(|c| cluster_of.get(&c.phrase).copied()).collect();
    let mut same_concept: Vec<&ConceptCandidate> = if exact_clusters.is_empty() {
        Vec::new()
    } else {
        candidates
            .iter()
            .filter(|c| !shown.contains(&c.phrase))
            .filter(|c| cluster_of.get(&c.phrase).is_some_and(|cid| exact_clusters.contains(cid)))
            .collect()
    };
    same_concept.sort_by_key(|c| std::cmp::Reverse(c.doc_freq));
    same_concept.truncate(top_k);
    for c in &same_concept {
        shown.insert(c.phrase.clone());
    }

    let query_words: Vec<&str> = query_lower.split_whitespace().collect();
    let mut specialization: Vec<&ConceptCandidate> = candidates
        .iter()
        .filter(|c| !shown.contains(&c.phrase))
        .filter(|c| is_specialization(&query_words, &c.phrase.split_whitespace().collect::<Vec<_>>()))
        .collect();
    specialization.sort_by_key(|c| std::cmp::Reverse(c.doc_freq));
    specialization.truncate(top_k);
    for c in &specialization {
        shown.insert(c.phrase.clone());
    }

    let effective_query_vector: Option<Vec<f32>> = query_vector.map(<[f32]>::to_vec).or_else(|| {
        exact.first().and_then(|c| embeddings.iter().find(|(p, _)| *p == c.phrase).map(|(_, v)| v.clone()))
    });

    let related: Vec<SearchHit> = match effective_query_vector {
        None => Vec::new(),
        Some(qv) => {
            let candidate_by_phrase: HashMap<&str, &ConceptCandidate> =
                candidates.iter().map(|c| (c.phrase.as_str(), c)).collect();
            let mut scored: Vec<(&ConceptCandidate, f32)> = embeddings
                .iter()
                .filter(|(p, _)| !shown.contains(p))
                .filter_map(|(p, v)| candidate_by_phrase.get(p.as_str()).map(|&c| (c, cosine_similarity(&qv, v))))
                .collect();
            scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            scored.truncate(top_k);
            scored.into_iter().map(|(c, s)| to_hit(c, s)).collect()
        }
    };

    HybridSearchResult {
        exact: exact.iter().map(|c| to_hit(c, 1.0)).collect(),
        same_concept: same_concept.iter().map(|c| to_hit(c, 1.0)).collect(),
        specialization: specialization
            .iter()
            .map(|c| {
                let candidate_word_count = c.phrase.split_whitespace().count().max(1);
                to_hit(c, query_words.len() as f32 / candidate_word_count as f32)
            })
            .collect(),
        related,
    }
}

/// 全embeddingについて、ノードごとのembedding近傍上位k件を求める
/// （Web版Explorerの静的JSONに「related」段階を載せるための事前計算——
/// ブラウザ側にembeddingベクトル本体を持たせるとサイズが跳ねるため、
/// 辺のリストだけをエクスポートする）。
///
/// 候補ペアの発見自体は`graph.rs::build_concept_graph`と同じく
/// `ann::candidate_pairs_above_threshold`に委譲する（n=数千までは総当たり、
/// それ以上はLSH——`ann.rs`参照）。目的は違う（あちらは共起・MSCも混ぜた
/// クラスタリング用グラフ、こちらはembedding単独のsearch用近傍）が、
/// 「上位k近傍を求める」という核の計算は同じため、その部分だけ共有する。
pub fn top_k_by_embedding(embeddings: &[(String, Vec<f32>)], k: usize, min_sim: f32) -> HashMap<String, Vec<(String, f32)>> {
    top_k_by_embedding_excluding(embeddings, k, min_sim, |_, _| false)
}

/// `top_k_by_embedding` に「この2件は同じものとみなすので近傍に出さない」
/// 判定を足したもの。
///
/// 実データで「関連概念」段階を全走査したところ、リストの**86.5%**
/// （64,520/74,566）が語幹を共有する候補——つまり同じ概念の表記ゆれ——で
/// 6割以上占められていた（"zeta function" の関連が zeta-function /
/// zeta functions / zeta-functions で埋まる）。表記ゆれは
/// `resolve.rs` が畳んで「同一概念」段階が見せる担当なので、
/// 「関連概念」段階に重ねて出す意味が無く、本来そこに出るべき別概念を
/// 押し出してしまう。
pub fn top_k_by_embedding_excluding(
    embeddings: &[(String, Vec<f32>)],
    k: usize,
    min_sim: f32,
    same_concept: impl Fn(usize, usize) -> bool,
) -> HashMap<String, Vec<(String, f32)>> {
    let n = embeddings.len();

    let mut per_node: Vec<Vec<(usize, f32)>> = vec![Vec::new(); n];
    for (i, j, sim) in candidate_pairs_above_threshold(n, |i| embeddings[i].1.as_slice(), min_sim) {
        if same_concept(i, j) {
            continue;
        }
        per_node[i].push((j, sim));
        per_node[j].push((i, sim));
    }

    per_node
        .into_iter()
        .enumerate()
        .map(|(i, mut adj)| {
            adj.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            adj.truncate(k);
            (embeddings[i].0.clone(), adj.into_iter().map(|(j, s)| (embeddings[j].0.clone(), s)).collect())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(phrase: &str, doc_freq: usize, msc_code: Option<&str>) -> ConceptCandidate {
        ConceptCandidate {
            phrase: phrase.to_string(),
            word_count: phrase.split_whitespace().count(),
            doc_freq,
            mean_score: 0.0,
            msc_code: msc_code.map(str::to_string),
            sample_arxiv_ids: vec![],
            field_concentration: None,
        }
    }

    #[test]
    fn is_specialization_requires_contiguous_containment_and_strictly_more_words() {
        assert!(is_specialization(&["manifold"], &["kähler", "manifold"]));
        assert!(is_specialization(&["kähler", "manifold"], &["compact", "kähler", "manifold"]));
        assert!(!is_specialization(&["manifold"], &["manifold"]), "同じ語数は特殊化ではない");
        assert!(!is_specialization(&["manifold"], &["smooth", "structure"]), "語自体が含まれない");
        assert!(
            !is_specialization(&["kähler", "manifold"], &["kähler", "compact", "manifold"]),
            "連続部分列でなければ特殊化とみなさない"
        );
    }

    #[test]
    fn exact_tier_matches_case_and_whitespace_insensitively() {
        let candidates = vec![candidate("kähler manifold", 10, Some("32Q15"))];
        let result = hybrid_search("  Kähler Manifold  ", &candidates, &HashMap::new(), None, &[], 10);
        assert_eq!(result.exact.len(), 1);
        assert_eq!(result.exact[0].phrase, "kähler manifold");
    }

    #[test]
    fn same_concept_tier_pulls_in_cluster_mates_of_the_exact_match_only() {
        let candidates = vec![
            candidate("kähler manifold", 10, Some("32Q15")),
            candidate("compact complex manifold", 6, Some("32Q99")),
            candidate("unrelated topic", 20, None),
        ];
        let mut cluster_of = HashMap::new();
        cluster_of.insert("kähler manifold".to_string(), 0);
        cluster_of.insert("compact complex manifold".to_string(), 0);
        cluster_of.insert("unrelated topic".to_string(), 1);

        let result = hybrid_search("kähler manifold", &candidates, &cluster_of, None, &[], 10);
        assert_eq!(result.same_concept.len(), 1);
        assert_eq!(result.same_concept[0].phrase, "compact complex manifold");
    }

    #[test]
    fn same_concept_tier_is_empty_without_an_exact_match() {
        let candidates = vec![candidate("compact complex manifold", 6, None)];
        let mut cluster_of = HashMap::new();
        cluster_of.insert("compact complex manifold".to_string(), 0);

        let result = hybrid_search("nonexistent phrase", &candidates, &cluster_of, None, &[], 10);
        assert!(result.same_concept.is_empty());
    }

    #[test]
    fn specialization_tier_finds_longer_phrases_containing_the_query_as_a_subsequence() {
        let candidates = vec![
            candidate("manifold", 50, None),
            candidate("kähler manifold", 10, Some("32Q15")),
            candidate("riemannian manifold", 8, None),
            candidate("the manifold of solutions", 3, None), // 単語1つの検索クエリなので、これも「含む」に該当する
        ];
        let result = hybrid_search("manifold", &candidates, &HashMap::new(), None, &[], 10);
        let phrases: HashSet<&str> = result.specialization.iter().map(|h| h.phrase.as_str()).collect();
        assert_eq!(phrases, HashSet::from(["kähler manifold", "riemannian manifold", "the manifold of solutions"]));
        // doc_freq降順
        assert_eq!(result.specialization[0].phrase, "kähler manifold");
    }

    #[test]
    fn specialization_tier_requires_contiguous_word_order_for_multi_word_queries() {
        let candidates = vec![
            candidate("compact kähler manifold", 10, None), // "kähler manifold"を連続して含む
            candidate("kähler manifold with boundary", 4, None), // 先頭に連続して含む
            candidate("kähler and manifold theory", 2, None), // 連続していない
        ];
        let result = hybrid_search("kähler manifold", &candidates, &HashMap::new(), None, &[], 10);
        let phrases: HashSet<&str> = result.specialization.iter().map(|h| h.phrase.as_str()).collect();
        assert_eq!(phrases, HashSet::from(["compact kähler manifold", "kähler manifold with boundary"]));
    }

    #[test]
    fn specialization_tier_excludes_phrases_already_shown_as_exact_or_same_concept() {
        let candidates = vec![candidate("kähler manifold", 10, None), candidate("compact kähler manifold", 4, None)];
        let mut cluster_of = HashMap::new();
        cluster_of.insert("kähler manifold".to_string(), 0);
        cluster_of.insert("compact kähler manifold".to_string(), 0);

        let result = hybrid_search("kähler manifold", &candidates, &cluster_of, None, &[], 10);
        // "compact kähler manifold" は同じクラスタなのでsame_concept止まりで、
        // specializationには重複して出てこない。
        assert_eq!(result.same_concept.iter().map(|h| h.phrase.as_str()).collect::<Vec<_>>(), vec!["compact kähler manifold"]);
        assert!(result.specialization.is_empty());
    }

    #[test]
    fn related_tier_uses_explicit_query_vector_when_given() {
        let candidates = vec![candidate("kähler manifold", 10, None), candidate("ricci flow", 5, None)];
        let embeddings =
            vec![("kähler manifold".to_string(), vec![1.0, 0.0]), ("ricci flow".to_string(), vec![0.99, 0.01])];
        let result = hybrid_search("some free-text query", &candidates, &HashMap::new(), Some(&[1.0, 0.0]), &embeddings, 10);
        assert_eq!(result.related.len(), 2);
        assert_eq!(result.related[0].phrase, "kähler manifold");
        assert!((result.related[0].score - 1.0).abs() < 1e-6);
    }

    #[test]
    fn related_tier_falls_back_to_the_exact_matchs_own_embedding_when_no_query_vector_given() {
        let candidates = vec![candidate("kähler manifold", 10, None), candidate("ricci flow", 5, None), candidate("group theory", 5, None)];
        let embeddings = vec![
            ("kähler manifold".to_string(), vec![1.0, 0.0]),
            ("ricci flow".to_string(), vec![0.99, 0.01]),
            ("group theory".to_string(), vec![0.0, 1.0]),
        ];
        let result = hybrid_search("kähler manifold", &candidates, &HashMap::new(), None, &embeddings, 10);
        // 自分自身(exact)はrelatedには出ない。
        assert_eq!(result.related.len(), 2);
        assert_eq!(result.related[0].phrase, "ricci flow");
    }

    #[test]
    fn related_tier_is_empty_when_no_query_vector_and_no_exact_match() {
        let candidates = vec![candidate("kähler manifold", 10, None)];
        let embeddings = vec![("kähler manifold".to_string(), vec![1.0, 0.0])];
        let result = hybrid_search("totally unknown phrase", &candidates, &HashMap::new(), None, &embeddings, 10);
        assert!(result.related.is_empty());
    }

    #[test]
    fn top_k_by_embedding_returns_closest_neighbors_above_threshold_sorted_descending() {
        let embeddings = vec![
            ("a".to_string(), vec![1.0, 0.0]),
            ("b".to_string(), vec![0.99, 0.01]),
            ("c".to_string(), vec![0.0, 1.0]),
        ];
        let neighbors = top_k_by_embedding(&embeddings, 5, 0.5);
        assert_eq!(neighbors["a"], vec![("b".to_string(), neighbors["a"][0].1)]);
        assert!(neighbors["a"][0].1 > 0.99);
        assert!(neighbors["c"].is_empty(), "類似度0.5未満のペアは辺にならない");
    }

    #[test]
    fn top_k_by_embedding_truncates_to_k() {
        let embeddings: Vec<(String, Vec<f32>)> =
            (0..5).map(|i| (format!("p{i}"), vec![1.0, 0.01 * i as f32])).collect();
        let neighbors = top_k_by_embedding(&embeddings, 2, 0.0);
        assert_eq!(neighbors["p0"].len(), 2);
    }
}

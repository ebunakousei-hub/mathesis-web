//! 概念候補どうしを繋ぐ重み付きグラフの構築（アーキテクチャ.txt 5.4/5.7
//! の w_ij = λ1·sim_emb + λ2·cooccur + λ3·citation + λ4·msc）。
//!
//! citation項は実装しない——arXivのOAI-PMHメタデータには引用関係が
//! 含まれておらず（Phase 1で確認済み）、引用グラフを持つにはSemantic
//! Scholar等の別APIからの追加収集が要る。今は
//!   w_ij = w_embed·sim_emb + w_cooccur·cooccur_norm + w_msc·msc_relatedness
//! の3項だけで構成し、citation項は将来λ3として差し込めるようにコメントで
//! 明記しておく。cooccur_normはJaccard係数（`jaccard_cooccur`参照——
//! 当初のmin正規化は文書頻度が小さい語のペアで異常に跳ね上がる実バグが
//! あり、100kスケールの実データで発見・修正した）。

use crate::ann::candidate_pairs_above_threshold;
use crate::embed::cosine_similarity;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug)]
pub struct GraphParams {
    /// embedding類似度によるk近傍グラフの k
    pub embed_top_k: usize,
    /// 近傍とみなす最低コサイン類似度
    pub embed_min_sim: f32,
    /// この件数以上の論文で共起していれば辺候補にする
    pub min_cooccur: u32,
    /// k近傍を**相互**にするか（j が i の上位k件で、かつ i が j の上位k件
    /// のときだけ辺を張る）。
    ///
    /// 和集合のk近傍は、高次元embedding空間の「ハブ性」——一部のベクトルが
    /// 異常に多くの点の近傍リストに現れる現象——をそのままグラフに持ち込む。
    /// 実データ（概念61,286件）で和集合のまま作ったグラフは辺304,061本の
    /// 密な塊になり、Louvainが解像度γ=128まで上げないとLPAのクラスタ品質
    /// （MSC-NMI 0.5677）に届かなかった。解像度パラメータでグラフ構造と
    /// 戦っている状態で、健全ではない。相互k近傍はハブ由来の片思いの辺を
    /// 落とすので、コミュニティが本来の形で分離する。
    pub mutual_knn: bool,
    pub w_embed: f32,
    pub w_cooccur: f32,
    pub w_msc: f32,
}

impl Default for GraphParams {
    fn default() -> Self {
        Self {
            embed_top_k: 8,
            embed_min_sim: 0.35,
            min_cooccur: 3,
            mutual_knn: true,
            w_embed: 1.0,
            w_cooccur: 0.5,
            w_msc: 0.3,
        }
    }
}

/// `embeddings[i]` はノードiのembedding、`doc_freqs[i]`・`msc_codes[i]` は
/// 対応するConcept候補のメタデータ（`concepts.rs`のConceptCandidateと同じ
/// 意味）。`paper_concept_nodes` は (arxiv_id, ノード添字) のペア
/// （共起シグナル用、あらかじめフレーズをノード添字に変換した状態で渡す）。
pub fn build_concept_graph(
    embeddings: &[Vec<f32>],
    doc_freqs: &[usize],
    msc_codes: &[Option<String>],
    paper_concept_nodes: &[(String, usize)],
    params: &GraphParams,
) -> crate::lpa::Graph {
    let n = embeddings.len();
    debug_assert_eq!(doc_freqs.len(), n);
    debug_assert_eq!(msc_codes.len(), n);

    // 1. embeddingのk近傍（各ノードごとに上位k件、min_sim以上のみ）。
    //    候補ペアの発見自体は`ann::candidate_pairs_above_threshold`に委譲——
    //    n=数千まではそのまま総当たり、それ以上はLSHで候補を絞ってから
    //    厳密採点する（`ann.rs`のモジュールコメント参照。10万〜100万論文
    //    規模でn²が非現実的になることが実データで判明したための変更）。
    let mut per_node_topk: Vec<Vec<(usize, f32)>> = vec![Vec::new(); n];
    for (i, j, sim) in candidate_pairs_above_threshold(n, |i| embeddings[i].as_slice(), params.embed_min_sim) {
        per_node_topk[i].push((j, sim));
        per_node_topk[j].push((i, sim));
    }
    for adj in &mut per_node_topk {
        adj.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        adj.truncate(params.embed_top_k);
    }

    // 相互k近傍にするなら、両側の上位k件に入っているペアだけを残す。
    let in_topk: Vec<HashSet<usize>> = if params.mutual_knn {
        per_node_topk.iter().map(|adj| adj.iter().map(|&(j, _)| j).collect()).collect()
    } else {
        Vec::new()
    };
    let mut pair_set: HashSet<(usize, usize)> = HashSet::new();
    for (i, adj) in per_node_topk.iter().enumerate() {
        for &(j, _) in adj {
            if params.mutual_knn && !in_topk[j].contains(&i) {
                continue;
            }
            pair_set.insert(if i < j { (i, j) } else { (j, i) });
        }
    }

    // 2. 共起カウント（同じ論文に現れたノードどうし）。
    let mut by_paper: HashMap<&str, Vec<usize>> = HashMap::new();
    for (arxiv_id, node) in paper_concept_nodes {
        by_paper.entry(arxiv_id.as_str()).or_default().push(*node);
    }
    let mut cooccur: HashMap<(usize, usize), u32> = HashMap::new();
    for nodes in by_paper.values() {
        for a in 0..nodes.len() {
            for b in (a + 1)..nodes.len() {
                let key = if nodes[a] < nodes[b] { (nodes[a], nodes[b]) } else { (nodes[b], nodes[a]) };
                *cooccur.entry(key).or_default() += 1;
            }
        }
    }
    for (&pair, &count) in &cooccur {
        if count >= params.min_cooccur {
            pair_set.insert(pair);
        }
    }

    // 3. 候補ペアそれぞれについて最終的な重みを計算する。
    let mut adjacency: Vec<Vec<(usize, f32)>> = vec![Vec::new(); n];
    for (i, j) in pair_set {
        let embed_sim = cosine_similarity(&embeddings[i], &embeddings[j]);
        let cooccur_norm = match cooccur.get(&(i, j)) {
            Some(&count) if count > 0 => jaccard_cooccur(count, doc_freqs[i], doc_freqs[j]),
            _ => 0.0,
        };
        let msc_bonus = msc_relatedness(&msc_codes[i], &msc_codes[j]);

        let weight = params.w_embed * embed_sim + params.w_cooccur * cooccur_norm + params.w_msc * msc_bonus;
        if weight > 0.0 {
            adjacency[i].push((j, weight));
            adjacency[j].push((i, weight));
        }
    }

    crate::lpa::Graph { adjacency }
}

/// 共起カウントの正規化。`|A∩B| / min(|A|,|B|)`（包含係数/overlap
/// coefficient）ではなく `|A∩B| / |A∪B|`（Jaccard係数）を使う。
///
/// 実データ（100,000論文）のクラスタ結果を目視したところ、"finite group"
/// のクラスタに "time reversal"・"charge conjugation"・"space inversion"
/// のような無関係な物理用語が混ざっていた。原因を辿ると、min-正規化の
/// 既知の弱点——**片方の文書頻度が小さいと分母が小さくなり、比率が
/// 実際の関連の強さと無関係に跳ね上がる**——にたどり着いた。例えば
/// "clifford-lipschitz groups"（df=4）と"orthogonal groups"（df=62）が
/// たった3論文で共起しただけで、min正規化では 3/min(4,62) = 0.75 という
/// 「ほぼ常に一緒に現れる」に相当する値になっていた——実態は5本前後の
/// 論文からなる1著者の狭い連作が、たまたま`min_cooccur`のしきい値ちょうど
/// で共起しただけ。コーパス全体を走査すると同型のペアが29,448件見つかり、
/// 孤立した事例ではなくグラフ構築の系統的な欠陥だった。
///
/// Jaccard係数 `count / (df_i + df_j - count)` は分母が両方の文書頻度の
/// 和（に近い値）になるため、片方が極端に小さいだけでは跳ね上がらない。
/// 上の例では 3/(4+62-3) = 0.048 まで下がる——共起した事実は残しつつ、
/// 「本当に強く結び付いている」ケースとは明確に差が付く。
fn jaccard_cooccur(count: u32, doc_freq_i: usize, doc_freq_j: usize) -> f32 {
    let union = (doc_freq_i + doc_freq_j).saturating_sub(count as usize).max(1);
    count as f32 / union as f32
}

/// 2つのMSCコードの関連度: 完全一致=1.0、同じセクション（3文字接頭辞、
/// 例"18A"）=0.6、同じトップレベル分野（2桁）=0.3、それ以外/情報なし=0.0。
fn msc_relatedness(a: &Option<String>, b: &Option<String>) -> f32 {
    let (Some(a), Some(b)) = (a, b) else {
        return 0.0;
    };
    if a == b {
        return 1.0;
    }

    let chain_a = mathesis_msc::ancestor_chain(a);
    let chain_b = mathesis_msc::ancestor_chain(b);

    if let (Some(sa), Some(sb)) = (section_of(&chain_a), section_of(&chain_b)) {
        if sa == sb {
            return 0.6;
        }
    }

    if let (Some(ta), Some(tb)) = (top_of(&chain_a), top_of(&chain_b)) {
        if ta == tb {
            return 0.3;
        }
    }

    0.0
}

fn section_of(chain: &[&'static mathesis_msc::MscCode]) -> Option<&'static str> {
    chain
        .iter()
        .find(|c| c.level == mathesis_msc::MscLevel::Section)
        .map(|c| c.code.as_str())
}

fn top_of(chain: &[&'static mathesis_msc::MscCode]) -> Option<&'static str> {
    chain.first().map(|c| c.code.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vec3(x: f32, y: f32, z: f32) -> Vec<f32> {
        vec![x, y, z]
    }

    #[test]
    fn msc_relatedness_exact_match_is_one() {
        assert_eq!(msc_relatedness(&Some("18A05".into()), &Some("18A05".into())), 1.0);
    }

    #[test]
    fn msc_relatedness_same_section_is_point_six() {
        // 18A05 と 18A25 はどちらもセクション 18Axx
        assert_eq!(msc_relatedness(&Some("18A05".into()), &Some("18A25".into())), 0.6);
    }

    #[test]
    fn msc_relatedness_same_top_level_is_point_three() {
        // 18A05（セクション18Axx）と 18B05（セクション18Bxx）はどちらも
        // トップレベル18-XX（実データで存在を確認済み——最初"18D05"で
        // 書いていたが、それはMSC1991/2000の廃止コードで実際には
        // MSC2020に存在せず、このテスト自体が誤って0.0を返すのを見逃す
        // ところだった）。
        assert_eq!(msc_relatedness(&Some("18A05".into()), &Some("18B05".into())), 0.3);
    }

    #[test]
    fn msc_relatedness_different_fields_is_zero() {
        assert_eq!(msc_relatedness(&Some("18A05".into()), &Some("11A05".into())), 0.0);
    }

    #[test]
    fn msc_relatedness_missing_code_is_zero() {
        assert_eq!(msc_relatedness(&None, &Some("18A05".into())), 0.0);
        assert_eq!(msc_relatedness(&None, &None), 0.0);
    }

    #[test]
    fn build_concept_graph_connects_similar_embeddings() {
        // node0とnode1のembeddingはほぼ同一（高類似度）、node2は直交（無関係）。
        let embeddings = vec![vec3(1.0, 0.0, 0.0), vec3(0.99, 0.01, 0.0), vec3(0.0, 0.0, 1.0)];
        let doc_freqs = vec![5, 5, 5];
        let msc_codes = vec![None, None, None];
        let params = GraphParams {
            embed_top_k: 2,
            embed_min_sim: 0.5,
            min_cooccur: 999, // 共起は無効化してembedding単独の効果を見る
            w_embed: 1.0,
            w_cooccur: 0.0,
            w_msc: 0.0,
            mutual_knn: true,
        };
        let graph = build_concept_graph(&embeddings, &doc_freqs, &msc_codes, &[], &params);

        let neighbors_of_0: Vec<usize> = graph.adjacency[0].iter().map(|&(j, _)| j).collect();
        assert!(neighbors_of_0.contains(&1), "node0 should connect to the near-identical node1");
        assert!(!neighbors_of_0.contains(&2), "node0 should not connect to the orthogonal node2");
    }

    #[test]
    fn build_concept_graph_connects_frequent_cooccurrence_even_with_dissimilar_embeddings() {
        // node0とnode1のembeddingは直交（似ていない）が、3本の論文で毎回共起する。
        let embeddings = vec![vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0)];
        let doc_freqs = vec![3, 3];
        let msc_codes = vec![None, None];
        let paper_concept_nodes = vec![
            ("p1".to_string(), 0), ("p1".to_string(), 1),
            ("p2".to_string(), 0), ("p2".to_string(), 1),
            ("p3".to_string(), 0), ("p3".to_string(), 1),
        ];
        let params = GraphParams {
            embed_top_k: 5,
            embed_min_sim: 0.9, // embeddingだけでは絶対に繋がらない閾値
            min_cooccur: 3,
            w_embed: 1.0,
            w_cooccur: 1.0,
            w_msc: 0.0,
            mutual_knn: true,
        };
        let graph = build_concept_graph(&embeddings, &doc_freqs, &msc_codes, &paper_concept_nodes, &params);
        assert_eq!(graph.adjacency[0].len(), 1, "cooccurrence alone must still create the edge");
        assert_eq!(graph.adjacency[0][0].0, 1);
    }

    /// ハブ由来の「片思い」の辺が相互k近傍で落ちること。
    /// node0 は node1/node2 の両方と中程度に似ており、k=1 だと
    /// node1・node2 の上位1件はどちらも node0（＝node0がハブ）だが、
    /// node0 の上位1件は node1 だけ。union なら 0-2 の辺も張られるが、
    /// mutual なら張られない。
    #[test]
    fn mutual_knn_drops_one_sided_hub_edges() {
        let embeddings = vec![vec3(1.0, 0.0, 0.0), vec3(0.98, 0.2, 0.0), vec3(0.9, 0.0, 0.44)];
        let doc_freqs = vec![5, 5, 5];
        let msc_codes = vec![None, None, None];
        let base = GraphParams {
            embed_top_k: 1,
            embed_min_sim: 0.5,
            min_cooccur: 999,
            w_embed: 1.0,
            w_cooccur: 0.0,
            w_msc: 0.0,
            mutual_knn: true,
        };
        let mutual = build_concept_graph(&embeddings, &doc_freqs, &msc_codes, &[], &base);
        let union = build_concept_graph(
            &embeddings,
            &doc_freqs,
            &msc_codes,
            &[],
            &GraphParams { mutual_knn: false, ..base },
        );
        let edges = |g: &crate::lpa::Graph| g.adjacency.iter().map(Vec::len).sum::<usize>() / 2;
        assert!(
            edges(&mutual) < edges(&union),
            "mutual={} union={} — 片思いの辺が落ちていない",
            edges(&mutual),
            edges(&union)
        );
    }

    #[test]
    fn jaccard_cooccur_matches_hand_computed_values() {
        // 実データ（100kスケール）で発見した実例そのもの:
        // "clifford-lipschitz groups"（df=4）と"orthogonal groups"（df=62）
        // が3論文で共起。min正規化なら3/4=0.75という「ほぼ常に一緒」の
        // 値になっていたが、Jaccardなら 3/(4+62-3)=3/63 に収まる。
        assert!((jaccard_cooccur(3, 4, 62) - 3.0 / 63.0).abs() < 1e-6);
        // 完全に一致する2つの集合（同じ文書頻度・全論文で共起）は1.0。
        assert!((jaccard_cooccur(10, 10, 10) - 1.0).abs() < 1e-6);
        // 共起が0件ならJaccardも0（呼び出し側でこの分岐には来ないが、
        // 関数単体としての境界値は確認しておく）。
        assert_eq!(jaccard_cooccur(0, 10, 10), 0.0);
    }

    /// 実データ（100,000論文）で実際に発覚した回帰の再現。"finite group"
    /// のクラスタに"time reversal"のような無関係な物理用語が紛れ込んで
    /// いた——原因は、文書頻度が極端に小さい語（df=4程度）が、たった
    /// `min_cooccur`件の偶然の共起だけでmin正規化なら極端に高い重みを
    /// 得てしまう構造的な欠陥だった。
    ///
    /// 注意: 「文書頻度が小さいペアの正規化co-occurrenceが、大きいペアより
    /// 高い値になること」自体はJaccardに直しても残る——これは欠陥ではなく
    /// 正しい統計的性質。3件の共起は、df=4の語にとっては全体の75%を占める
    /// 強い相関だが、df=500の語にとっては0.6%に過ぎない弱い相関で、
    /// この非対称性を無視して両者を同じ数値で表す方が誤り。実際に直った
    /// ことは、単体テストの数値比較ではなく実データでのクラスタ内容
    /// （このファイルのコメント、及びアーキテクチャ.txt参照）で検証済み:
    /// 修正後、"finite group"クラスタから物理用語が抜け、"clifford-lipschitz
    /// groups"は"charge conjugation"・"fundamental automorphisms"という
    /// 意味的に正しい別クラスタに移った。
    ///
    /// ここでの回帰テストは、その代わりに**関数の境界での正しさ**——
    /// min正規化が生んでいた「文書頻度の小さい側1つだけで分母が決まり、
    /// 大きい側の文書頻度が結果に一切影響しない」という性質が消えている
    /// こと——を確認する。Jaccardでは大きい側の文書頻度も分母（union）に
    /// 効くので、同じ共起件数・同じ小さい方の文書頻度でも、大きい方の
    /// 文書頻度が増えるほど正規化co-occurrenceは単調に下がる。
    #[test]
    fn jaccard_normalization_is_sensitive_to_the_larger_side_unlike_the_old_min_normalization() {
        // 旧実装 count/min(df_i,df_j) は、doc_freq_j をいくら変えても
        // min(4, doc_freq_j) が4のままである限り値が変わらなかった
        // （df_j=62でもdf_j=100000でも同じ0.75）。新実装はunionが
        // doc_freq_jと共に増えるので、値は単調に下がる。
        let small_partner = jaccard_cooccur(3, 4, 62);
        let huge_partner = jaccard_cooccur(3, 4, 100_000);
        assert!(
            huge_partner < small_partner,
            "共起相手の文書頻度が大きくなったのに正規化co-occurrenceが下がらない:              small_partner={small_partner} huge_partner={huge_partner}"
        );
        // 極端な場合（相手が非常に一般的な語）はほぼ0に潰れるべき——
        // 「4回のうち3回」が「10万件中4件しかない語との偶然」でしかないなら、
        // その語同士が強く関連しているとはもはや言えない。
        assert!(huge_partner < 0.001, "huge_partner={huge_partner} should collapse toward 0");
    }
}

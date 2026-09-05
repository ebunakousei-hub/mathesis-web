//! Louvain法（Blondel et al. 2008）によるモジュラリティ最適化。
//!
//! `lpa.rs`（Label Propagation）の置き換え。LPAを選んだ当初の判断は
//! 「実装が単純で検証しやすく、ほぼ線形時間で収束する。モジュラリティ
//! 最適化の精度が要るとわかってから乗り換える」だったが、100,000論文
//! 規模の実データでその「要るとわかる」状況になった:
//!
//!   LPAの出力（概念61,286件・辺304,061本）には1,117件の巨大クラスタが
//!   でき、その中身は "finite group" / "symmetric group" /
//!   "irreducible representations" / "finite set" / "finitely generated" /
//!   "direct sum" / "free group" と、群論・線形代数・集合論が混ざった
//!   塊だった。所属メンバーのMSCトップレベル分野の一致率はわずか
//!   50/1,117 = 4.5%。2位（852件）は14/852、3位（691件）は17/691。
//!
//! これはLPAの既知の失敗モード——最適化すべき目的関数を持たないため、
//! ラベルは「隣の多数派」に流れるだけで、弱く繋がった別コミュニティ同士が
//! 際限なく融合していく。Louvainは明示的な目的関数（モジュラリティ）を
//! 貪欲に最大化し、さらに解像度パラメータ γ で粒度を制御できる。
//!
//! モジュラリティ（解像度付き、Reichardt–Bornholdt）:
//!   Q = (1/2m) Σ_ij [ A_ij - γ k_i k_j / 2m ] δ(c_i, c_j)
//! 「実際にコミュニティ内にある辺の重み」と「次数だけ保ったランダム
//! グラフで期待される重み」の差。γ を上げると期待値の項が重くなり、
//! 融合しにくくなる＝細かいクラスタになる。
//!
//! # グラフの表現について
//!
//! `lpa.rs` の `Graph` と同じ「対称な重み付き隣接リスト」を受け取るが、
//! Louvainは第2段階でコミュニティを1ノードに畳んだ集約グラフを作るため、
//! 自己ループ（畳まれたコミュニティ内部の辺）を扱える必要がある。
//! 表現は次のとおりで統一する:
//!   - i≠j の辺は adjacency[i] と adjacency[j] の両方に1回ずつ入る
//!   - 自己ループ (i,i,w) は adjacency[i] に1回だけ入り、次数と内部重みへの
//!     寄与は 2w として数える（A_ii が Σ_ij の両方向で数えられるため）
//!
//! 入力グラフに自己ループが無くても正しく動く。

use crate::lpa::Graph;
use std::collections::HashMap;

/// モジュラリティ最適化での解像度 γ の既定値。1.0が古典的なモジュラリティ。
pub const DEFAULT_RESOLUTION: f32 = 1.0;

/// CPMでの解像度 γ の既定値。
///
/// 実データ（概念61,286件・相互k近傍グラフ）でγを振って決めた:
///   γ=0.05  クラスタ11,947  NMI 0.6253  純度85.5%  最大38
///   γ=0.10  クラスタ15,536  NMI 0.6316  純度89.5%  最大23  ← 採用
///   γ=0.20  クラスタ20,274  NMI 0.6353  純度91.9%  最大17
///   γ=0.80  クラスタ41,442  NMI 0.6448  純度97.4%  最大 9
/// NMIも純度もγを上げるほど単調に上がるが、それは分割を細かくするほど
/// 有利になる指標の性質でもあるので、数字だけでは決められない。実際に
/// 中身を読んで決めた:
///   γ=0.10 の最大クラスタは "fixed points / fixed point / fixed point set /
///          fixed point theorem / unique fixed point / fixed point sets /
///          fixed point theorems / fixed point property" ——「同一概念の
///          表記ゆれ」としてこれ以上ない出力。
///   γ=0.20 では最大クラスタが "asymptotic distribution / various classes /
///          different definitions / data point / spatial patterns" と
///          意味的に崩れ始める（指標は上がっているのに）。
///   γ=0.05 では "singular points" と "calabi-yau 3-folds" が同居し始める。
/// サイズ分布もγ=0.1が最良: 21件以上のクラスタがわずか3個（LPAは575個）で、
/// 2〜20件という「同一概念グループ」として使える大きさに11,183個が入る。
pub const DEFAULT_CPM_RESOLUTION: f32 = 0.1;

/// 最大化する目的関数。
///
/// モジュラリティには**解像度限界**（Fortunato–Barthélemy 2007）がある。
/// 期待値の項が全体の辺数 2m に依存するため、グラフが大きくなるほど
/// 「小さなコミュニティを併合した方が得」になり、本来別物のコミュニティが
/// 融合する。実データでこれが実際に起きた: 概念61,286件のグラフで
/// モジュラリティを最大化すると、γ=1 では2,499件の巨大クラスタができ、
/// MSC-NMIは0.38まで落ちた。γ=128まで上げてようやくLPA並みになる、
/// という「解像度パラメータでグラフ構造と戦う」状態だった。
///
/// CPM（Constant Potts Model, Traag et al. 2011）は
///   Q = Σ_c [ e_c - γ · n_c(n_c-1)/2 ]
/// で、e_c はコミュニティ内部の辺重み、n_c はノード数。**全体の規模に
/// 依存しない**ので解像度限界が無く、γ が「コミュニティ内部の辺密度の
/// 下限」という素直な意味を持つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Objective {
    Modularity,
    Cpm,
}

/// ノードの重み付き次数 k_i（自己ループは2回数える）。
fn weighted_degree(graph: &Graph, node: usize) -> f64 {
    graph.adjacency[node]
        .iter()
        .map(|&(j, w)| if j == node { 2.0 * w as f64 } else { w as f64 })
        .sum()
}

/// グラフ全体の 2m（＝全ノードの重み付き次数の和）。
fn total_degree(graph: &Graph) -> f64 {
    (0..graph.adjacency.len()).map(|i| weighted_degree(graph, i)).sum()
}

/// 与えられた分割のモジュラリティ Q。クラスタリング結果の品質を、
/// アルゴリズムに依らず同じ物差しで比べるために公開する
/// （LPAの出力にもそのまま適用できる）。
pub fn modularity(graph: &Graph, communities: &[usize], resolution: f32) -> f64 {
    let m2 = total_degree(graph);
    if m2 <= 0.0 {
        return 0.0;
    }
    let mut internal: HashMap<usize, f64> = HashMap::new();
    let mut total: HashMap<usize, f64> = HashMap::new();

    for i in 0..graph.adjacency.len() {
        let ci = communities[i];
        *total.entry(ci).or_default() += weighted_degree(graph, i);
        for &(j, w) in &graph.adjacency[i] {
            if communities[j] != ci {
                continue;
            }
            *internal.entry(ci).or_default() += if j == i { 2.0 * w as f64 } else { w as f64 };
        }
    }

    total
        .iter()
        .map(|(c, &tot)| {
            let inside = internal.get(c).copied().unwrap_or(0.0);
            inside / m2 - resolution as f64 * (tot / m2) * (tot / m2)
        })
        .sum()
}

/// CPMの目的関数値。`modularity` と同じく、分割の品質を後から評価するために公開する。
pub fn cpm_quality(graph: &Graph, communities: &[usize], resolution: f32) -> f64 {
    let mut internal: HashMap<usize, f64> = HashMap::new();
    let mut size: HashMap<usize, f64> = HashMap::new();
    for i in 0..graph.adjacency.len() {
        let ci = communities[i];
        *size.entry(ci).or_default() += 1.0;
        for &(j, w) in &graph.adjacency[i] {
            if communities[j] != ci {
                continue;
            }
            // 内部の辺重み e_c は無向辺を1回だけ数える
            *internal.entry(ci).or_default() += if j == i { w as f64 } else { 0.5 * w as f64 };
        }
    }
    size.iter()
        .map(|(c, &n)| {
            let inside = internal.get(c).copied().unwrap_or(0.0);
            inside - resolution as f64 * n * (n - 1.0) / 2.0
        })
        .sum()
}

/// Louvain法でクラスタリングする。戻り値はノードごとのコミュニティID
/// （0始まりの連番。`lpa::group_by_label` がそのまま使える）。
///
/// 走査順はノードID昇順で固定してあり乱択を一切使わないので、同じ入力
/// からは常に同じ分割が出る（原論文の実装はノード順をランダム化するが、
/// ここでは実データの再現性を優先する）。
pub fn louvain(graph: &Graph, objective: Objective, resolution: f32, max_passes: usize) -> Vec<usize> {
    let n = graph.adjacency.len();
    if n == 0 {
        return Vec::new();
    }
    // 元のノード → 現在の階層でのコミュニティ
    let mut node_to_community: Vec<usize> = (0..n).collect();
    let mut current = Graph { adjacency: graph.adjacency.clone() };
    // 集約後のノードは元のノードを複数まとめたものなので、CPMの n_c を
    // 正しく数えるには「このノードが元の何ノード分か」を持ち回る必要がある。
    let mut sizes: Vec<f64> = vec![1.0; n];

    for _ in 0..max_passes {
        let local = local_moving(&current, objective, resolution, &sizes);
        let (renumbered, count) = renumber(&local);
        // 1つも畳まれなかった＝これ以上目的関数を上げられない
        if count == current.adjacency.len() {
            break;
        }
        for c in node_to_community.iter_mut() {
            *c = renumbered[*c];
        }
        let mut next_sizes = vec![0.0; count];
        for (node, &c) in renumbered.iter().enumerate() {
            next_sizes[c] += sizes[node];
        }
        current = aggregate(&current, &renumbered, count);
        sizes = next_sizes;
    }
    node_to_community
}

/// 第1段階: 各ノードを、目的関数の利得が最大の隣接コミュニティへ移す。
/// 改善が無くなるまで繰り返す。
///
/// ノード i をコミュニティ c へ移したときの利得（共通の係数を落としたもの）:
///   モジュラリティ: gain(c) = k_{i→c} - γ · Σ_tot(c) · k_i / 2m
///   CPM:            gain(c) = k_{i→c} - γ · n_c · s_i
/// どちらも k_{i→c} は i から c 内のノードへ出ている辺の重みの和。
/// CPM側の n_c は c に属する**元のノード数**、s_i は i が代表する元ノード数。
fn local_moving(graph: &Graph, objective: Objective, resolution: f32, sizes: &[f64]) -> Vec<usize> {
    let n = graph.adjacency.len();
    let m2 = total_degree(graph);
    let mut community: Vec<usize> = (0..n).collect();
    if m2 <= 0.0 {
        return community;
    }

    let degree: Vec<f64> = (0..n).map(|i| weighted_degree(graph, i)).collect();
    // モジュラリティ用の Σ_tot と、CPM用の n_c を同じ形で持つ。
    let mut community_total: Vec<f64> = degree.clone();
    let mut community_size: Vec<f64> = sizes.to_vec();

    // 隣接コミュニティごとの重み。ノードごとに作り直さず使い回す。
    let mut weight_to: HashMap<usize, f64> = HashMap::new();

    loop {
        let mut moved = false;
        for i in 0..n {
            let from = community[i];
            weight_to.clear();
            for &(j, w) in &graph.adjacency[i] {
                if j == i {
                    continue; // 自己ループはどこへ移っても同じだけ寄与する
                }
                *weight_to.entry(community[j]).or_default() += w as f64;
            }

            // いったん i を取り除いた状態で比較する
            community_total[from] -= degree[i];
            community_size[from] -= sizes[i];

            let penalty = |c: usize, totals: &[f64], counts: &[f64]| -> f64 {
                match objective {
                    Objective::Modularity => resolution as f64 * totals[c] * degree[i] / m2,
                    Objective::Cpm => resolution as f64 * counts[c] * sizes[i],
                }
            };

            let stay_weight = weight_to.get(&from).copied().unwrap_or(0.0);
            let mut best = from;
            let mut best_gain = stay_weight - penalty(from, &community_total, &community_size);
            for (&c, &w_ic) in &weight_to {
                if c == from {
                    continue;
                }
                let gain = w_ic - penalty(c, &community_total, &community_size);
                // 同点は小さいコミュニティIDへ寄せる——走査順と合わせて
                // 結果を決定的にするため。
                if gain > best_gain || (gain == best_gain && c < best) {
                    best = c;
                    best_gain = gain;
                }
            }

            community_total[best] += degree[i];
            community_size[best] += sizes[i];
            if best != from {
                community[i] = best;
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    community
}

/// コミュニティIDを 0..count の連番へ詰め直す。戻り値は
/// （ノード → 新コミュニティID, コミュニティ数）。
fn renumber(community: &[usize]) -> (Vec<usize>, usize) {
    let max_id = community.iter().copied().max().unwrap_or(0);
    let mut mapping = vec![usize::MAX; max_id + 1];
    let mut next = 0;
    for &c in community {
        if mapping[c] == usize::MAX {
            mapping[c] = next;
            next += 1;
        }
    }
    let node_to_new: Vec<usize> = community.iter().map(|&c| mapping[c]).collect();
    (node_to_new, next)
}

/// 第2段階: 各コミュニティを1ノードに畳んだ集約グラフを作る。
/// コミュニティ内部の辺は自己ループになる。
fn aggregate(graph: &Graph, node_to_community: &[usize], community_count: usize) -> Graph {
    let mut edges: Vec<HashMap<usize, f64>> = vec![HashMap::new(); community_count];
    for i in 0..graph.adjacency.len() {
        let ci = node_to_community[i];
        for &(j, w) in &graph.adjacency[i] {
            // 無向辺を1回だけ数える（i<=j のみ採用。自己ループは i==j で1回）
            if j < i {
                continue;
            }
            let cj = node_to_community[j];
            let (a, b) = if ci <= cj { (ci, cj) } else { (cj, ci) };
            *edges[a].entry(b).or_default() += w as f64;
        }
    }

    let mut adjacency: Vec<Vec<(usize, f32)>> = vec![Vec::new(); community_count];
    for (a, targets) in edges.iter().enumerate() {
        for (&b, &w) in targets {
            if a == b {
                adjacency[a].push((a, w as f32));
            } else {
                adjacency[a].push((b, w as f32));
                adjacency[b].push((a, w as f32));
            }
        }
    }
    // 隣接の順序も決定的にしておく（HashMapの反復順に依存させない）。
    for list in adjacency.iter_mut() {
        list.sort_unstable_by_key(|&(j, _)| j);
    }
    Graph { adjacency }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(n: usize, edges: &[(usize, usize, f32)]) -> Graph {
        let mut adjacency = vec![Vec::new(); n];
        for &(a, b, w) in edges {
            if a == b {
                adjacency[a].push((a, w));
            } else {
                adjacency[a].push((b, w));
                adjacency[b].push((a, w));
            }
        }
        Graph { adjacency }
    }

    /// 三角形2つを弱い橋1本で繋いだグラフ。手計算でも2コミュニティが
    /// 最適だと分かる古典的な例。
    fn two_triangles() -> Graph {
        graph(
            6,
            &[
                (0, 1, 1.0),
                (1, 2, 1.0),
                (0, 2, 1.0),
                (3, 4, 1.0),
                (4, 5, 1.0),
                (3, 5, 1.0),
                (2, 3, 0.1),
            ],
        )
    }

    fn count_communities(labels: &[usize]) -> usize {
        let mut seen: Vec<usize> = labels.to_vec();
        seen.sort_unstable();
        seen.dedup();
        seen.len()
    }

    #[test]
    fn separates_two_triangles_joined_by_a_weak_bridge() {
        let g = two_triangles();
        let c = louvain(&g, Objective::Modularity, DEFAULT_RESOLUTION, 20);
        assert_eq!(c[0], c[1]);
        assert_eq!(c[1], c[2]);
        assert_eq!(c[3], c[4]);
        assert_eq!(c[4], c[5]);
        assert_ne!(c[0], c[3], "弱い橋でしか繋がっていない2つの三角形は分かれるべき");
    }

    #[test]
    fn a_clique_stays_one_community() {
        let g = graph(
            4,
            &[(0, 1, 1.0), (0, 2, 1.0), (0, 3, 1.0), (1, 2, 1.0), (1, 3, 1.0), (2, 3, 1.0)],
        );
        let c = louvain(&g, Objective::Modularity, DEFAULT_RESOLUTION, 20);
        assert!(c.iter().all(|&x| x == c[0]), "完全グラフを割る理由は無い");
    }

    #[test]
    fn modularity_matches_a_hand_computed_value() {
        // 辺1本 (0,1,1.0) だけのグラフ。k_0 = k_1 = 1、2m = 2。
        // 同じコミュニティ: internal = 2（両方向）、tot = 2
        //   Q = 2/2 - 1*(2/2)^2 = 0
        let g = graph(2, &[(0, 1, 1.0)]);
        assert!((modularity(&g, &[0, 0], 1.0) - 0.0).abs() < 1e-12);
        // 別コミュニティ: 各 c で internal = 0、tot = 1 → -(1/2)^2 が2つ
        assert!((modularity(&g, &[0, 1], 1.0) - (-0.5)).abs() < 1e-12);
    }

    #[test]
    fn louvain_beats_the_singleton_partition_on_modularity() {
        let g = two_triangles();
        let found = louvain(&g, Objective::Modularity, DEFAULT_RESOLUTION, 20);
        let singletons: Vec<usize> = (0..6).collect();
        assert!(
            modularity(&g, &found, 1.0) > modularity(&g, &singletons, 1.0),
            "最適化した分割が、何もしない分割より悪いことはありえない"
        );
    }

    #[test]
    fn a_higher_resolution_does_not_produce_fewer_communities() {
        // 4つの三角形を輪に繋いだグラフ。γを上げると融合が割に合わなく
        // なり、クラスタは細かくなる方向にしか動かないはず。
        let mut edges = Vec::new();
        for t in 0..4 {
            let b = t * 3;
            edges.push((b, b + 1, 1.0));
            edges.push((b + 1, b + 2, 1.0));
            edges.push((b, b + 2, 1.0));
        }
        for t in 0..4 {
            edges.push((t * 3 + 2, ((t + 1) % 4) * 3, 0.5));
        }
        let g = graph(12, &edges);
        let coarse = count_communities(&louvain(&g, Objective::Modularity, 0.5, 20));
        let fine = count_communities(&louvain(&g, Objective::Modularity, 3.0, 20));
        assert!(fine >= coarse, "解像度を上げたのにクラスタ数が減った: {fine} < {coarse}");
    }

    #[test]
    fn isolated_nodes_stay_in_their_own_community() {
        let g = graph(4, &[(0, 1, 1.0)]);
        let c = louvain(&g, Objective::Modularity, DEFAULT_RESOLUTION, 20);
        assert_ne!(c[2], c[3], "辺を持たないノード同士をまとめる理由は無い");
    }

    #[test]
    fn is_deterministic_across_runs() {
        let g = two_triangles();
        assert_eq!(louvain(&g, Objective::Modularity, DEFAULT_RESOLUTION, 20), louvain(&g, Objective::Modularity, DEFAULT_RESOLUTION, 20));
    }

    #[test]
    fn an_empty_graph_is_handled() {
        let g = Graph { adjacency: Vec::new() };
        assert!(louvain(&g, Objective::Modularity, DEFAULT_RESOLUTION, 20).is_empty());
        assert_eq!(modularity(&g, &[], 1.0), 0.0);
    }

    #[test]
    fn self_loops_from_aggregation_count_twice_in_the_degree() {
        let g = graph(2, &[(0, 0, 1.0), (0, 1, 1.0)]);
        assert!((weighted_degree(&g, 0) - 3.0).abs() < 1e-12);
        assert!((weighted_degree(&g, 1) - 1.0).abs() < 1e-12);
    }

    /// 集約したグラフのモジュラリティは、元のグラフで同じ分割を評価した
    /// ものと一致しなければならない（Louvainの階層構造が壊れていない証拠）。
    #[test]
    fn aggregation_preserves_modularity() {
        let g = two_triangles();
        let local = local_moving(&g, Objective::Modularity, 1.0, &[1.0; 6]);
        let (renumbered, count) = renumber(&local);
        let before = modularity(&g, &renumbered, 1.0);
        let coarse = aggregate(&g, &renumbered, count);
        let after = modularity(&coarse, &(0..count).collect::<Vec<_>>(), 1.0);
        assert!((before - after).abs() < 1e-9, "before={before} after={after}");
    }
}

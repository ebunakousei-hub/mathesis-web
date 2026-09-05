//! Label Propagation（Raghavan et al. 2007）による重み付きグラフの
//! community detection。アーキテクチャ.txt 5.4「クラスタリング」段の
//! 実装で、concept固有の知識は一切持たない——重み付き隣接リストだけを
//! 入力に取る汎用アルゴリズムとして、`graph.rs`（概念グラフの構築）とは
//! 独立にテストできるようにしている。
//!
//! Louvainのようなモジュラリティ最適化ではなくLabel Propagationを選んだ
//! 理由: 実装がはるかに単純で正しさを検証しやすく（本ファイルのテストの
//! ように少数ノードの手作りグラフで挙動を確認できる）、大きな概念グラフ
//! でもほぼ線形時間で収束する。モジュラリティ最適化の精度が要るとわかって
//! から乗り換える、という段階的な判断。

use std::collections::HashMap;

/// 対称な重み付き隣接リスト。`adjacency[i]` はノード `i` の
/// `(隣接ノード, 辺の重み)` の一覧。対称性（iの隣接にjがあればjの隣接にも
/// iがある）は呼び出し側が保証する。
pub struct Graph {
    pub adjacency: Vec<Vec<(usize, f32)>>,
}

/// 各ノードに自分自身のIDをラベルとして与え、ノードをID昇順に走査しながら
/// 「隣接ノードの現在のラベルのうち、辺の重みの合計が最大のもの」に
/// その場（非同期）で置き換えていく。1回のパス内で更新した結果が同じ
/// パス内の後続ノードにも即座に反映される。
///
/// 最初は全ノードを一括更新する同期版で実装したが、星型グラフ（中心1
/// ノード＋葉4枚）で手計算検証したところ、「中心が葉の多数派ラベルへ→
/// 葉が中心の旧ラベルへ」を永久に繰り返す2周期振動に陥り、
/// `max_iterations` に達するまで収束しないことが判明した（これは
/// 同期版Label Propagationで知られる病理的ケース）。原論文どおりの
/// 非同期更新（走査順に即座に反映）に直すことでこの振動が起きないことを
/// 同じ例で再確認した——本ファイルの `a_star_graph_collapses_to_a_single_cluster`
/// テストがその回帰防止。走査順は決定的（ノードID昇順固定）にしてあるので、
/// 非同期でも実行結果は再現可能。
///
/// 変化がなくなるか `max_iterations` に達したら停止する。戻り値は
/// ノードごとの最終ラベル（クラスタID、連番ではない生の値）。
pub fn label_propagation(graph: &Graph, max_iterations: usize) -> Vec<usize> {
    let n = graph.adjacency.len();
    let mut labels: Vec<usize> = (0..n).collect();

    for _ in 0..max_iterations {
        let mut changed = false;

        for i in 0..n {
            if graph.adjacency[i].is_empty() {
                continue; // 孤立ノードは自分自身のラベルのまま
            }

            let mut scores: HashMap<usize, f32> = HashMap::new();
            for &(j, w) in &graph.adjacency[i] {
                *scores.entry(labels[j]).or_default() += w;
            }

            // 最大スコアのラベルを選ぶ。同点の場合はラベル値が小さい方に
            // 決める（HashMapの走査順に依存しない、決定的な結果にするため）。
            let mut best_label = labels[i];
            let mut best_score = f32::NEG_INFINITY;
            for (&label, &score) in &scores {
                if score > best_score || (score == best_score && label < best_label) {
                    best_score = score;
                    best_label = label;
                }
            }

            if best_label != labels[i] {
                changed = true;
                labels[i] = best_label; // その場で反映（非同期更新）
            }
        }

        if !changed {
            break;
        }
    }

    labels
}

/// ラベルをクラスタごとにまとめたノード集合に変換する。
pub fn group_by_label(labels: &[usize]) -> Vec<Vec<usize>> {
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for (node, &label) in labels.iter().enumerate() {
        groups.entry(label).or_default().push(node);
    }
    let mut out: Vec<Vec<usize>> = groups.into_values().collect();
    out.sort_by_key(|g| std::cmp::Reverse(g.len()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn undirected(n: usize, edges: &[(usize, usize, f32)]) -> Graph {
        let mut adjacency = vec![Vec::new(); n];
        for &(a, b, w) in edges {
            adjacency[a].push((b, w));
            adjacency[b].push((a, w));
        }
        Graph { adjacency }
    }

    #[test]
    fn isolated_nodes_keep_their_own_label() {
        let graph = undirected(3, &[]);
        let labels = label_propagation(&graph, 10);
        assert_eq!(labels, vec![0, 1, 2]);
    }

    #[test]
    fn two_disjoint_triangles_become_two_clusters() {
        // 0-1-2 の三角形と 3-4-5 の三角形。辺は同じ重みで、互いには繋がっていない。
        let graph = undirected(
            6,
            &[
                (0, 1, 1.0), (1, 2, 1.0), (0, 2, 1.0),
                (3, 4, 1.0), (4, 5, 1.0), (3, 5, 1.0),
            ],
        );
        let labels = label_propagation(&graph, 20);
        assert_eq!(labels[0], labels[1]);
        assert_eq!(labels[1], labels[2]);
        assert_eq!(labels[3], labels[4]);
        assert_eq!(labels[4], labels[5]);
        assert_ne!(labels[0], labels[3], "the two triangles must not merge — they share no edge");

        let groups = group_by_label(&labels);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].len(), 3);
        assert_eq!(groups[1].len(), 3);
    }

    #[test]
    fn a_weak_bridge_does_not_merge_two_strong_communities() {
        // 0-1-2 と 3-4-5 はそれぞれ強く結びついた三角形（重み10）。
        // 2-3 は弱い橋渡し（重み0.01）——強い三角形の内部結束に負けて、
        // 2つのコミュニティは融合しないはず。
        let graph = undirected(
            6,
            &[
                (0, 1, 10.0), (1, 2, 10.0), (0, 2, 10.0),
                (3, 4, 10.0), (4, 5, 10.0), (3, 5, 10.0),
                (2, 3, 0.01),
            ],
        );
        let labels = label_propagation(&graph, 20);
        assert_eq!(labels[0], labels[1]);
        assert_eq!(labels[1], labels[2]);
        assert_eq!(labels[3], labels[4]);
        assert_eq!(labels[4], labels[5]);
        assert_ne!(labels[0], labels[3], "a weak bridge must not overpower strong intra-cluster edges");
    }

    #[test]
    fn a_star_graph_collapses_to_a_single_cluster() {
        // 中心ノード0に4枚の葉がぶら下がるだけのグラフ。同期更新版では
        // 「中心が葉の多数派へ→葉が中心の旧ラベルへ」を無限に繰り返す
        // 2周期振動に陥ることを手計算で確認した実例（非同期更新への
        // 切り替えの動機になった回帰テスト）。
        let graph = undirected(5, &[(0, 1, 1.0), (0, 2, 1.0), (0, 3, 1.0), (0, 4, 1.0)]);
        let labels = label_propagation(&graph, 20);
        let groups = group_by_label(&labels);
        assert_eq!(groups.len(), 1, "expected a single cluster, got groups {groups:?} from labels {labels:?}");
        assert_eq!(groups[0].len(), 5);
    }

    #[test]
    fn group_by_label_sorts_largest_cluster_first() {
        let labels = vec![0, 0, 0, 1, 1, 2];
        let groups = group_by_label(&labels);
        assert_eq!(groups.iter().map(Vec::len).collect::<Vec<_>>(), vec![3, 2, 1]);
    }
}

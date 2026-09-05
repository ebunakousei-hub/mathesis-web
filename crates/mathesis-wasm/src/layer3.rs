//! ブラウザデモ向けの層3〜5（射の型付け・同値類の縮約・戦略タグ・推論エンジン）。
//!
//! `mathesis-graph` の `morphism.rs`/`quotient.rs`/`strategy.rs`/`inference.rs`
//! と同じアルゴリズム（受理済み同値射のUnion-Find縮約、エッジ合成則、最短
//! 導出パス探索）をこの純粋Rustモジュールで再現するが、ID型・永続化方式は
//! 独立させている——`mathesis-graph` 側はSQLite行・インクリメンタルキャッシュ
//! （`QuotientCache`）・Proposed/Accepted/Rejectedの3状態を持つのに対し、
//! こちらはブラウザの手動デモ（数件〜数十件規模）向けなので、射は常に
//! Accepted（人間が明示的に追加した射のみ）とし、商グラフも呼び出しのたびに
//! 全射から再計算する（この規模ではインクリメンタル化のコストに見合わない）。
//! `web/README.md` が述べる「SQLite依存をCargo featureで任意化すれば本実装に
//! 置き換えられる」という将来課題は変わらず残っている。

use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MorphismKind {
    Implication,
    Specialization,
    Generalization,
    Equivalence,
}

impl MorphismKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MorphismKind::Implication => "implication",
            MorphismKind::Specialization => "specialization",
            MorphismKind::Generalization => "generalization",
            MorphismKind::Equivalence => "equivalence",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "implication" => MorphismKind::Implication,
            "specialization" => MorphismKind::Specialization,
            "generalization" => MorphismKind::Generalization,
            "equivalence" => MorphismKind::Equivalence,
            _ => return None,
        })
    }
}

/// よく使う戦略名。`mathesis_graph::strategy::well_known` と同じ語彙を
/// ミラーする（あちらはSQLite上の任意の文字列を許すオープンなカタログだが、
/// ブラウザデモではドロップダウン候補として固定リストを示せば十分）。
pub const WELL_KNOWN_STRATEGIES: &[&str] = &[
    "induction",
    "strong_induction",
    "contradiction",
    "contrapositive",
    "diagonalization",
    "direct_construction",
    "case_split",
    "compactness",
    "pigeonhole",
    "probabilistic_method",
    "adjoint_functor",
    "universal_property",
];

#[derive(Debug, Clone)]
pub struct Morphism {
    pub id: u32,
    pub src: u32,
    pub dst: u32,
    pub kind: MorphismKind,
    pub rationale: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MorphismView {
    pub id: u32,
    pub src: u32,
    pub dst: u32,
    pub kind: String,
    pub rationale: Option<String>,
    pub strategies: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotientClassView {
    pub representative: u32,
    pub members: Vec<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HopView {
    pub src: u32,
    pub dst: u32,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InferredPathView {
    pub composed_kind: String,
    pub hops: Vec<HopView>,
}

/// エッジ合成則。`mathesis-graph::inference::compose` と同一の真理値表
/// （アーキテクチャ文書フェーズ5が明示する例そのもの）。
pub fn compose(first: MorphismKind, second: MorphismKind) -> Option<MorphismKind> {
    use MorphismKind::{Equivalence, Generalization, Implication, Specialization};
    match (first, second) {
        (Equivalence, k) => Some(k),
        (k, Equivalence) => Some(k),
        (Implication, Implication) => Some(Implication),
        (Specialization, Specialization) => Some(Specialization),
        (Generalization, Generalization) => Some(Generalization),
        (Specialization, Implication) => Some(Implication),
        _ => None,
    }
}

#[derive(Debug, Default)]
struct UnionFind {
    parent: HashMap<u32, u32>,
}

impl UnionFind {
    fn find(&mut self, x: u32) -> u32 {
        let p = *self.parent.entry(x).or_insert(x);
        if p != x {
            let r = self.find(p);
            self.parent.insert(x, r);
            r
        } else {
            x
        }
    }

    fn union(&mut self, a: u32, b: u32) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra == rb {
            return;
        }
        // 小さいidを代表元にする（決定的）。
        if ra < rb {
            self.parent.insert(rb, ra);
        } else {
            self.parent.insert(ra, rb);
        }
    }

    fn known_ids(&self) -> Vec<u32> {
        self.parent.keys().copied().collect()
    }
}

/// 受理済み同値射から「判断id → 代表元」写像を作る。同値エッジに一度も
/// 関わっていない判断ノードはここに現れない（自分自身が代表元のまま）。
pub fn representative_map(morphisms: &[Morphism]) -> BTreeMap<u32, u32> {
    let mut uf = UnionFind::default();
    for m in morphisms {
        if m.kind == MorphismKind::Equivalence {
            uf.union(m.src, m.dst);
        }
    }
    uf.known_ids().into_iter().map(|id| (id, uf.find(id))).collect()
}

/// 代表元 → クラス成員（id昇順）。単集合（同値エッジに関わっていない）は
/// 呼び出し側で個別に無視してよい——ここでは触れた判断ノードだけを返す。
pub fn quotient_classes(representative: &BTreeMap<u32, u32>) -> Vec<QuotientClassView> {
    let mut inv: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for (id, rep) in representative {
        inv.entry(*rep).or_default().push(*id);
    }
    inv.into_iter()
        .map(|(representative, mut members)| {
            members.sort_unstable();
            QuotientClassView { representative, members }
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CollapsedEdge {
    src: u32,
    dst: u32,
    kind: MorphismKind,
}

/// 受理済み射の端点を代表元へ写し、同値エッジ自体とクラス内部に潰れる自己
/// ループを落とす。`mathesis-graph::quotient::collapse_morphisms` と同じ規則。
fn collapse(morphisms: &[Morphism], representative: &BTreeMap<u32, u32>) -> Vec<CollapsedEdge> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for m in morphisms {
        if m.kind == MorphismKind::Equivalence {
            continue;
        }
        let src = representative.get(&m.src).copied().unwrap_or(m.src);
        let dst = representative.get(&m.dst).copied().unwrap_or(m.dst);
        if src == dst {
            continue;
        }
        if !seen.insert((src, dst, m.kind)) {
            continue;
        }
        out.push(CollapsedEdge { src, dst, kind: m.kind });
    }
    out
}

/// `from` から `to` への、常に単一の導出関係（[`compose`] で還元可能）になる
/// 最短パスを探す。`mathesis-graph::inference::shortest_derivation` と同じ
/// BFS（合成できないホップに差し掛かった探索枝はそこで打ち切る）。
pub fn shortest_derivation(morphisms: &[Morphism], from: u32, to: u32) -> Option<InferredPathView> {
    let representative = representative_map(morphisms);
    let from = representative.get(&from).copied().unwrap_or(from);
    let to = representative.get(&to).copied().unwrap_or(to);

    let collapsed = collapse(morphisms, &representative);
    let mut adjacency: HashMap<u32, Vec<&CollapsedEdge>> = HashMap::new();
    for e in &collapsed {
        adjacency.entry(e.src).or_default().push(e);
    }

    let mut visited: std::collections::HashSet<(u32, Option<MorphismKind>)> = std::collections::HashSet::new();
    let mut queue: std::collections::VecDeque<(u32, Option<MorphismKind>, Vec<HopView>)> =
        std::collections::VecDeque::new();
    visited.insert((from, None));
    queue.push_back((from, None, Vec::new()));

    while let Some((cur, acc_kind, path)) = queue.pop_front() {
        for edge in adjacency.get(&cur).into_iter().flatten() {
            let composed = match acc_kind {
                None => Some(edge.kind),
                Some(k) => compose(k, edge.kind),
            };
            let Some(composed) = composed else { continue };
            let mut next_path = path.clone();
            next_path.push(HopView { src: edge.src, dst: edge.dst, kind: edge.kind.as_str().to_string() });
            if edge.dst == to {
                return Some(InferredPathView {
                    composed_kind: composed.as_str().to_string(),
                    hops: next_path,
                });
            }
            let state = (edge.dst, Some(composed));
            if visited.insert(state) {
                queue.push_back((edge.dst, Some(composed), next_path));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(id: u32, src: u32, dst: u32, kind: MorphismKind) -> Morphism {
        Morphism { id, src, dst, kind, rationale: None }
    }

    #[test]
    fn composition_table_matches_documented_rules() {
        use MorphismKind::{Equivalence, Generalization, Implication, Specialization};
        assert_eq!(compose(Implication, Implication), Some(Implication));
        assert_eq!(compose(Specialization, Specialization), Some(Specialization));
        assert_eq!(compose(Generalization, Generalization), Some(Generalization));
        assert_eq!(compose(Specialization, Implication), Some(Implication));
        assert_eq!(compose(Equivalence, Implication), Some(Implication));
        assert_eq!(compose(Specialization, Equivalence), Some(Specialization));
        assert_eq!(compose(Implication, Specialization), None);
        assert_eq!(compose(Generalization, Implication), None);
        assert_eq!(compose(Implication, Generalization), None);
        assert_eq!(compose(Specialization, Generalization), None);
        assert_eq!(compose(Generalization, Specialization), None);
    }

    #[test]
    fn equivalence_edges_collapse_into_shared_class() {
        let morphisms = vec![
            m(1, 10, 20, MorphismKind::Equivalence),
            m(2, 20, 30, MorphismKind::Equivalence),
            m(3, 40, 50, MorphismKind::Implication),
        ];
        let rep = representative_map(&morphisms);
        // 10, 20, 30は同じ代表元。40, 50は同値エッジに関わっていないので現れない。
        assert_eq!(rep.get(&10), rep.get(&20));
        assert_eq!(rep.get(&20), rep.get(&30));
        assert!(!rep.contains_key(&40));
        assert!(!rep.contains_key(&50));

        let classes = quotient_classes(&rep);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].members, vec![10, 20, 30]);
    }

    #[test]
    fn shortest_derivation_composes_specialization_then_implication() {
        // 10 --Specialization--> 20 --Implication--> 30 は Implication 1本に還元できる。
        let morphisms = vec![
            m(1, 10, 20, MorphismKind::Specialization),
            m(2, 20, 30, MorphismKind::Implication),
        ];
        let path = shortest_derivation(&morphisms, 10, 30).expect("path should exist");
        assert_eq!(path.composed_kind, "implication");
        assert_eq!(path.hops.len(), 2);
    }

    #[test]
    fn shortest_derivation_is_none_when_composition_breaks() {
        // 一般化してから含意に戻る経路は、健全な単一の矢としては表せない。
        let morphisms = vec![
            m(1, 10, 20, MorphismKind::Generalization),
            m(2, 20, 30, MorphismKind::Implication),
        ];
        assert!(shortest_derivation(&morphisms, 10, 30).is_none());
    }

    #[test]
    fn shortest_derivation_transparently_uses_equivalence_class_members() {
        // 10 ≡ 15 で、15 --Implication--> 30 なら、10 から 30 への導出が見つかる。
        let morphisms = vec![
            m(1, 10, 15, MorphismKind::Equivalence),
            m(2, 15, 30, MorphismKind::Implication),
        ];
        let path = shortest_derivation(&morphisms, 10, 30).expect("path should exist via equivalence class");
        assert_eq!(path.composed_kind, "implication");
    }
}

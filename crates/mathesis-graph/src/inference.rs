//! 層5: クエリ・推論エンジン層（Inference Engine Layer）
//!
//! 層3のエッジ（含意・特殊化・一般化）を合成し、単純な到達可能性のグラフ探索
//! だけでは出せない「導出パス」を計算する。同値エッジは層2/3で既に
//! `GraphStore::quotient_graph()` が代表元へ縮約しているため、この層はそれを
//! そのまま使うだけで「同値類はクエリ時に透過的にマージする」という
//! アーキテクチャ文書の要件を追加実装なしに満たしている。
//!
//! ## 対応する範囲と、意図的に対応しない範囲
//!
//! 実装するのは次の2つ:
//! 1. エッジ合成ルール [`compose`]（例: `Specialization ∘ Implication = Implication`、
//!    フェーズ5ロードマップが明示している例そのもの）
//! 2. その合成則の下で常に単一の導出関係へ還元できる、最短の導出パス探索
//!    [`shortest_derivation`]（アーキテクチャ文書のクエリ例「群論の定義から、
//!    アーベル群の性質Pを導く最短の依存パスは？」に対応する）
//!
//! アーキテクチャ文書はこの層に「カテゴリカルな極限（積・余積）の計算」と
//! 「グラフDB上のDatalogライクなルールエンジン（Souffléの併設）」も挙げているが、
//! どちらも見送っている:
//! - 積・余積は、現在のノード/エッジモデル（Theorem/Definition/Axiom を
//!   `Judgment(Γ⊢P)` に統一し、射を4種に型付けしただけで、恒等射や結合律を持つ
//!   厳密な圏にはまだなっていない）の上では「2つの定理の積」が数学的に何を
//!   意味するかがまだ定義できない。中身のない実装を作るより、層3が本当に
//!   圏の公理を満たす構造に育った段階で改めて設計するほうが誠実だと判断した。
//! - Datalogエンジンの併設は、現状のエッジ種別4つ・合成則10通り程度の規模では
//!   外部プロセスを運用するコストが素朴なBFSの実装コストを明らかに上回る。
//!   実データでBFSが遅くなった段階（フェーズ3レビューが指摘したのと同種の
//!   スケーラビリティ課題が層5でも顕在化した段階）で検討すればよい。

use crate::model::JudgmentId;
use crate::morphism::{MorphismId, MorphismKind};
use crate::store::{GraphStore, Result};
use std::collections::{HashMap, HashSet, VecDeque};

/// 2つの射（層3のエッジ）をこの順に辿ったとき、単一の関係へ合成できるならその
/// 種類を返す。合成できない組み合わせ（例: 特殊化してから一般化に戻るような
/// 向きの混在）は `None`。「合成できない」は「矛盾している」ではなく単に
/// 「1本の矢では表せない」という意味であり、経路そのもの（各ホップの証拠）は
/// 呼び出し側でなお有効な情報として扱える。
///
/// 各規則の妥当性:
/// - `Implication ∘ Implication = Implication` — `→` の推移律そのもの。
/// - `Specialization ∘ Specialization = Specialization` — 文脈の拡張は推移的
///   （Γ₁⊆Γ₂⊆Γ₃ なら Γ₁⊆Γ₃）。
/// - `Generalization ∘ Generalization = Generalization` — 上の双対。
/// - `Specialization ∘ Implication = Implication` — `Specialization(A,B)` は
///   「Bの文脈はAの文脈を包む」ことを意味する。健全な論理では前提を増やしても
///   結論は保たれる（文脈の弱化）ため、Aの成立はBの成立を含意する。これと
///   `Implication(B,C)` を合成すれば `A → C`。アーキテクチャ文書がフェーズ5の
///   実装例として明示しているルール。
/// - `Equivalence ∘ K = K`、`K ∘ Equivalence = K`（`K` は任意）— 同値な命題への
///   置き換えは関係の種類を変えない。ただし `Equivalence` エッジ自体は
///   `quotient_graph()` の時点で既に代表元へ縮約済みのため、実際にこの関数へ
///   渡ってくることは通常ない。
/// - それ以外（`Implication ∘ Specialization`、一般化が絡む組み合わせなど）は
///   すべて `None`。例えば「一般化してから含意」は「一般化された（より弱い）
///   命題が元の強い命題を含意する」という向きの逆転を要求してしまい健全ではない。
pub fn compose(first: MorphismKind, second: MorphismKind) -> Option<MorphismKind> {
    use MorphismKind::*;
    match (first, second) {
        (Equivalence, k) => Some(k),
        (k, Equivalence) => Some(k),
        (Implication, Implication) => Some(Implication),
        (Specialization, Specialization) => Some(Specialization),
        (Generalization, Generalization) => Some(Generalization),
        (Specialization, Implication) => Some(Implication),
        (Implication, Specialization) => None,
        (Generalization, Implication) => None,
        (Implication, Generalization) => None,
        (Specialization, Generalization) => None,
        (Generalization, Specialization) => None,
    }
}

/// 導出パスの1ホップ分。`witnesses` はこの区間を裏付ける（同値縮約前の）実際の
/// 射ID群——同値クラス内に複数の証拠がありうるため複数持てる。
#[derive(Debug, Clone)]
pub struct PathHop {
    pub src: JudgmentId,
    pub dst: JudgmentId,
    pub kind: MorphismKind,
    pub witnesses: Vec<MorphismId>,
}

#[derive(Debug, Clone)]
pub struct InferredPath {
    pub from: JudgmentId,
    pub to: JudgmentId,
    /// パス全体を [`compose`] で左から畳み込んだ、単一の導出関係の種類。
    pub composed_kind: MorphismKind,
    pub hops: Vec<PathHop>,
}

/// `from` から `to` へ、受理済みの層3エッジ（同値は代表元へ縮約済み）だけを
/// 辿って到達できるかどうか。種類の合成可否は考慮しない、素朴な到達可能性。
pub fn is_reachable(store: &GraphStore, from: JudgmentId, to: JudgmentId) -> Result<bool> {
    let adjacency = build_adjacency(store)?;
    let from = store.representative(from)?;
    let to = store.representative(to)?;
    if from == to {
        return Ok(true);
    }
    let mut visited: HashSet<JudgmentId> = HashSet::from([from]);
    let mut queue: VecDeque<JudgmentId> = VecDeque::from([from]);
    while let Some(cur) = queue.pop_front() {
        for edge in adjacency.get(&cur).into_iter().flatten() {
            if edge.dst == to {
                return Ok(true);
            }
            if visited.insert(edge.dst) {
                queue.push_back(edge.dst);
            }
        }
    }
    Ok(false)
}

/// `from` から `to` への、常に単一の導出関係（[`compose`] で還元可能）になる
/// 最短パスを探す。ホップ数最小。合成できないホップに差し掛かった探索枝は
/// そこで打ち切られるため、素朴な [`is_reachable`] より制約が強く、`None` に
/// なる可能性も高い——「経路はあるが、1本の矢としては説明できない」場合を
/// 正しく区別するのがこの関数の目的（例: 特殊化してから一般化に戻る経路は
/// 到達可能ではあっても導出にはならない）。
pub fn shortest_derivation(
    store: &GraphStore,
    from: JudgmentId,
    to: JudgmentId,
) -> Result<Option<InferredPath>> {
    let adjacency = build_adjacency(store)?;
    let from = store.representative(from)?;
    let to = store.representative(to)?;

    let mut visited: HashSet<(JudgmentId, Option<MorphismKind>)> = HashSet::new();
    let mut queue: VecDeque<(JudgmentId, Option<MorphismKind>, Vec<PathHop>)> = VecDeque::new();
    visited.insert((from, None));
    queue.push_back((from, None, Vec::new()));

    while let Some((cur, acc_kind, path)) = queue.pop_front() {
        for edge in adjacency.get(&cur).into_iter().flatten() {
            let composed = match acc_kind {
                None => Some(edge.kind),
                Some(k) => compose(k, edge.kind),
            };
            let Some(composed) = composed else {
                continue;
            };
            let mut next_path = path.clone();
            next_path.push(edge.clone());
            if edge.dst == to {
                return Ok(Some(InferredPath {
                    from,
                    to,
                    composed_kind: composed,
                    hops: next_path,
                }));
            }
            let state = (edge.dst, Some(composed));
            if visited.insert(state) {
                queue.push_back((edge.dst, Some(composed), next_path));
            }
        }
    }
    Ok(None)
}

fn build_adjacency(store: &GraphStore) -> Result<HashMap<JudgmentId, Vec<PathHop>>> {
    let q = store.quotient_graph()?;
    let mut adjacency: HashMap<JudgmentId, Vec<PathHop>> = HashMap::new();
    for m in q.morphisms {
        adjacency.entry(m.src).or_default().push(PathHop {
            src: m.src,
            dst: m.dst,
            kind: m.kind,
            witnesses: m.witnesses,
        });
    }
    Ok(adjacency)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composition_table_matches_documented_rules() {
        use MorphismKind::*;
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
}

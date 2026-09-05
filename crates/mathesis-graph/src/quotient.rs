//! 受理済み同値エッジから同値類を作り、クエリ時の商グラフ（Quotient Graph）を構築する。
//! 判断ノード自体は削除・物理マージしない。出典とコンテキストを残すため、
//! 代表元への縮約はクエリ側で行う。
//!
//! `UnionFind` 自体はステートレスな道具だが、`QuotientCache`（`GraphStore` が
//! `RefCell` で永続的に保持する）は「これまでに取り込んだ受理済み同値射」を
//! 覚えておくことで、新しく1本同値エッジが受理されるたびに全判断・全受理済み
//! 射を読み直して Union-Find を一から作り直す必要をなくす（フェーズ3
//! アーキテクチャレビュー問題5・フェーズ3.2「Union-Find永続化」への対応）。

use crate::model::JudgmentId;
use crate::morphism::{MorphismKind, MorphismRecord};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Debug, Clone, Default)]
pub struct UnionFind {
    parent: HashMap<i64, i64>,
}

impl UnionFind {
    pub fn add(&mut self, x: i64) {
        self.parent.entry(x).or_insert(x);
    }

    pub fn find(&mut self, x: i64) -> i64 {
        self.add(x);
        let p = self.parent[&x];
        if p != x {
            let r = self.find(p);
            self.parent.insert(x, r);
            r
        } else {
            x
        }
    }

    pub fn union(&mut self, a: i64, b: i64) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra == rb {
            return;
        }
        // 小さい id を代表元にする（決定的）。
        if ra < rb {
            self.parent.insert(rb, ra);
        } else {
            self.parent.insert(ra, rb);
        }
    }

    /// これまでに `add`/`find`/`union` のいずれかで一度でも登場した ID の一覧
    /// （順不同）。同値エッジに一度も関わっていない判断ノードはここに現れない
    /// ——それらは最初から「自分自身が代表元」で正しく、Union-Find に載せる
    /// 必要すらないというのが、このキャッシュ設計の要になっている。
    pub fn known_ids(&self) -> Vec<i64> {
        self.parent.keys().copied().collect()
    }
}

/// `GraphStore` が保持する永続 Union-Find キャッシュ。
///
/// `applied` は「もう `uf` に取り込んだ受理済み同値射の ID」の集合。射の承認は
/// 「挿入時に直接 Accepted」（`annotate`/`insert_morphism`）と「Proposed から
/// `accept_morphism` で昇格」の2経路があり、後者は行の `id` 自体は変わらず
/// `status` だけが変わるため、`id` の昇順ウォーターマークでは「後から承認された
/// 古いID」を取りこぼす。射ID集合で管理することで承認の順序に依存しないように
/// している。
#[derive(Debug, Default)]
pub struct QuotientCache {
    uf: UnionFind,
    applied: HashSet<i64>,
}

impl QuotientCache {
    /// Union-Find を空の状態に戻す。`reject_morphism` で一度受理した同値エッジを
    /// 取り消すときのように、Union-Find が原理的に「分割」できない変更が
    /// 起きた場合はここから作り直すしかない。
    pub fn reset(&mut self) {
        self.uf = UnionFind::default();
        self.applied.clear();
    }

    /// 受理済み同値射 `(morphism_id, src, dst)` の列を渡し、まだ取り込んでいない
    /// ものだけを Union-Find に反映する。既に取り込み済みの射は無視するので、
    /// 呼び出し側は差分計算をせずに「現在受理されている同値射全部」を毎回
    /// 渡してよい（実際のユニオン演算が起きるのは新規分だけ）。新規に取り込んだ
    /// ものが1件でもあれば `true` を返す——呼び出し側はこれを見て、
    /// `representative_id` 列への書き戻し（DB書き込み）を、Union-Find が実際に
    /// 変化したときだけに絞れる。
    pub fn absorb<I: IntoIterator<Item = (i64, i64, i64)>>(&mut self, accepted_equivalences: I) -> bool {
        let mut changed = false;
        for (morphism_id, src, dst) in accepted_equivalences {
            if self.applied.insert(morphism_id) {
                self.uf.union(src, dst);
                changed = true;
            }
        }
        changed
    }

    /// 現在の Union-Find の状態から、これまでに同値エッジへ関わったことのある
    /// 判断ノードだけを対象にした「判断 id → 代表元」写像を作る。
    pub fn representative_map(&mut self) -> BTreeMap<JudgmentId, JudgmentId> {
        self.uf
            .known_ids()
            .into_iter()
            .map(|id| (JudgmentId(id), JudgmentId(self.uf.find(id))))
            .collect()
    }
}

/// 代表元 → クラス成員（id 昇順）。`representative` に載っていない（＝同値エッジに
/// 一度も関わっていない）判断ノードはここにも現れない。
pub fn invert_classes(
    representative: &BTreeMap<JudgmentId, JudgmentId>,
) -> BTreeMap<JudgmentId, Vec<JudgmentId>> {
    let mut inv: BTreeMap<JudgmentId, Vec<JudgmentId>> = BTreeMap::new();
    for (id, rep) in representative {
        inv.entry(*rep).or_default().push(*id);
    }
    for members in inv.values_mut() {
        members.sort();
    }
    inv
}

/// 受理済み射の端点を代表元に写し、クラス内部の同値エッジは落とす。
/// 同じ (src_rep, dst_rep, kind) は1本に畳む。`representative` に載っていない
/// 端点（同値エッジに一度も関わっていない判断ノード）は自分自身をそのまま使う。
pub fn collapse_morphisms(
    accepted: &[MorphismRecord],
    representative: &BTreeMap<JudgmentId, JudgmentId>,
) -> Vec<CollapsedMorphism> {
    let mut seen: BTreeSet<(i64, i64, MorphismKind)> = BTreeSet::new();
    let mut out = Vec::new();
    for e in accepted {
        let src = representative.get(&e.src).copied().unwrap_or(e.src);
        let dst = representative.get(&e.dst).copied().unwrap_or(e.dst);
        if e.kind == MorphismKind::Equivalence {
            continue;
        }
        if src == dst {
            continue;
        }
        if !seen.insert((src.0, dst.0, e.kind)) {
            continue;
        }
        out.push(CollapsedMorphism {
            src,
            dst,
            kind: e.kind,
            witnesses: vec![e.id],
        });
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollapsedMorphism {
    pub src: JudgmentId,
    pub dst: JudgmentId,
    pub kind: MorphismKind,
    pub witnesses: Vec<crate::morphism::MorphismId>,
}

#[derive(Debug, Clone)]
pub struct QuotientGraph {
    /// 判断 id → 代表元（同値エッジに一度も関わっていない判断ノードは含まない）
    pub representative: BTreeMap<JudgmentId, JudgmentId>,
    pub classes: BTreeMap<JudgmentId, Vec<JudgmentId>>,
    pub morphisms: Vec<CollapsedMorphism>,
}

impl QuotientGraph {
    /// `id` の同値類（自分自身を含む）。`representative` に載っていない場合は
    /// 「同値エッジに一度も関わっていない＝自分だけの単集合」として扱う。
    pub fn class_of(&self, id: JudgmentId) -> Vec<JudgmentId> {
        let rep = self.representative.get(&id).copied().unwrap_or(id);
        self.classes
            .get(&rep)
            .cloned()
            .unwrap_or_else(|| vec![id])
    }
}

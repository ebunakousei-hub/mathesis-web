//! 層3のヒューリスティック。命名規則とステートメント／コンテキストの構造比較から
//! 候補を出すが、**承認はしない**。アーキテクチャの警告どおり、同値の自動検出を
//! 最初から本採用すると破綻するため、出力は常に `Proposed` に落とす。
//!
//! ## 実装済みヒューリスティック一覧
//!
//! | 種別 | 根拠 | 信頼度 |
//! |------|------|--------|
//! | ステートメント同一 + コンテキスト等価 | 正規化ハッシュ完全一致 | 0.90 |
//! | ステートメント同一 + コンテキスト真包含 | 一方が他方の特殊環境 | 0.75 |
//! | `foo_iff_bar` 命名 | `iff`/`equiv` セパレータ | 0.45 |
//! | `abelian_group` ⊃ `group` 修飾語命名 | QUALIFIERS リスト | 0.35 |
//! | `_of_` パターン (bar_of_foo → foo→bar) | 数学慣習的命名 | 0.40 |
//! | `_implies_` パターン | 明示的含意命名 | 0.55 |
//! | `_generalizes_` / `_specializes_` パターン | 明示的関係命名 | 0.50 |
//! | `corollary_` / `cor_` プレフィックス | 系は定理への含意 | 0.30 |

use crate::model::{JudgmentId, JudgmentKind};
use crate::morphism::{MorphismKind, MorphismProposal};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// ヒューリスティックが使う判断の軽量ビュー（式 AST は展開しない）。
#[derive(Debug, Clone)]
pub struct JudgmentLite {
    pub id: JudgmentId,
    pub kind: JudgmentKind,
    pub name: Option<String>,
    pub statement_hash: String,
    /// コンテキスト Γ の型ハッシュ。名前は捨て、多重集合として比較する。
    pub context_hashes: Vec<String>,
}

impl JudgmentLite {
    pub fn context_multiset(&self) -> BTreeMap<&str, usize> {
        let mut m = BTreeMap::new();
        for h in &self.context_hashes {
            *m.entry(h.as_str()).or_insert(0) += 1;
        }
        m
    }
}

/// `propose_morphisms()` を呼ぶたびに全判断を SQLite から読み直して
/// `JudgmentLite` へ組み立て直す（JOIN・JSON パース込み）のを避けるための
/// 永続キャッシュ（フェーズ3.2「インクリメンタル提案」への対応）。
///
/// 判断ノードは挿入後に名前・ステートメント・コンテキストが変わることはない
/// （更新APIが存在しない）ため、一度取り込んだ `JudgmentLite` は将来にわたって
/// 有効であり続ける。よって「最後に取り込んだ判断ID」を単純な昇順ウォーター
/// マークとして使ってよい——これは `QuotientCache` が同値射の「承認」に対して
/// 単純なIDウォーターマークを使えなかった（承認は挿入後に起こりうる別イベント
/// なので順序が保証されない）のとは事情が異なる。判断は「挿入されたら
/// それで確定」なので、IDウォーターマークで安全に取りこぼしなく差分を追える。
///
/// ヒューリスティック本体（`propose()`）は、この構造体が返す蓄積済みの全判断に
/// 対して毎回実行する。ここで省いているのは「DBへの再アクセスと再構築」で
/// あって「計算」ではないため、再現率は一切変わらない（新規判断と既存判断の
/// 組み合わせも含め、常に完全な判断集合に対してヒューリスティックが走る）。
#[derive(Debug, Default)]
pub struct JudgmentLiteCache {
    judgments: Vec<JudgmentLite>,
    last_id: i64,
}

impl JudgmentLiteCache {
    /// 新規に取得した `JudgmentLite`（`last_id()` より大きい ID のものだけを
    /// 渡すのが呼び出し側の責務）を蓄積に追加する。
    pub fn absorb(&mut self, new: Vec<JudgmentLite>) {
        for j in new {
            if j.id.0 > self.last_id {
                self.last_id = j.id.0;
            }
            self.judgments.push(j);
        }
    }

    /// これまでに取り込んだ最大の判断ID（まだ何も取り込んでいなければ 0）。
    pub fn last_id(&self) -> i64 {
        self.last_id
    }

    pub fn judgments(&self) -> &[JudgmentLite] {
        &self.judgments
    }
}

fn proper_multiset_subset(small: &BTreeMap<&str, usize>, large: &BTreeMap<&str, usize>) -> bool {
    if small == large {
        return false;
    }
    for (k, n) in small {
        if large.get(k).copied().unwrap_or(0) < *n {
            return false;
        }
    }
    true
}

fn contexts_equal(a: &JudgmentLite, b: &JudgmentLite) -> bool {
    a.context_multiset() == b.context_multiset()
}

/// ストア上の判断列から、まだ永続化していない射候補を列挙する。
pub fn propose(judgments: &[JudgmentLite]) -> Vec<MorphismProposal> {
    let mut out = Vec::new();
    let mut seen: BTreeSet<(i64, i64, MorphismKind)> = BTreeSet::new();

    let mut push = |p: MorphismProposal| {
        let (a, b) = if p.kind == MorphismKind::Equivalence && p.src.0 > p.dst.0 {
            (p.dst.0, p.src.0)
        } else {
            (p.src.0, p.dst.0)
        };
        if seen.insert((a, b, p.kind)) {
            out.push(p);
        }
    };

    propose_statement_structure(judgments, &mut push);
    propose_iff_names(judgments, &mut push);
    propose_qualifier_names(judgments, &mut push);
    propose_of_pattern(judgments, &mut push);
    propose_implies_pattern(judgments, &mut push);
    propose_generalizes_pattern(judgments, &mut push);
    propose_corollary_pattern(judgments, &mut push);

    out.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.src.0.cmp(&b.src.0))
            .then_with(|| a.dst.0.cmp(&b.dst.0))
    });
    out
}

// ---------------------------------------------------------------------------
// ヒューリスティック実装
// ---------------------------------------------------------------------------

/// 同一正規化ステートメントを共有する判断同士。
/// Γ が等しい → 同値候補。Γ が真の包含 → 特殊化（小さい Γ ⇒ 大きい Γ）。
fn propose_statement_structure(
    judgments: &[JudgmentLite],
    push: &mut impl FnMut(MorphismProposal),
) {
    let mut by_hash: HashMap<&str, Vec<&JudgmentLite>> = HashMap::new();
    for j in judgments {
        by_hash.entry(j.statement_hash.as_str()).or_default().push(j);
    }

    for group in by_hash.values() {
        if group.len() < 2 {
            continue;
        }
        for i in 0..group.len() {
            for k in (i + 1)..group.len() {
                let a = group[i];
                let b = group[k];
                if contexts_equal(a, b) {
                    push(MorphismProposal {
                        src: a.id,
                        dst: b.id,
                        kind: MorphismKind::Equivalence,
                        rationale: format!(
                            "same canonical statement hash {} and equal context signatures",
                            a.statement_hash
                        ),
                        confidence: 0.9,
                    });
                } else if proper_multiset_subset(&a.context_multiset(), &b.context_multiset()) {
                    // a のコンテキストが b の真の部分集合 → b のほうが強い条件下での成立 → b は a の特殊化
                    push(MorphismProposal {
                        src: a.id,
                        dst: b.id,
                        kind: MorphismKind::Specialization,
                        rationale: format!(
                            "statement {} shared; context of {} properly extends {}",
                            a.statement_hash, b.id.0, a.id.0
                        ),
                        confidence: 0.75,
                    });
                } else if proper_multiset_subset(&b.context_multiset(), &a.context_multiset()) {
                    push(MorphismProposal {
                        src: b.id,
                        dst: a.id,
                        kind: MorphismKind::Specialization,
                        rationale: format!(
                            "statement {} shared; context of {} properly extends {}",
                            a.statement_hash, a.id.0, b.id.0
                        ),
                        confidence: 0.75,
                    });
                }
            }
        }
    }
}

/// `foo_iff_bar` / `foo_equiv_bar` という名前の判断があれば foo と bar を同値候補にする。
fn propose_iff_names(judgments: &[JudgmentLite], push: &mut impl FnMut(MorphismProposal)) {
    let mut by_name: HashMap<String, Vec<JudgmentId>> = HashMap::new();
    for j in judgments {
        if let Some(n) = &j.name {
            by_name
                .entry(n.to_ascii_lowercase())
                .or_default()
                .push(j.id);
        }
    }

    for j in judgments {
        let Some(name) = j.name.as_deref() else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        let Some((left, right)) = split_iff(&lower) else {
            continue;
        };
        let lefts = by_name.get(left).cloned().unwrap_or_default();
        let rights = by_name.get(right).cloned().unwrap_or_default();
        for lid in &lefts {
            for rid in &rights {
                if lid.0 == rid.0 {
                    continue;
                }
                push(MorphismProposal {
                    src: *lid,
                    dst: *rid,
                    kind: MorphismKind::Equivalence,
                    rationale: format!(
                        "name {} suggests equivalence between {} and {}",
                        name, left, right
                    ),
                    confidence: 0.45,
                });
            }
        }
    }
}

fn split_iff(name: &str) -> Option<(&str, &str)> {
    for sep in ["_iff_", "_equiv_"] {
        if let Some((l, r)) = name.split_once(sep) {
            if !l.is_empty() && !r.is_empty() {
                return Some((l, r));
            }
        }
    }
    None
}

/// `abelian_group` が `group` を含む、など修飾語付き名前を特殊化候補にする。
/// ステートメントが一致している必要はなく、信頼度は低く抑える。
///
/// 既知の制限（フェーズ3アーキテクチャレビュー問題2）: 命名済み判断すべての組を
/// 総当たりする O(n²) の部分文字列比較のため、大規模（100万+）では支配的なコストに
/// なりうる。レビューが推奨する修正は「新規判断だけを対象にした差分（Δ）計算」への
/// 切り替えであり、単純な文字列マッチのアルゴリズム置き換えではない（置き換えると
/// 中間一致パターン、例えば "free_group_action" のように修飾語が接頭辞・接尾辞の
/// どちらでもない場合を拾えなくなり、ヒューリスティックの再現率が黙って落ちる）。
/// そのためこの関数自体は変更せず、フェーズ3.2（インクリメンタル提案）の対象として残す。
fn propose_qualifier_names(judgments: &[JudgmentLite], push: &mut impl FnMut(MorphismProposal)) {
    const QUALIFIERS: &[&str] = &[
        "abelian",
        "commutative",
        "comm",
        "finite",
        "noetherian",
        "artinian",
        "complete",
        "compact",
        "linear",
        "free",
        "simple",
        "normal",
        "local",
        "graded",
        "connected",
        "hausdorff",
        "metric",
    ];

    let named: Vec<(&JudgmentLite, String)> = judgments
        .iter()
        .filter_map(|j| {
            j.name
                .as_ref()
                .map(|n| (j, n.to_ascii_lowercase().replace('-', "_")))
        })
        .collect();

    for (spec, spec_name) in &named {
        for (gen, gen_name) in &named {
            if spec.id == gen.id {
                continue;
            }
            if spec_name == gen_name {
                continue;
            }
            if !spec_name.contains(gen_name.as_str()) {
                continue;
            }
            let extra = spec_name.replace(gen_name.as_str(), "");
            let has_qualifier = QUALIFIERS.iter().any(|q| extra.contains(q));
            if !has_qualifier && extra.chars().any(|c| c.is_alphanumeric()) {
                // 単なる部分文字列（group ⊂ subgroup など）はノイズになりやすいので除外
                continue;
            }
            if !has_qualifier {
                continue;
            }
            push(MorphismProposal {
                src: gen.id,
                dst: spec.id,
                kind: MorphismKind::Specialization,
                rationale: format!(
                    "name {} looks like a qualified specialization of {}",
                    spec_name, gen_name
                ),
                confidence: 0.35,
            });
        }
    }
}

/// `bar_of_foo` / `foo_to_bar` パターン:
/// 数学では `ring_of_group` (群から環を作る) のように `_of_` や `_to_` を使う慣習がある。
/// `bar_of_foo` は「foo から bar を導く」= foo → bar (含意) の候補にする。
fn propose_of_pattern(judgments: &[JudgmentLite], push: &mut impl FnMut(MorphismProposal)) {
    let named: Vec<(&JudgmentLite, String)> = judgments
        .iter()
        .filter_map(|j| j.name.as_ref().map(|n| (j, n.to_ascii_lowercase())))
        .collect();

    let by_name: HashMap<&str, JudgmentId> = named
        .iter()
        .map(|(j, n)| (n.as_str(), j.id))
        .collect();

    for (j, name) in &named {
        // `bar_of_foo` → foo が先, bar が後 → 含意 foo → bar
        for sep in ["_of_", "_from_"] {
            if let Some((bar, foo)) = name.split_once(sep) {
                if bar.is_empty() || foo.is_empty() {
                    continue;
                }
                if let (Some(&src_id), Some(&dst_id)) = (by_name.get(foo), by_name.get(bar)) {
                    if src_id == j.id || dst_id == j.id || src_id == dst_id {
                        continue;
                    }
                    push(MorphismProposal {
                        src: src_id,
                        dst: dst_id,
                        kind: MorphismKind::Implication,
                        rationale: format!(
                            "name pattern '{}' suggests {} → {} (implication)",
                            name, foo, bar
                        ),
                        confidence: 0.40,
                    });
                }
            }
        }
        // `foo_to_bar` → 含意 foo → bar
        if let Some((foo, bar)) = name.split_once("_to_") {
            if !foo.is_empty() && !bar.is_empty() {
                if let (Some(&src_id), Some(&dst_id)) = (by_name.get(foo), by_name.get(bar)) {
                    if src_id != dst_id && src_id != j.id && dst_id != j.id {
                        push(MorphismProposal {
                            src: src_id,
                            dst: dst_id,
                            kind: MorphismKind::Implication,
                            rationale: format!(
                                "name pattern '{}' suggests {} → {} (implication)",
                                name, foo, bar
                            ),
                            confidence: 0.40,
                        });
                    }
                }
            }
        }
    }
}

/// `foo_implies_bar` パターン: 含意 foo → bar を提案する。信頼度高め。
fn propose_implies_pattern(judgments: &[JudgmentLite], push: &mut impl FnMut(MorphismProposal)) {
    let named: Vec<(&JudgmentLite, String)> = judgments
        .iter()
        .filter_map(|j| j.name.as_ref().map(|n| (j, n.to_ascii_lowercase())))
        .collect();

    let by_name: HashMap<&str, JudgmentId> = named
        .iter()
        .map(|(j, n)| (n.as_str(), j.id))
        .collect();

    for (_j, name) in &named {
        for sep in ["_implies_", "_entails_"] {
            if let Some((src_name, dst_name)) = name.split_once(sep) {
                if src_name.is_empty() || dst_name.is_empty() {
                    continue;
                }
                if let (Some(&src_id), Some(&dst_id)) =
                    (by_name.get(src_name), by_name.get(dst_name))
                {
                    if src_id == dst_id {
                        continue;
                    }
                    push(MorphismProposal {
                        src: src_id,
                        dst: dst_id,
                        kind: MorphismKind::Implication,
                        rationale: format!(
                            "name '{}' explicitly encodes {} → {}",
                            name, src_name, dst_name
                        ),
                        confidence: 0.55,
                    });
                }
            }
        }
    }
}

/// `foo_generalizes_bar` / `bar_specializes_foo` パターン:
/// 明示的な一般化・特殊化の関係を名前から読み取る。
fn propose_generalizes_pattern(
    judgments: &[JudgmentLite],
    push: &mut impl FnMut(MorphismProposal),
) {
    let named: Vec<(&JudgmentLite, String)> = judgments
        .iter()
        .filter_map(|j| j.name.as_ref().map(|n| (j, n.to_ascii_lowercase())))
        .collect();

    let by_name: HashMap<&str, JudgmentId> = named
        .iter()
        .map(|(j, n)| (n.as_str(), j.id))
        .collect();

    for (_j, name) in &named {
        // `foo_generalizes_bar` → foo は bar より一般的 → bar → foo (特殊化: bar は foo の特殊)
        if let Some((gen_name, spec_name)) = name.split_once("_generalizes_") {
            if !gen_name.is_empty() && !spec_name.is_empty() {
                if let (Some(&gen_id), Some(&spec_id)) =
                    (by_name.get(gen_name), by_name.get(spec_name))
                {
                    if gen_id != spec_id {
                        push(MorphismProposal {
                            src: gen_id,
                            dst: spec_id,
                            kind: MorphismKind::Specialization,
                            rationale: format!(
                                "name '{}' encodes {} generalizes {} (spec: {} → {})",
                                name, gen_name, spec_name, gen_name, spec_name
                            ),
                            confidence: 0.50,
                        });
                    }
                }
            }
        }
        // `bar_specializes_foo` → bar は foo の特殊 → Specialization: foo → bar
        if let Some((spec_name, gen_name)) = name.split_once("_specializes_") {
            if !spec_name.is_empty() && !gen_name.is_empty() {
                if let (Some(&gen_id), Some(&spec_id)) =
                    (by_name.get(gen_name), by_name.get(spec_name))
                {
                    if gen_id != spec_id {
                        push(MorphismProposal {
                            src: gen_id,
                            dst: spec_id,
                            kind: MorphismKind::Specialization,
                            rationale: format!(
                                "name '{}' encodes {} specializes {} (spec: {} → {})",
                                name, spec_name, gen_name, gen_name, spec_name
                            ),
                            confidence: 0.50,
                        });
                    }
                }
            }
        }
    }
}

/// `corollary_foo` / `cor_foo` プレフィックスパターン:
/// 系（Corollary）は通常、対応する定理（foo）からの含意として解釈できる。
fn propose_corollary_pattern(
    judgments: &[JudgmentLite],
    push: &mut impl FnMut(MorphismProposal),
) {
    let named: Vec<(&JudgmentLite, String)> = judgments
        .iter()
        .filter_map(|j| j.name.as_ref().map(|n| (j, n.to_ascii_lowercase())))
        .collect();

    let by_name: HashMap<&str, JudgmentId> = named
        .iter()
        .map(|(j, n)| (n.as_str(), j.id))
        .collect();

    for (cor_j, name) in &named {
        for prefix in ["corollary_", "cor_", "coroll_"] {
            if let Some(base) = name.strip_prefix(prefix) {
                if base.is_empty() {
                    continue;
                }
                if let Some(&base_id) = by_name.get(base) {
                    if base_id != cor_j.id {
                        push(MorphismProposal {
                            src: base_id,
                            dst: cor_j.id,
                            kind: MorphismKind::Implication,
                            rationale: format!(
                                "name '{}' looks like a corollary of '{}'",
                                name, base
                            ),
                            confidence: 0.30,
                        });
                    }
                }
            }
        }
    }
}

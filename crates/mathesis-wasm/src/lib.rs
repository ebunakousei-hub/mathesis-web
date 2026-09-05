//! ブラウザ向けカーネル（層1 + 簡易版層2）。
//!
//! `mathesis-graph` の `GraphStore` は永続化に rusqlite（バンドルされた C 製
//! SQLite）を使っており、C コンパイラ（clang）を要求するため
//! `wasm32-unknown-unknown` へは素直にコンパイルできない（実機で確認済み）。
//! ブラウザ上の1セッションはそもそも「開いている間だけ生きていればよい」
//! 探索用途なのでファイル永続化は本質的に不要と判断し、このクレートは
//! `mathesis-graph` を再利用せず、純粋な Rust データ構造（`Vec`）だけで
//! 動く最小限のインメモリ実装を独自に持つ。
//!
//! 式のパース・α正規化・正規化ハッシュ（層1）は `mathesis-ast` をそのまま
//! 使う——ここは C 依存が一切なく wasm32 で無条件に動く。
//!
//! 将来 `mathesis-graph` 側の SQLite 依存を Cargo feature で任意化すれば、
//! この簡易実装は同クレートの本実装（`GraphStore` の同値類縮約・射の合成
//! ルール等）に置き換えられる。現時点ではブラウザ側の骨格を先に通すことを
//! 優先した。

mod layer3;
mod lean;

use layer3::{
    compose, quotient_classes, representative_map, shortest_derivation, Morphism, MorphismKind,
    MorphismView, WELL_KNOWN_STRATEGIES,
};
use lean::parse_lean_to_view;
use mathesis_ast::{parse_expr, ParseStatus as AstParseStatus};
use serde::Serialize;
use std::collections::HashMap;
use wasm_bindgen::prelude::*;

/// wasm モジュールのロード時に自動実行される（`start` 属性）。パニック時の
/// スタックトレースをブラウザのコンソールへ出すためのフックを仕込むだけで、
/// TypeScript 側から明示的に呼ぶ必要はない。
#[wasm_bindgen(start)]
pub fn set_panic_hook() {
    console_error_panic_hook::set_once();
}

#[derive(Debug, Clone, Serialize)]
pub struct ParsedStatement {
    /// 正規化後の式を整形して表示したもの
    pub display: String,
    /// α同値な式なら常に一致する正規化ハッシュ（16進数）
    pub canonical_hash: String,
    /// "full" | "partial" | "failed"
    pub status: String,
}

fn parse_to_view(src: &str) -> ParsedStatement {
    match parse_expr(src) {
        Ok(outcome) => ParsedStatement {
            display: format!("{}", outcome.expr),
            canonical_hash: outcome.expr.canonical_hash_hex(),
            status: match outcome.status {
                AstParseStatus::Full => "full",
                AstParseStatus::Partial => "partial",
            }
            .to_string(),
        },
        Err(_) => ParsedStatement {
            display: src.to_string(),
            canonical_hash: String::new(),
            status: "failed".to_string(),
        },
    }
}

/// 式を1つパースして、正規化済み表示・正規化ハッシュ・パース状態を返す。
/// 検索バーの「入力しながらライブプレビュー」用の最小API。
#[wasm_bindgen]
pub fn parse_statement(src: &str) -> JsValue {
    serde_wasm_bindgen::to_value(&parse_to_view(src)).unwrap_or(JsValue::NULL)
}

/// 貼り付けたLeanソースをその場でパースし、判断ノード・依存関係の一覧を
/// 返す（`lean.rs`参照）。`mathesis-import` CLIが1回のインポートで
/// `judgments.json`に書き出すのと同じ形の情報を、ファイル保存もDBも
/// 経由せずブラウザだけで得られる。
#[wasm_bindgen(js_name = parseLeanSource)]
pub fn parse_lean_source_js(source: &str) -> JsValue {
    serde_wasm_bindgen::to_value(&parse_lean_to_view(source)).unwrap_or(JsValue::NULL)
}

#[derive(Debug, Clone, Serialize)]
pub struct JudgmentView {
    pub id: u32,
    pub kind: String,
    pub name: Option<String>,
    pub statement: String,
    pub canonical_hash: String,
    pub parse_status: String,
    pub context: Vec<String>,
    /// `web/src/fields.ts` の `FieldNode`/`BridgeNode` の `id` と対応する分野タグ。
    /// 上位分野と下位分野の両方を付けてよい（例: 群論の定理には
    /// `["algebra", "group-theory"]`）——エクスプローラー側は上位分野を見る
    /// ときに下位分野のタグも合算して数える。
    pub fields: Vec<String>,
}

/// 層2〜5の最小版: 判断ノード・射・戦略タグをブラウザのメモリ上に保持する
/// ストア。`mathesis-graph::GraphStore` と違い、SQLite永続化・Proposed/
/// Accepted/Rejectedの3状態・インクリメンタルキャッシュは持たない——射は
/// 常に人間が明示的に追加したAccepted相当のものとして扱う（`layer3`モジュール
/// のドキュメント参照）。
#[wasm_bindgen]
pub struct KernelStore {
    judgments: Vec<JudgmentView>,
    next_id: u32,
    morphisms: Vec<Morphism>,
    next_morphism_id: u32,
    strategy_tags: HashMap<u32, Vec<String>>,
}

impl Default for KernelStore {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl KernelStore {
    #[wasm_bindgen(constructor)]
    pub fn new() -> KernelStore {
        KernelStore {
            judgments: Vec::new(),
            next_id: 1,
            morphisms: Vec::new(),
            next_morphism_id: 1,
            strategy_tags: HashMap::new(),
        }
    }

    /// 判断ノードを1件追加する。`context` は `"a : G"` のような文字列の配列
    /// （表示用。まだ型として構造化はしていない）。`fields` は
    /// `web/src/fields.ts` の分野/学際領域の `id` の配列。
    #[wasm_bindgen(js_name = addJudgment)]
    pub fn add_judgment(
        &mut self,
        kind: &str,
        name: Option<String>,
        statement_src: &str,
        context: Vec<String>,
        fields: Vec<String>,
    ) -> u32 {
        let parsed = parse_to_view(statement_src);
        let id = self.next_id;
        self.next_id += 1;
        self.judgments.push(JudgmentView {
            id,
            kind: kind.to_string(),
            name,
            statement: parsed.display,
            canonical_hash: parsed.canonical_hash,
            parse_status: parsed.status,
            context,
            fields,
        });
        id
    }

    #[wasm_bindgen(js_name = listJudgments)]
    pub fn list_judgments(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.judgments).unwrap_or(JsValue::NULL)
    }

    pub fn len(&self) -> usize {
        self.judgments.len()
    }

    #[wasm_bindgen(js_name = isEmpty)]
    pub fn is_empty(&self) -> bool {
        self.judgments.is_empty()
    }

    /// アーキテクチャ文書の例（「ペアノ公理下での加法の結合律」と
    /// 「群論下での結合律」は文脈が違うので別ノード）をそのまま投入する、
    /// UIの動作確認用シードデータ。
    #[wasm_bindgen(js_name = seedDemoJudgments)]
    pub fn seed_demo_judgments(&mut self) {
        self.add_judgment(
            "theorem",
            Some("add_assoc".into()),
            "a + b + c = a + (b + c)",
            vec!["a : ℕ".into(), "b : ℕ".into(), "c : ℕ".into()],
            vec!["algebra".into()],
        );
        self.add_judgment(
            "theorem",
            Some("mul_assoc".into()),
            "a * b * c = a * (b * c)",
            vec!["a : G".into(), "b : G".into(), "c : G".into(), "inst : Group G".into()],
            vec!["algebra".into(), "group-theory".into()],
        );
        self.add_judgment(
            "theorem",
            Some("add_comm".into()),
            "a + b = b + a",
            vec!["a : ℕ".into(), "b : ℕ".into()],
            vec!["algebra".into()],
        );
        let group_hom = self.add_judgment(
            "definition",
            Some("group_hom".into()),
            "∀ x y, f (x * y) = f x * f y",
            vec!["f : G → H".into()],
            vec!["algebra".into(), "group-theory".into()],
        );
        let abelian_group_hom = self.add_judgment(
            "definition",
            Some("abelian_group_hom".into()),
            "∀ x y, f (x * y) = f x * f y",
            vec![
                "f : G → H".into(),
                "instG : AbelianGroup G".into(),
                "instH : AbelianGroup H".into(),
            ],
            vec!["algebra".into(), "group-theory".into()],
        );
        // group_hom と abelian_group_hom は同一のステートメント（正規化ハッシュも
        // 一致する）で、コンテキストだけが真に拡張されている——
        // `mathesis-graph::heuristics::propose_statement_structure` が特殊化
        // として自動提案する規則そのものを、ここでは人間の注釈として先に投入する。
        let _ = self.add_morphism_inner(
            group_hom,
            abelian_group_hom,
            "specialization",
            Some(
                "同じ準同型条件だが、G・Hを共にアーベル群と仮定した特殊環境（ヒューリスティックのルール1と同型のパターン）"
                    .into(),
            ),
        );
    }

    /// 層3: 2つの判断ノード間に射を1本張る（人間による直接注釈、常にAccepted
    /// 相当）。`mathesis-graph::GraphStore::annotate` のブラウザ簡易版。
    /// 同じ (src, dst, kind) の射が既にあれば新規作成せずその id を返す。
    #[wasm_bindgen(js_name = addMorphism)]
    pub fn add_morphism(
        &mut self,
        src: u32,
        dst: u32,
        kind: &str,
        rationale: Option<String>,
    ) -> Result<u32, JsValue> {
        self.add_morphism_inner(src, dst, kind, rationale)
            .map_err(|e| JsValue::from_str(&e))
    }

    #[wasm_bindgen(js_name = listMorphisms)]
    pub fn list_morphisms(&self) -> JsValue {
        let views: Vec<MorphismView> = self
            .morphisms
            .iter()
            .map(|m| MorphismView {
                id: m.id,
                src: m.src,
                dst: m.dst,
                kind: m.kind.as_str().to_string(),
                rationale: m.rationale.clone(),
                strategies: self.strategy_tags.get(&m.id).cloned().unwrap_or_default(),
            })
            .collect();
        serde_wasm_bindgen::to_value(&views).unwrap_or(JsValue::NULL)
    }

    /// 層4: 射に証明戦略タグを付与する。既に同じタグが付いていれば何もしない
    /// （`mathesis-graph::GraphStore::tag_morphism_strategy` と同じく冪等）。
    #[wasm_bindgen(js_name = tagStrategy)]
    pub fn tag_strategy(&mut self, morphism_id: u32, name: &str) -> Result<(), JsValue> {
        self.tag_strategy_inner(morphism_id, name).map_err(|e| JsValue::from_str(&e))
    }

    #[wasm_bindgen(js_name = wellKnownStrategies)]
    pub fn well_known_strategies() -> Vec<String> {
        WELL_KNOWN_STRATEGIES.iter().map(|s| s.to_string()).collect()
    }

    /// 層3の縮約: 受理済み同値射から同値類を計算する。同値エッジに一度も
    /// 関わっていない判断ノードは（自明な単集合として）含めない。
    #[wasm_bindgen(js_name = quotientClasses)]
    pub fn quotient_classes_js(&self) -> JsValue {
        let representative = representative_map(&self.morphisms);
        let classes = quotient_classes(&representative);
        serde_wasm_bindgen::to_value(&classes).unwrap_or(JsValue::NULL)
    }

    /// 層5: `from` から `to` への、単一の導出関係へ還元できる最短パスを探す。
    /// 見つからなければ `null`。
    #[wasm_bindgen(js_name = shortestDerivation)]
    pub fn shortest_derivation_js(&self, from: u32, to: u32) -> JsValue {
        match shortest_derivation(&self.morphisms, from, to) {
            Some(path) => serde_wasm_bindgen::to_value(&path).unwrap_or(JsValue::NULL),
            None => JsValue::NULL,
        }
    }

    /// 層5のエッジ合成則をそのまま呼べる小さな窓口（教育目的の表示用）。
    /// 合成できなければ `null`。
    #[wasm_bindgen(js_name = composeMorphismKinds)]
    pub fn compose_morphism_kinds(first: &str, second: &str) -> Option<String> {
        let first = MorphismKind::from_str(first)?;
        let second = MorphismKind::from_str(second)?;
        compose(first, second).map(|k| k.as_str().to_string())
    }
}

/// `wasm_bindgen` を経由しない素のロジック。`JsValue` はネイティブターゲット
/// （`cargo test` 実行環境）では実質的に使えない（構築しただけでabortする）
/// ため、検証ロジック自体はここに切り出し、上の `#[wasm_bindgen]` メソッドは
/// `String` エラーを `JsValue` へ変換するだけの薄いラッパーにしてある。
impl KernelStore {
    fn add_morphism_inner(
        &mut self,
        src: u32,
        dst: u32,
        kind: &str,
        rationale: Option<String>,
    ) -> Result<u32, String> {
        let kind = MorphismKind::from_str(kind).ok_or_else(|| format!("unknown morphism kind: {kind}"))?;
        if src == dst {
            return Err("a morphism cannot connect a judgment to itself".to_string());
        }
        if !self.judgments.iter().any(|j| j.id == src) {
            return Err(format!("no judgment with id {src}"));
        }
        if !self.judgments.iter().any(|j| j.id == dst) {
            return Err(format!("no judgment with id {dst}"));
        }

        // 同値は無向なので、正規化のため id の小さい方を src にする。
        let (src, dst) = if kind == MorphismKind::Equivalence && src > dst {
            (dst, src)
        } else {
            (src, dst)
        };

        if let Some(existing) = self
            .morphisms
            .iter()
            .find(|m| m.src == src && m.dst == dst && m.kind == kind)
        {
            return Ok(existing.id);
        }

        let id = self.next_morphism_id;
        self.next_morphism_id += 1;
        self.morphisms.push(Morphism { id, src, dst, kind, rationale });
        Ok(id)
    }

    /// 層4: 射に証明戦略タグを付与する。既に同じタグが付いていれば何もしない
    /// （`mathesis-graph::GraphStore::tag_morphism_strategy` と同じく冪等）。
    fn tag_strategy_inner(&mut self, morphism_id: u32, name: &str) -> Result<(), String> {
        if !self.morphisms.iter().any(|m| m.id == morphism_id) {
            return Err(format!("no morphism with id {morphism_id}"));
        }
        let name = name.trim();
        if name.is_empty() {
            return Err("strategy name must not be empty".to_string());
        }
        let tags = self.strategy_tags.entry(morphism_id).or_default();
        if !tags.iter().any(|t| t == name) {
            tags.push(name.to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_to_view_reports_full_status_and_hash() {
        let v = parse_to_view("a + b = b + a");
        assert_eq!(v.status, "full");
        assert!(!v.canonical_hash.is_empty());
        assert_eq!(v.display, "((a + b) = (b + a))");
    }

    #[test]
    fn store_keeps_same_shape_different_context_as_distinct_nodes() {
        // アーキテクチャ文書の例そのもの: ペアノ公理下の加法の結合律と
        // 群論下の結合律は、文脈が違うので別ノードのまま残るべき。
        let mut store = KernelStore::new();
        let peano = store.add_judgment(
            "theorem",
            Some("add_assoc".into()),
            "a + b + c = a + (b + c)",
            vec!["a : ℕ".into()],
            vec!["algebra".into()],
        );
        let group = store.add_judgment(
            "theorem",
            Some("mul_assoc".into()),
            "a * b * c = a * (b * c)",
            vec!["a : G".into(), "inst : Group G".into()],
            vec!["algebra".into(), "group-theory".into()],
        );
        assert_ne!(peano, group);
        assert_eq!(store.len(), 2);
        assert!(!store.is_empty());
        assert_ne!(
            store.judgments[0].canonical_hash, store.judgments[1].canonical_hash,
            "同じ形（結合律）でも別々の式ノードとして正規化ハッシュが一致してはいけない"
        );
    }

    #[test]
    fn seed_demo_judgments_populates_five_entries_and_one_specialization_morphism() {
        let mut store = KernelStore::new();
        store.seed_demo_judgments();
        assert_eq!(store.len(), 5);
        assert_eq!(store.morphisms.len(), 1);
        assert_eq!(store.morphisms[0].kind, MorphismKind::Specialization);
    }

    #[test]
    fn add_morphism_rejects_self_loop_and_unknown_endpoints() {
        let mut store = KernelStore::new();
        let a = store.add_judgment("theorem", None, "a = a", vec![], vec![]);
        assert!(store.add_morphism_inner(a, a, "implication", None).is_err());
        assert!(store.add_morphism_inner(a, 999, "implication", None).is_err());
    }

    #[test]
    fn add_morphism_is_idempotent_for_the_same_src_dst_kind() {
        let mut store = KernelStore::new();
        let a = store.add_judgment("theorem", None, "a = a", vec![], vec![]);
        let b = store.add_judgment("theorem", None, "b = b", vec![], vec![]);
        let first = store.add_morphism_inner(a, b, "implication", None).unwrap();
        let second = store
            .add_morphism_inner(a, b, "implication", Some("different rationale".into()))
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(store.morphisms.len(), 1);
    }

    #[test]
    fn tag_strategy_is_idempotent_and_rejects_unknown_morphism() {
        let mut store = KernelStore::new();
        let a = store.add_judgment("theorem", None, "a = a", vec![], vec![]);
        let b = store.add_judgment("theorem", None, "b = b", vec![], vec![]);
        let m = store.add_morphism_inner(a, b, "implication", None).unwrap();
        store.tag_strategy_inner(m, "induction").unwrap();
        store.tag_strategy_inner(m, "induction").unwrap();
        assert_eq!(store.strategy_tags.get(&m).unwrap().len(), 1);
        assert!(store.tag_strategy_inner(999, "induction").is_err());
    }

    #[test]
    fn shortest_derivation_js_finds_the_seeded_specialization() {
        let mut store = KernelStore::new();
        store.seed_demo_judgments();
        // group_hom は id 4、abelian_group_hom は id 5（1始まりの採番）。
        let path = shortest_derivation(&store.morphisms, 4, 5);
        assert!(path.is_some());
    }
}

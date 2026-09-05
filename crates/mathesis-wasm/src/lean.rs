//! Leanのソースをブラウザ内でその場でパースする（`mathesis-lean-parse`の
//! 薄いラッパー）。
//!
//! これまで「証明が何に依拠するかを追える」体験（`web/src/lineage.ts` /
//! `lineageView.ts`）は、CLIで事前にインポート・書き出した静的な
//! `judgments.json`（1件の実データ、DeGiorgiコーパス）でしか見られなかった。
//! `README.md`の「今後の課題」に書いていた既知のギャップ——
//! 「`mathesis-importer`のLeanパーサーは今のところCLIバイナリ内に閉じて
//! いる。ブラウザから直接Leanソースを貼り付けてインポートできるように
//! するには、パーサーを再利用可能なライブラリへ切り出す必要がある」——
//! を埋める。`mathesis-lean-parse`（`mathesis-graph`に依存しない純粋な
//! パーサー、`mathesis-importer`のCLIとここの両方が使う単一の実装）を
//! そのまま呼ぶだけで、貼り付けたLeanから判断ノードと依存関係をその場で
//! 抽出できる。
//!
//! CLIの`mathesis-importer`と違い、ここでは:
//!   - `mathesis-graph::GraphStore`を経由しない（wasm32でC依存を持ち込め
//!     ないため——`lib.rs`冒頭のコメント参照）。式・文脈のインターン
//!     （重複排除してIDを振る）は行わず、貼り付けた分だけをその場で
//!     配列の添字でID付けする。1回の貼り付けはせいぜい数百行程度なので、
//!     重複排除を省いても実用上問題にならない。
//!   - 依存関係は**この貼り付け内で閉じる**（CLI版の
//!     `judgment_dependencies`と同じ制約——1回のインポートバッチ内の名前
//!     解決に限る、既知の制約をここでも踏襲）。

use mathesis_lean_parse::{find_dependencies, parse_lean_source};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeanJudgmentView {
    /// この貼り付け内だけで意味を持つ、1始まりの通し番号。
    pub id: u32,
    pub kind: String,
    pub name: Option<String>,
    pub statement: String,
    pub context: Vec<String>,
    pub parse_status: String,
    pub source_line: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct LeanDependencyView {
    pub from: u32,
    pub to: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeanParseView {
    pub judgments: Vec<LeanJudgmentView>,
    pub dependencies: Vec<LeanDependencyView>,
}

fn context_line(name: &str, ty: &mathesis_ast::Expr) -> String {
    format!("{name} : {ty}")
}

/// 貼り付けられたLeanソースを判断ノード・依存関係の一覧へパースする。
/// `Result`を返さない——構文的に何一つ宣言が見つからなくても、
/// `judgments: []` という「空だった」という結果自体が利用者への回答に
/// なる（1個の壊れた宣言のせいで全体を諦めない、CLI版と同じ姿勢）。
pub fn parse_lean_to_view(source: &str) -> LeanParseView {
    let judgments = parse_lean_source(source);
    let edges = find_dependencies(&judgments);

    let views = judgments
        .iter()
        .enumerate()
        .map(|(i, j)| LeanJudgmentView {
            id: (i + 1) as u32,
            kind: j.kind.as_str().to_string(),
            name: j.name.clone(),
            statement: format!("{}", j.statement_expr),
            context: j.context.iter().map(|(n, t)| context_line(n, t)).collect(),
            parse_status: j.statement_status.as_str().to_string(),
            source_line: j.line,
        })
        .collect();

    let dependencies = edges
        .into_iter()
        .map(|(from, to)| LeanDependencyView { from: (from + 1) as u32, to: (to + 1) as u32 })
        .collect();

    LeanParseView { judgments: views, dependencies }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lean_to_view_extracts_judgments_and_resolves_dependencies_within_the_snippet() {
        let source = r"
theorem base_lemma : True := trivial

theorem derived_thm : True := by exact base_lemma
";
        let view = parse_lean_to_view(source);
        assert_eq!(view.judgments.len(), 2);
        assert_eq!(view.judgments[0].name.as_deref(), Some("base_lemma"));
        assert_eq!(view.judgments[0].id, 1);
        assert_eq!(view.judgments[1].name.as_deref(), Some("derived_thm"));
        assert_eq!(view.judgments[1].id, 2);
        assert_eq!(view.dependencies.len(), 1);
        assert_eq!(view.dependencies[0].from, 2);
        assert_eq!(view.dependencies[0].to, 1);
    }

    #[test]
    fn parse_lean_to_view_returns_an_empty_result_for_source_with_no_declarations() {
        let view = parse_lean_to_view("-- just a comment, nothing to import\n");
        assert!(view.judgments.is_empty());
        assert!(view.dependencies.is_empty());
    }

    #[test]
    fn parse_lean_to_view_formats_context_the_same_way_as_the_cli_export() {
        // `crates/mathesis-graph/src/export.rs::context_line` と同じ
        // "名前 : 型" 形式で揃える——CLIで取り込んだ静的データも、ここで
        // その場でパースしたデータも、Web側は同じ `"name : type"` 文字列
        // を前提に読んでいるため。
        let source = "theorem foo (n : ℕ) : (0 ≤ n) := by positivity";
        let view = parse_lean_to_view(source);
        assert_eq!(view.judgments[0].context, vec!["n : ℕ".to_string()]);
    }
}

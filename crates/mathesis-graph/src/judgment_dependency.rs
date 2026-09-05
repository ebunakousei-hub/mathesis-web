//! Phase 9: 判断ノード間の「証明が参照している」依存関係。
//!
//! `morphisms`（含意・特殊化・一般化・同値の4種のみ、層3のロジカルな関係を
//! 表す厳密な型付きエッジ）とは意味が異なるため、既存の`MorphismKind`には
//! 混ぜない——「Aの証明はBという既存の補題を使っている」は論理的な関係
//! （AとBの命題としての関係）ではなく、証明テキストが持つ参照構造であり、
//! `crates/mathesis-fulltext`のPhase 8で作った`theorem_dependencies`
//! （arXiv論文内の証明が`\ref`で参照する他の定理）とまったく同じ形の関係を
//! Lean側に持たせたもの。同一の由来（`mathesis-importer`が1回のインポートで
//! 読んだ判断ノード名の集合）の中でしか解決しない——他ファイル・他論文の
//! 名前を勝手に結びつけることはしない、既知の制約。

use crate::model::JudgmentId;
use crate::store::{GraphStore, Result};
use rusqlite::params;

impl GraphStore {
    /// `from`の証明（または定義本体）が`to`を参照していることを記録する。
    /// 同じ組を重ねて呼んでも冪等。
    pub fn record_judgment_dependency(&self, from: JudgmentId, to: JudgmentId) -> Result<()> {
        self.conn
            .prepare_cached(
                "INSERT OR IGNORE INTO judgment_dependencies (from_judgment, to_judgment)
                 VALUES (?1, ?2)",
            )?
            .execute(params![from.0, to.0])?;
        Ok(())
    }

    /// `judgment`の証明が参照している判断ノード（＝`judgment`が依存している側）。
    pub fn dependencies_of(&self, judgment: JudgmentId) -> Result<Vec<JudgmentId>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT to_judgment FROM judgment_dependencies WHERE from_judgment = ?1 ORDER BY to_judgment",
        )?;
        let rows = stmt
            .query_map(params![judgment.0], |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows.into_iter().map(JudgmentId).collect())
    }

    /// `judgment`を参照している判断ノード（＝逆方向、"これに依存している証明はどれか"）。
    pub fn dependents_of(&self, judgment: JudgmentId) -> Result<Vec<JudgmentId>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT from_judgment FROM judgment_dependencies WHERE to_judgment = ?1 ORDER BY from_judgment",
        )?;
        let rows = stmt
            .query_map(params![judgment.0], |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows.into_iter().map(JudgmentId).collect())
    }

    pub fn judgment_dependency_count(&self) -> Result<i64> {
        self.conn.query_row("SELECT COUNT(*) FROM judgment_dependencies", [], |r| r.get(0))
    }
}

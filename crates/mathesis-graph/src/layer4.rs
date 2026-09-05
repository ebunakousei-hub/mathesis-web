//! 層4: 戦略・メタ層（Strategy/Meta Layer）の GraphStore 操作。
//!
//! 層3の射（証明）に「どの戦略で導かれたか」をタグ付けし、失敗した試行を
//! FailedAttempt ノードとして蓄積する。層3のヒューリスティックが自動承認を
//! 避けたのと同じ思想で、ここでも「負の知識」は既存レコードを書き換えず
//! 追記のみで育てる。

use crate::error::{GraphResult, ValidationError};
use crate::failed_attempt::{
    FailedAttemptId, FailedAttemptRecord, FailurePattern, NewFailedAttempt,
};
use crate::model::JudgmentId;
use crate::morphism::{EdgeStatus, MorphismId, MorphismRecord};
use crate::store::{GraphStore, Result};
use crate::strategy::{StrategyId, StrategyRecord};
use rusqlite::{params, OptionalExtension};

impl GraphStore {
    // ---- 戦略ノード -------------------------------------------------------

    /// 名前で戦略ノードをインターンする（既存なら再利用、"induction" を何度
    /// 呼んでも同じ `StrategyId` が返る）。
    pub fn intern_strategy(&self, name: &str, description: Option<&str>) -> Result<StrategyId> {
        if let Some(id) = self
            .conn
            .prepare_cached("SELECT id FROM strategies WHERE name = ?1")?
            .query_row(params![name], |r| r.get::<_, i64>(0))
            .optional()?
        {
            return Ok(StrategyId(id));
        }
        self.conn
            .prepare_cached("INSERT INTO strategies (name, description) VALUES (?1, ?2)")?
            .execute(params![name, description])?;
        Ok(StrategyId(self.conn.last_insert_rowid()))
    }

    pub(crate) fn strategy_exists(&self, id: StrategyId) -> Result<bool> {
        let n: i64 = self
            .conn
            .prepare_cached("SELECT COUNT(*) FROM strategies WHERE id = ?1")?
            .query_row(params![id.0], |r| r.get(0))?;
        Ok(n > 0)
    }

    pub fn get_strategy(&self, id: StrategyId) -> GraphResult<StrategyRecord> {
        self.conn
            .prepare_cached("SELECT id, name, description FROM strategies WHERE id = ?1")?
            .query_row(params![id.0], Self::strategy_row)
            .optional()?
            .ok_or(ValidationError::StrategyNotFound(id))
            .map_err(Into::into)
    }

    pub fn find_strategy_by_name(&self, name: &str) -> Result<Option<StrategyRecord>> {
        self.conn
            .prepare_cached("SELECT id, name, description FROM strategies WHERE name = ?1")?
            .query_row(params![name], Self::strategy_row)
            .optional()
    }

    pub fn list_strategies(&self) -> Result<Vec<StrategyRecord>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT id, name, description FROM strategies ORDER BY id")?;
        let rows = stmt
            .query_map([], Self::strategy_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    fn strategy_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StrategyRecord> {
        Ok(StrategyRecord {
            id: StrategyId(row.get(0)?),
            name: row.get(1)?,
            description: row.get(2)?,
        })
    }

    // ---- 射（証明）への戦略タグ付け ----------------------------------------

    /// 射に戦略タグを付与する。同じ組を重ねて呼んでも冪等。
    pub fn tag_morphism_strategy(
        &self,
        morphism_id: MorphismId,
        strategy_id: StrategyId,
    ) -> GraphResult<()> {
        if !self.morphism_exists(morphism_id)? {
            return Err(ValidationError::MissingMorphism(morphism_id).into());
        }
        if !self.strategy_exists(strategy_id)? {
            return Err(ValidationError::StrategyNotFound(strategy_id).into());
        }
        self.conn
            .prepare_cached(
                "INSERT OR IGNORE INTO morphism_strategies (morphism_id, strategy_id)
                 VALUES (?1, ?2)",
            )?
            .execute(params![morphism_id.0, strategy_id.0])?;
        Ok(())
    }

    pub fn strategies_of_morphism(&self, morphism_id: MorphismId) -> Result<Vec<StrategyRecord>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT s.id, s.name, s.description
             FROM strategies s
             JOIN morphism_strategies ms ON ms.strategy_id = s.id
             WHERE ms.morphism_id = ?1
             ORDER BY s.id",
        )?;
        let rows = stmt
            .query_map(params![morphism_id.0], Self::strategy_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// 「この戦略で証明されている定理を探す」クエリ（アーキテクチャ文書の例:
    /// 「帰納法で証明されている定理を探す」を実現する）。
    pub fn morphisms_by_strategy(
        &self,
        strategy_id: StrategyId,
        status: Option<EdgeStatus>,
    ) -> Result<Vec<MorphismRecord>> {
        let mut sql = String::from(
            "SELECT m.id, m.src, m.dst, m.kind, m.origin, m.status, m.rationale,
                    m.proof_term_hash, m.dependency_signature, m.created_at
             FROM morphisms m
             JOIN morphism_strategies ms ON ms.morphism_id = m.id
             WHERE ms.strategy_id = ?1",
        );
        if status.is_some() {
            sql.push_str(" AND m.status = ?2");
        }
        sql.push_str(" ORDER BY m.id");
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = match status {
            Some(s) => stmt
                .query_map(params![strategy_id.0, s.as_str()], Self::morphism_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?,
            None => stmt
                .query_map(params![strategy_id.0], Self::morphism_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?,
        };
        Ok(rows)
    }

    // ---- 失敗試行（FailedPath / FalseTheorem 相当） ------------------------

    /// 失敗した証明試行を記録する。既存の判断・成功証明を書き換えることはなく
    /// 追記のみ。同じ手詰まりが複数回記録されること自体が「よくある失敗
    /// パターン」のシグナルになる。`target` を指定した場合はその判断ノードが
    /// 実在することを検証する。
    pub fn record_failed_attempt(&self, new: &NewFailedAttempt) -> GraphResult<FailedAttemptId> {
        if let Some(target) = new.target {
            if !self.judgment_exists(target)? {
                return Err(ValidationError::MissingTargetJudgment(target).into());
            }
        }
        self.conn
            .prepare_cached(
                "INSERT INTO failed_attempts
                    (target_judgment, goal_text, pattern, detail, proof_term_hash,
                     source_file, source_line, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?
            .execute(params![
                new.target.map(|t| t.0),
                new.goal_text,
                new.pattern.as_str(),
                new.detail,
                new.proof_term_hash,
                new.source_file,
                new.source_line,
                Self::now_unix(),
            ])?;
        Ok(FailedAttemptId(self.conn.last_insert_rowid()))
    }

    pub fn tag_failed_attempt_strategy(
        &self,
        failed_attempt_id: FailedAttemptId,
        strategy_id: StrategyId,
    ) -> GraphResult<()> {
        if !self.failed_attempt_exists(failed_attempt_id)? {
            return Err(ValidationError::FailedAttemptNotFound(failed_attempt_id).into());
        }
        if !self.strategy_exists(strategy_id)? {
            return Err(ValidationError::StrategyNotFound(strategy_id).into());
        }
        self.conn
            .prepare_cached(
                "INSERT OR IGNORE INTO failed_attempt_strategies (failed_attempt_id, strategy_id)
                 VALUES (?1, ?2)",
            )?
            .execute(params![failed_attempt_id.0, strategy_id.0])?;
        Ok(())
    }

    fn failed_attempt_exists(&self, id: FailedAttemptId) -> Result<bool> {
        let n: i64 = self
            .conn
            .prepare_cached("SELECT COUNT(*) FROM failed_attempts WHERE id = ?1")?
            .query_row(params![id.0], |r| r.get(0))?;
        Ok(n > 0)
    }

    pub fn get_failed_attempt(&self, id: FailedAttemptId) -> GraphResult<FailedAttemptRecord> {
        self.conn
            .prepare_cached(
                "SELECT id, target_judgment, goal_text, pattern, detail, proof_term_hash,
                        source_file, source_line, created_at
                 FROM failed_attempts WHERE id = ?1",
            )?
            .query_row(params![id.0], Self::failed_attempt_row)
            .optional()?
            .ok_or(ValidationError::FailedAttemptNotFound(id))
            .map_err(Into::into)
    }

    pub fn strategies_of_failed_attempt(
        &self,
        id: FailedAttemptId,
    ) -> Result<Vec<StrategyRecord>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT s.id, s.name, s.description
             FROM strategies s
             JOIN failed_attempt_strategies fas ON fas.strategy_id = s.id
             WHERE fas.failed_attempt_id = ?1
             ORDER BY s.id",
        )?;
        let rows = stmt
            .query_map(params![id.0], Self::strategy_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// 特定の目標（判断ノード）に対する失敗試行の履歴。証明探索がここを
    /// 引いてから次の一手を選べば、同じ手詰まりを繰り返さずに刈り込める。
    pub fn failed_attempts_for_target(
        &self,
        target: JudgmentId,
    ) -> Result<Vec<FailedAttemptRecord>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, target_judgment, goal_text, pattern, detail, proof_term_hash,
                    source_file, source_line, created_at
             FROM failed_attempts WHERE target_judgment = ?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map(params![target.0], Self::failed_attempt_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn failed_attempts_by_pattern(
        &self,
        pattern: FailurePattern,
    ) -> Result<Vec<FailedAttemptRecord>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, target_judgment, goal_text, pattern, detail, proof_term_hash,
                    source_file, source_line, created_at
             FROM failed_attempts WHERE pattern = ?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map(params![pattern.as_str()], Self::failed_attempt_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn list_failed_attempts(&self) -> Result<Vec<FailedAttemptRecord>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, target_judgment, goal_text, pattern, detail, proof_term_hash,
                    source_file, source_line, created_at
             FROM failed_attempts ORDER BY id",
        )?;
        let rows = stmt
            .query_map([], Self::failed_attempt_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// `target` が反例により偽と判明している（FalseTheorem 相当）かどうか。
    /// 証明探索の入口でここを確認すれば、反証済みの予想に無駄な計算資源を
    /// 割かずに済む。
    pub fn is_refuted(&self, target: JudgmentId) -> Result<bool> {
        let n: i64 = self
            .conn
            .prepare_cached(
                "SELECT COUNT(*) FROM failed_attempts WHERE target_judgment = ?1 AND pattern = ?2",
            )?
            .query_row(
                params![target.0, FailurePattern::Counterexample.as_str()],
                |r| r.get(0),
            )?;
        Ok(n > 0)
    }

    fn failed_attempt_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<FailedAttemptRecord> {
        Ok(FailedAttemptRecord {
            id: FailedAttemptId(row.get(0)?),
            target: row.get::<_, Option<i64>>(1)?.map(JudgmentId),
            goal_text: row.get(2)?,
            pattern: FailurePattern::from_str(&row.get::<_, String>(3)?)
                .expect("saved pattern is always known"),
            detail: row.get(4)?,
            proof_term_hash: row.get(5)?,
            source_file: row.get(6)?,
            source_line: row.get(7)?,
            created_at: row.get(8)?,
        })
    }
}

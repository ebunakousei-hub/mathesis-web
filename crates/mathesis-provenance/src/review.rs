use crate::model::{AssertionId, NewReviewDecision, ReviewDecision, ReviewId, ReviewOutcome};
use crate::store::{ProvenanceStore, Result};
use rusqlite::params;

impl ProvenanceStore {
    pub fn insert_review_decision(&self, new: &NewReviewDecision) -> Result<ReviewId> {
        self.conn
            .prepare_cached(
                "INSERT INTO review_decisions
                    (assertion_id, decision, reviewer_id, scope, rationale, decided_at_unix, dataset_version)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?
            .execute(params![
                new.assertion_id.0,
                new.decision.as_str(),
                new.reviewer_id,
                new.scope,
                new.rationale,
                new.decided_at_unix,
                new.dataset_version,
            ])?;
        Ok(ReviewId(self.conn.last_insert_rowid()))
    }

    pub fn review_decisions_for(&self, assertion: AssertionId) -> Result<Vec<ReviewDecision>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, assertion_id, decision, reviewer_id, scope, rationale, decided_at_unix, dataset_version
             FROM review_decisions WHERE assertion_id = ?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map(params![assertion.0], Self::review_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn review_decision_count(&self) -> Result<i64> {
        self.conn.query_row("SELECT COUNT(*) FROM review_decisions", [], |r| r.get(0))
    }

    fn review_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReviewDecision> {
        let decision_str: String = row.get(2)?;
        Ok(ReviewDecision {
            id: ReviewId(row.get(0)?),
            assertion_id: AssertionId(row.get(1)?),
            decision: ReviewOutcome::from_str(&decision_str).expect("保存済み decision は常に既知の値"),
            reviewer_id: row.get(3)?,
            scope: row.get(4)?,
            rationale: row.get(5)?,
            decided_at_unix: row.get(6)?,
            dataset_version: row.get(7)?,
        })
    }
}

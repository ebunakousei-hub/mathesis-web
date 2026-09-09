use crate::model::{AssertionId, Evidence, EvidenceId, EvidenceKind, NewEvidence, SourceRecordId};
use crate::store::{ProvenanceStore, Result};
use rusqlite::params;

impl ProvenanceStore {
    pub fn insert_evidence(&self, new: &NewEvidence) -> Result<EvidenceId> {
        self.conn
            .prepare_cached(
                "INSERT INTO evidence
                    (assertion_id, source_record_id, locator, evidence_kind, extractor_or_model,
                     version, input_hash, output_hash, metric_name, metric_value, dependency_origin,
                     external_classification)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            )?
            .execute(params![
                new.assertion_id.0,
                new.source_record_id.0,
                new.locator,
                new.evidence_kind.as_str(),
                new.extractor_or_model,
                new.version,
                new.input_hash,
                new.output_hash,
                new.metric_name,
                new.metric_value,
                new.dependency_origin,
                new.external_classification,
            ])?;
        Ok(EvidenceId(self.conn.last_insert_rowid()))
    }

    pub fn evidence_for(&self, assertion: AssertionId) -> Result<Vec<Evidence>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, assertion_id, source_record_id, locator, evidence_kind, extractor_or_model,
                    version, input_hash, output_hash, metric_name, metric_value, dependency_origin,
                    external_classification
             FROM evidence WHERE assertion_id = ?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map(params![assertion.0], Self::evidence_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn evidence_count(&self) -> Result<i64> {
        self.conn.query_row("SELECT COUNT(*) FROM evidence", [], |r| r.get(0))
    }

    /// assertionあたりのevidence行数のヒストグラム（`stats`サブコマンド用、
    /// 「ConfirmedはGroundedの2倍の証拠を持つはず」の検証に使う）。
    pub fn evidence_count_histogram(&self) -> Result<Vec<(i64, i64)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT n, COUNT(*) FROM (
                SELECT assertion_id, COUNT(*) AS n FROM evidence GROUP BY assertion_id
             ) GROUP BY n ORDER BY n",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    fn evidence_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Evidence> {
        let kind_str: String = row.get(4)?;
        Ok(Evidence {
            id: EvidenceId(row.get(0)?),
            assertion_id: AssertionId(row.get(1)?),
            source_record_id: SourceRecordId(row.get(2)?),
            locator: row.get(3)?,
            evidence_kind: EvidenceKind::from_str(&kind_str).expect("保存済み evidence_kind は常に既知の値"),
            extractor_or_model: row.get(5)?,
            version: row.get(6)?,
            input_hash: row.get(7)?,
            output_hash: row.get(8)?,
            metric_name: row.get(9)?,
            metric_value: row.get(10)?,
            dependency_origin: row.get(11)?,
            external_classification: row.get(12)?,
        })
    }
}

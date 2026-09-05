use crate::model::{NewSourceRecord, SourceRecord, SourceRecordId};
use crate::store::{ProvenanceStore, Result};
use rusqlite::{params, OptionalExtension};

impl ProvenanceStore {
    /// `(provider, provider_id)`でインターンする。同じ論文/ファイルを指す
    /// アダプタ呼び出しを何度実行しても同じSourceRecordに集約される
    /// （`docs/DATA_DICTIONARY.md`設計判断1のper-paper重複排除）。
    pub fn get_or_insert_source_record(&self, new: &NewSourceRecord) -> Result<SourceRecordId> {
        if let Some(id) = self
            .conn
            .prepare_cached("SELECT id FROM source_records WHERE provider = ?1 AND provider_id = ?2")?
            .query_row(params![new.provider, new.provider_id], |r| r.get::<_, i64>(0))
            .optional()?
        {
            return Ok(SourceRecordId(id));
        }
        self.conn
            .prepare_cached(
                "INSERT INTO source_records
                    (provider, provider_id, provider_revision, retrieved_at_unix, content_hash,
                     licence, attribution, raw_payload_uri, adapter_name, adapter_version, parser_version)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )?
            .execute(params![
                new.provider,
                new.provider_id,
                new.provider_revision,
                new.retrieved_at_unix,
                new.content_hash,
                new.licence,
                new.attribution,
                new.raw_payload_uri,
                new.adapter_name,
                new.adapter_version,
                new.parser_version,
            ])?;
        Ok(SourceRecordId(self.conn.last_insert_rowid()))
    }

    pub fn get_source_record(&self, id: SourceRecordId) -> Result<SourceRecord> {
        self.conn
            .prepare_cached(
                "SELECT id, provider, provider_id, provider_revision, retrieved_at_unix, content_hash,
                        licence, attribution, raw_payload_uri, adapter_name, adapter_version, parser_version
                 FROM source_records WHERE id = ?1",
            )?
            .query_row(params![id.0], Self::source_record_row)
    }

    pub fn source_record_count(&self) -> Result<i64> {
        self.conn.query_row("SELECT COUNT(*) FROM source_records", [], |r| r.get(0))
    }

    fn source_record_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SourceRecord> {
        Ok(SourceRecord {
            id: SourceRecordId(row.get(0)?),
            provider: row.get(1)?,
            provider_id: row.get(2)?,
            provider_revision: row.get(3)?,
            retrieved_at_unix: row.get(4)?,
            content_hash: row.get(5)?,
            licence: row.get(6)?,
            attribution: row.get(7)?,
            raw_payload_uri: row.get(8)?,
            adapter_name: row.get(9)?,
            adapter_version: row.get(10)?,
            parser_version: row.get(11)?,
        })
    }
}

use crate::model::{NewSourceRecord, SourceRecord, SourceRecordId};
use crate::store::{ProvenanceStore, Result};
use rusqlite::{params, OptionalExtension};

impl ProvenanceStore {
    /// `(provider, provider_id, provider_revision)`でインターンする。同じ
    /// 論文/ファイルを指すアダプタ呼び出しを何度実行しても同じSourceRecordに
    /// 集約される（`docs/DATA_DICTIONARY.md`設計判断1のper-paper重複排除）。
    ///
    /// 外部レビュー（2026-09-05）指摘の修正: 以前は`(provider, provider_id)`
    /// だけで引いていたため、同じ論文を後で別リビジョン/別内容で取得しても
    /// 古い行が黙って再利用され、ARCHITECTURE_NEXT.md §5.1の「不変な
    /// source envelope」という前提と矛盾していた。`provider_revision`を
    /// 鍵に含める——`IS`で比較するのは、SQLiteのUNIQUE制約はNULL同士を
    /// 別物として扱う（つまり制約だけでは重複を防げない）ため、この
    /// SELECTでの明示的な突き合わせが実質的な重複排除の役割を担う。
    /// このクレートの現在の呼び出し元は全員`provider_revision: None`を渡す
    /// ので、挙動は変わらない——将来、実際にリビジョン違いを渡す呼び出し元が
    /// 現れたときに初めて新しい行が作られるようになる。
    pub fn get_or_insert_source_record(&self, new: &NewSourceRecord) -> Result<SourceRecordId> {
        if let Some(id) = self
            .conn
            .prepare_cached(
                "SELECT id FROM source_records
                 WHERE provider = ?1 AND provider_id = ?2 AND provider_revision IS ?3",
            )?
            .query_row(params![new.provider, new.provider_id, new.provider_revision], |r| r.get::<_, i64>(0))
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

    /// `get_source_record`のOption版（`verify.rs`用、`try_get_assertion`と同じ理由）。
    pub fn try_get_source_record(&self, id: SourceRecordId) -> Result<Option<SourceRecord>> {
        self.conn
            .prepare_cached(
                "SELECT id, provider, provider_id, provider_revision, retrieved_at_unix, content_hash,
                        licence, attribution, raw_payload_uri, adapter_name, adapter_version, parser_version
                 FROM source_records WHERE id = ?1",
            )?
            .query_row(params![id.0], Self::source_record_row)
            .optional()
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

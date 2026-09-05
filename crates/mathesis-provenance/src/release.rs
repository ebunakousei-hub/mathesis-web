use crate::model::{NewRelease, Release, ReleaseId};
use crate::store::{ProvenanceStore, Result};
use rusqlite::{params, OptionalExtension};

impl ProvenanceStore {
    /// `tag`でインターンする（既存なら再利用、`mathesis-graph::intern_paper`と
    /// 同じ方針）。同じリリースを指す呼び出しは何度実行しても同じIDを返す。
    pub fn get_or_insert_release(&self, new: &NewRelease) -> Result<ReleaseId> {
        if let Some(id) = self
            .conn
            .prepare_cached("SELECT id FROM releases WHERE tag = ?1")?
            .query_row(params![new.tag], |r| r.get::<_, i64>(0))
            .optional()?
        {
            return Ok(ReleaseId(id));
        }
        self.conn
            .prepare_cached(
                "INSERT INTO releases (tag, git_commit, generated_at_unix, notes)
                 VALUES (?1, ?2, ?3, ?4)",
            )?
            .execute(params![new.tag, new.git_commit, new.generated_at_unix, new.notes])?;
        Ok(ReleaseId(self.conn.last_insert_rowid()))
    }

    pub fn get_release(&self, id: ReleaseId) -> Result<Release> {
        self.conn
            .prepare_cached(
                "SELECT id, tag, git_commit, generated_at_unix, notes FROM releases WHERE id = ?1",
            )?
            .query_row(params![id.0], Self::release_row)
    }

    pub fn get_release_by_tag(&self, tag: &str) -> Result<Option<Release>> {
        self.conn
            .prepare_cached(
                "SELECT id, tag, git_commit, generated_at_unix, notes FROM releases WHERE tag = ?1",
            )?
            .query_row(params![tag], Self::release_row)
            .optional()
    }

    pub fn list_releases(&self) -> Result<Vec<Release>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT id, tag, git_commit, generated_at_unix, notes FROM releases ORDER BY id")?;
        let rows = stmt.query_map([], Self::release_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    fn release_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Release> {
        Ok(Release {
            id: ReleaseId(row.get(0)?),
            tag: row.get(1)?,
            git_commit: row.get(2)?,
            generated_at_unix: row.get(3)?,
            notes: row.get(4)?,
        })
    }
}

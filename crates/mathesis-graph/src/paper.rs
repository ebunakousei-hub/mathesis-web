//! Phase 9: 判断ノードの由来となった論文（arXiv id等）のノード。
//!
//! `crates/mathesis-taxonomy`/`mathesis-fulltext` が扱うarXiv論文・概念タクソノミー
//! と、この判断グラフ（Lean/Coqの形式証明）を「合流」させる最初の橋渡し。
//! 論文それ自体のメタデータ（title/abstract/MSC等）は`mathesis-ingest`の
//! `papers`テーブルが既に持っているため、ここでは重複させず「この論文に
//! 対応するarXiv idは何か」だけを持つ薄いノードにする——judgmentから
//! `arxiv_id`経由でmathesis-ingest/taxonomy側のDBへ引ける最小限の鍵。
//! `StrategyRecord`（層4）と同じ「名前（ここではarxiv_id）で引ける、
//! 自由に増えるノード」という設計を踏襲する。

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PaperId(pub i64);

#[derive(Debug, Clone)]
pub struct NewPaper {
    pub arxiv_id: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaperRecord {
    pub id: PaperId,
    pub arxiv_id: String,
    pub title: Option<String>,
}

use crate::model::JudgmentId;
use crate::store::{GraphStore, Result};
use rusqlite::{params, OptionalExtension};

impl GraphStore {
    /// arxiv_idで論文ノードをインターンする（既存なら再利用、同じidを何度
    /// 呼んでも同じ`PaperId`が返る——`intern_strategy`と同じ方針）。titleは
    /// 初回挿入時のものを使う（後から呼んでも上書きしない、strategyの
    /// descriptionと同様append-onlyの精神）。
    pub fn intern_paper(&self, arxiv_id: &str, title: Option<&str>) -> Result<PaperId> {
        if let Some(id) = self
            .conn
            .prepare_cached("SELECT id FROM papers WHERE arxiv_id = ?1")?
            .query_row(params![arxiv_id], |r| r.get::<_, i64>(0))
            .optional()?
        {
            return Ok(PaperId(id));
        }
        self.conn
            .prepare_cached("INSERT INTO papers (arxiv_id, title) VALUES (?1, ?2)")?
            .execute(params![arxiv_id, title])?;
        Ok(PaperId(self.conn.last_insert_rowid()))
    }

    pub fn get_paper(&self, id: PaperId) -> Result<Option<PaperRecord>> {
        self.conn
            .prepare_cached("SELECT id, arxiv_id, title FROM papers WHERE id = ?1")?
            .query_row(params![id.0], Self::paper_row)
            .optional()
    }

    pub fn find_paper_by_arxiv_id(&self, arxiv_id: &str) -> Result<Option<PaperRecord>> {
        self.conn
            .prepare_cached("SELECT id, arxiv_id, title FROM papers WHERE arxiv_id = ?1")?
            .query_row(params![arxiv_id], Self::paper_row)
            .optional()
    }

    pub fn list_papers(&self) -> Result<Vec<PaperRecord>> {
        let mut stmt = self.conn.prepare_cached("SELECT id, arxiv_id, title FROM papers ORDER BY id")?;
        let rows = stmt.query_map([], Self::paper_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// この論文の由来を持つ判断ノード一覧（`judgments.source_paper_id`経由）。
    pub fn judgments_of_paper(&self, id: PaperId) -> Result<Vec<JudgmentId>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT id FROM judgments WHERE source_paper_id = ?1 ORDER BY id")?;
        let rows = stmt
            .query_map(params![id.0], |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows.into_iter().map(JudgmentId).collect())
    }

    fn paper_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PaperRecord> {
        Ok(PaperRecord { id: PaperId(row.get(0)?), arxiv_id: row.get(1)?, title: row.get(2)? })
    }
}

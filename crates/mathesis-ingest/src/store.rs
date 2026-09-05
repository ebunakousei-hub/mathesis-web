//! `Paper` の永続化。mathesis-graph（Judgment粒度、SQLite）とはあえて別の
//! DBファイルに分ける——アーキテクチャ.txt 5.6の役割分担のとおり、証明の
//! 粒度と論文メタデータの粒度は別スキーマで持つ。Phase 1ではPostgreSQL等の
//! 外部サービスを新たに立てず、rusqlite（既にmathesis-graphで実績あり）を
//! そのまま使う。

use crate::model::Paper;
use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::Path;

pub struct PaperStore {
    conn: Connection,
}

impl PaperStore {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS papers (
                arxiv_id        TEXT PRIMARY KEY,
                title           TEXT NOT NULL,
                abstract_text   TEXT NOT NULL,
                authors_json    TEXT NOT NULL,
                categories_json TEXT NOT NULL,
                msc_codes_json  TEXT NOT NULL,
                submitted       TEXT NOT NULL,
                fetched_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            );
            CREATE INDEX IF NOT EXISTS idx_papers_submitted ON papers(submitted);
            CREATE TABLE IF NOT EXISTS harvest_state (
                harvest_key       TEXT PRIMARY KEY,
                resumption_token  TEXT,
                harvested         INTEGER NOT NULL DEFAULT 0,
                finished          INTEGER NOT NULL DEFAULT 0,
                updated_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            );",
        )?;
        Ok(Self { conn })
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "CREATE TABLE papers (
                arxiv_id        TEXT PRIMARY KEY,
                title           TEXT NOT NULL,
                abstract_text   TEXT NOT NULL,
                authors_json    TEXT NOT NULL,
                categories_json TEXT NOT NULL,
                msc_codes_json  TEXT NOT NULL,
                submitted       TEXT NOT NULL,
                fetched_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            );
            CREATE TABLE harvest_state (
                harvest_key       TEXT PRIMARY KEY,
                resumption_token  TEXT,
                harvested         INTEGER NOT NULL DEFAULT 0,
                finished          INTEGER NOT NULL DEFAULT 0,
                updated_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            );",
        )?;
        Ok(Self { conn })
    }

    /// 全件を1トランザクションでUPSERTする。1行ごとの自動コミットに任せると
    /// 行数分のfsync待ちが支配的になる（教訓は
    /// `mathesis_graph::GraphStore::transaction` のドキュメント参照）。
    pub fn upsert_all(&mut self, papers: &[Paper]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO papers
                    (arxiv_id, title, abstract_text, authors_json, categories_json, msc_codes_json, submitted)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(arxiv_id) DO UPDATE SET
                    title=excluded.title,
                    abstract_text=excluded.abstract_text,
                    authors_json=excluded.authors_json,
                    categories_json=excluded.categories_json,
                    msc_codes_json=excluded.msc_codes_json,
                    submitted=excluded.submitted",
            )?;
            for p in papers {
                stmt.execute(params![
                    p.arxiv_id,
                    p.title,
                    p.abstract_text,
                    serde_json::to_string(&p.authors)?,
                    serde_json::to_string(&p.categories)?,
                    serde_json::to_string(&p.msc_codes)?,
                    p.submitted,
                ])?;
            }
        }
        tx.commit()?;
        Ok(papers.len())
    }

    /// 保存済みの全論文を読み出す（`mathesis-taxonomy` のConcept抽出入力用）。
    pub fn list_all(&self) -> Result<Vec<Paper>> {
        let mut stmt = self.conn.prepare(
            "SELECT arxiv_id, title, abstract_text, authors_json, categories_json, msc_codes_json, submitted
             FROM papers",
        )?;
        let rows = stmt
            .query_map([], |r| {
                let authors_json: String = r.get(3)?;
                let categories_json: String = r.get(4)?;
                let msc_codes_json: String = r.get(5)?;
                Ok(Paper {
                    arxiv_id: r.get(0)?,
                    title: r.get(1)?,
                    abstract_text: r.get(2)?,
                    authors: serde_json::from_str(&authors_json).unwrap_or_default(),
                    categories: serde_json::from_str(&categories_json).unwrap_or_default(),
                    msc_codes: serde_json::from_str(&msc_codes_json).unwrap_or_default(),
                    submitted: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<Paper>>>()?;
        Ok(rows)
    }

    /// 収集の途中経過（次に使う `resumptionToken` と累計件数）を記録する。
    ///
    /// これが無かった頃、`harvest` は全ページをメモリに溜めてから最後に
    /// 一度だけ保存していた。途中でネットワークが切れれば**それまでの
    /// 収集が丸ごと消える**——embedding生成で実際に起きたのと同じ
    /// 「最後にしか永続化しない」失敗パターン（10万件中39,400件で
    /// クラッシュして全損した）。ページごとに保存し、トークンを残す。
    pub fn save_harvest_state(
        &mut self,
        key: &str,
        resumption_token: Option<&str>,
        harvested: usize,
        finished: bool,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO harvest_state (harvest_key, resumption_token, harvested, finished, updated_at)
             VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%SZ','now'))
             ON CONFLICT(harvest_key) DO UPDATE SET
                resumption_token = excluded.resumption_token,
                harvested = excluded.harvested,
                finished = excluded.finished,
                updated_at = excluded.updated_at",
            params![key, resumption_token, harvested as i64, i64::from(finished)],
        )?;
        Ok(())
    }

    /// 中断した収集の続き（`resumptionToken`, 累計件数, 完了済みか）。
    pub fn load_harvest_state(&self, key: &str) -> Result<Option<(Option<String>, usize, bool)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT resumption_token, harvested, finished FROM harvest_state WHERE harvest_key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        match rows.next()? {
            Some(row) => {
                let token: Option<String> = row.get(0)?;
                let harvested: i64 = row.get(1)?;
                let finished: i64 = row.get(2)?;
                Ok(Some((token, harvested as usize, finished != 0)))
            }
            None => Ok(None),
        }
    }

    /// 増分収集の起点にする日付（YYYY-MM-DD）。
    ///
    /// 「前回この対象を収集した日」を返す。OAI-PMHの `from` が比較するのは
    /// レコードの datestamp（arXiv側で最後に更新された日時）なので、
    /// 「前回の収集以降に更新されたもの」を取るにはこちらが正しい起点になる。
    ///
    /// 最初は論文の `submitted`（投稿日）の最大値を使おうとして失敗した——
    /// `submitted` はarXivの生の日付文字列（"Wed, 31 May 2006 12:00:00 GMT"
    /// 形式）をそのまま保持しており、(a) 先頭10文字は "Wed, 31 Ma" という
    /// 日付ですらない文字列になり、(b) この形式では文字列としての MAX が
    /// 時系列の最大と一致しない。実際にarXivから
    /// `from date format must be YYYY-MM-DD` と拒否されて判明した。
    /// `fetched_at` はこちらが `strftime('%Y-%m-%dT%H:%M:%SZ')` で入れて
    /// いるので、素直にISOで並ぶ。
    pub fn last_harvest_date(&self) -> Result<Option<String>> {
        let value: Option<String> = self
            .conn
            .query_row("SELECT MAX(fetched_at) FROM papers", [], |r| r.get(0))?;
        Ok(value.map(|v| v.chars().take(10).collect()))
    }

    pub fn count(&self) -> Result<usize> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM papers", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    pub fn count_with_msc(&self) -> Result<usize> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM papers WHERE msc_codes_json != '[]'",
            [],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: &str) -> Paper {
        Paper {
            arxiv_id: id.to_string(),
            title: "Sample title".to_string(),
            abstract_text: "Sample abstract".to_string(),
            authors: vec!["A. Author".to_string()],
            categories: vec!["math.CT".to_string()],
            msc_codes: vec!["18A05".to_string()],
            submitted: "Tue, 22 Dec 2020 02:43:53 GMT".to_string(),
        }
    }

    #[test]
    fn upsert_then_count_round_trips() {
        let mut store = PaperStore::open_in_memory().unwrap();
        store.upsert_all(&[sample("2012.11800"), sample("2012.11801")]).unwrap();
        assert_eq!(store.count().unwrap(), 2);
        assert_eq!(store.count_with_msc().unwrap(), 2);
    }

    #[test]
    fn list_all_round_trips_every_field() {
        let mut store = PaperStore::open_in_memory().unwrap();
        let p = sample("2012.11800");
        store.upsert_all(std::slice::from_ref(&p)).unwrap();
        let back = store.list_all().unwrap();
        assert_eq!(back, vec![p]);
    }

    #[test]
    fn upsert_is_idempotent_on_the_same_arxiv_id() {
        let mut store = PaperStore::open_in_memory().unwrap();
        let mut p = sample("2012.11800");
        store.upsert_all(&[p.clone()]).unwrap();
        p.title = "Updated title".to_string();
        store.upsert_all(&[p]).unwrap();

        assert_eq!(store.count().unwrap(), 1, "re-ingesting the same id must not duplicate rows");
        let title: String = store
            .conn
            .query_row("SELECT title FROM papers WHERE arxiv_id = '2012.11800'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(title, "Updated title");
    }

    #[test]
    fn papers_without_msc_codes_are_not_counted() {
        let mut store = PaperStore::open_in_memory().unwrap();
        let mut p = sample("cs/9812019");
        p.msc_codes = vec![];
        store.upsert_all(&[p]).unwrap();
        assert_eq!(store.count().unwrap(), 1);
        assert_eq!(store.count_with_msc().unwrap(), 0);
    }

    #[test]
    fn harvest_state_round_trips_and_is_updated_in_place() {
        let mut store = PaperStore::open_in_memory().unwrap();
        assert_eq!(store.load_harvest_state("math||").unwrap(), None);

        store.save_harvest_state("math||", Some("tok-1"), 1000, false).unwrap();
        assert_eq!(
            store.load_harvest_state("math||").unwrap(),
            Some((Some("tok-1".to_string()), 1000, false))
        );

        // 同じキーへの2回目は上書き（履歴を積まない）
        store.save_harvest_state("math||", Some("tok-2"), 2000, false).unwrap();
        assert_eq!(
            store.load_harvest_state("math||").unwrap(),
            Some((Some("tok-2".to_string()), 2000, false))
        );

        // 完了したらトークンは無く finished が立つ
        store.save_harvest_state("math||", None, 2500, true).unwrap();
        assert_eq!(store.load_harvest_state("math||").unwrap(), Some((None, 2500, true)));
    }

    /// 期間が違えば別の収集なので、前回のトークンを取り違えないこと。
    #[test]
    fn harvest_state_is_keyed_per_set_and_date_range() {
        let mut store = PaperStore::open_in_memory().unwrap();
        store.save_harvest_state("math||", Some("all"), 10, false).unwrap();
        store.save_harvest_state("math|2026-09-01|", Some("incremental"), 3, false).unwrap();
        assert_eq!(store.load_harvest_state("math||").unwrap().unwrap().0, Some("all".to_string()));
        assert_eq!(
            store.load_harvest_state("math|2026-09-01|").unwrap().unwrap().0,
            Some("incremental".to_string())
        );
    }

    /// 増分収集の起点は YYYY-MM-DD でなければならない（arXivが他の形式を
    /// 拒否する）。論文の `submitted` はarXivの生の文字列なのでここには
    /// 使えない——実際にそれで `badArgument` を返された。
    #[test]
    fn last_harvest_date_is_an_iso_day_not_the_raw_arxiv_submitted_string() {
        let mut store = PaperStore::open_in_memory().unwrap();
        assert_eq!(store.last_harvest_date().unwrap(), None);
        store
            .upsert_all(&[sample_paper("1", "Wed, 31 May 2006 12:00:00 GMT")])
            .unwrap();
        let day = store.last_harvest_date().unwrap().expect("収集後は日付が取れる");
        assert_eq!(day.len(), 10, "YYYY-MM-DD の10文字であること: {day}");
        assert!(
            day.chars().all(|c| c.is_ascii_digit() || c == '-'),
            "数字とハイフンだけであること: {day}"
        );
    }

    fn sample_paper(id: &str, submitted: &str) -> Paper {
        Paper {
            arxiv_id: id.to_string(),
            title: format!("paper {id}"),
            abstract_text: String::new(),
            authors: vec![],
            categories: vec!["math.AG".to_string()],
            msc_codes: vec![],
            submitted: submitted.to_string(),
        }
    }
}

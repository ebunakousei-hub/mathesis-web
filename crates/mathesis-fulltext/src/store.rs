//! 取得済みソース・抽出済み定理/証明の永続化。`mathesis-ingest::PaperStore`
//! と同じDBファイルに相乗りする（アーキテクチャ.txt 5.6の方針通り、
//! 新たな外部サービスをこの段階で立てない）。

use crate::theorem::{ProofMatch, TheoremRecord};
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::path::Path;

fn proof_match_to_str(m: ProofMatch) -> &'static str {
    match m {
        ProofMatch::Titled => "titled",
        ProofMatch::Adjacent => "adjacent",
        ProofMatch::None => "none",
    }
}

pub struct FulltextStore {
    conn: Connection,
}

impl FulltextStore {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS paper_sources (
                arxiv_id   TEXT PRIMARY KEY,
                has_source INTEGER NOT NULL,
                file_count INTEGER NOT NULL,
                tex_text   TEXT,
                fetched_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            );
            CREATE TABLE IF NOT EXISTS paper_theorems (
                id             INTEGER PRIMARY KEY AUTOINCREMENT,
                arxiv_id       TEXT NOT NULL,
                order_index    INTEGER NOT NULL,
                kind           TEXT NOT NULL,
                label          TEXT,
                has_proof      INTEGER NOT NULL,
                proof_match    TEXT NOT NULL,
                cites_json     TEXT NOT NULL,
                statement_text TEXT NOT NULL DEFAULT '',
                source_line    INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_paper_theorems_arxiv ON paper_theorems(arxiv_id);
            CREATE TABLE IF NOT EXISTS theorem_dependencies (
                arxiv_id         TEXT NOT NULL,
                from_order_index INTEGER NOT NULL,
                to_label         TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_theorem_deps_arxiv ON theorem_dependencies(arxiv_id);
            -- 診断⑥拡張: 引用キー→arXiv IDの対応（citation::extract_arxiv_id
            -- が明示的な\"arXiv:\"記載から解決できた分だけ）。
            CREATE TABLE IF NOT EXISTS bibitem_citations (
                arxiv_id        TEXT NOT NULL,
                cite_key        TEXT NOT NULL,
                target_arxiv_id TEXT NOT NULL,
                PRIMARY KEY (arxiv_id, cite_key)
            );",
        )?;
        Ok(Self { conn })
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            // `papers`はmathesis-ingest::PaperStoreが同じDBファイルに作る
            // テーブル——ここではjoinクエリのテスト用に必要最小限だけ再現する。
            "CREATE TABLE papers (arxiv_id TEXT PRIMARY KEY, title TEXT NOT NULL DEFAULT '');
            CREATE TABLE paper_sources (
                arxiv_id TEXT PRIMARY KEY, has_source INTEGER NOT NULL,
                file_count INTEGER NOT NULL, tex_text TEXT
            );
            CREATE TABLE paper_theorems (
                id INTEGER PRIMARY KEY AUTOINCREMENT, arxiv_id TEXT NOT NULL,
                order_index INTEGER NOT NULL, kind TEXT NOT NULL, label TEXT,
                has_proof INTEGER NOT NULL, proof_match TEXT NOT NULL, cites_json TEXT NOT NULL,
                statement_text TEXT NOT NULL DEFAULT '', source_line INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE theorem_dependencies (
                arxiv_id TEXT NOT NULL, from_order_index INTEGER NOT NULL, to_label TEXT NOT NULL
            );
            CREATE TABLE bibitem_citations (
                arxiv_id TEXT NOT NULL, cite_key TEXT NOT NULL, target_arxiv_id TEXT NOT NULL,
                PRIMARY KEY (arxiv_id, cite_key)
            );",
        )?;
        Ok(Self { conn })
    }

    #[cfg(test)]
    fn seed_papers(&self, arxiv_ids: &[&str]) -> Result<()> {
        for id in arxiv_ids {
            self.conn.execute("INSERT INTO papers (arxiv_id) VALUES (?1)", params![id])?;
        }
        Ok(())
    }

    /// 取得できなかった論文（`has_source=0`）もあえて記録する——再実行のたびに
    /// 同じPDF専用論文へ空振りリクエストを繰り返さないため（`fetch-sources`
    /// は既に`paper_sources`にある`arxiv_id`をスキップする）。
    pub fn save_source(&mut self, arxiv_id: &str, tex_text: Option<&str>, file_count: usize) -> Result<()> {
        self.conn.execute(
            "INSERT INTO paper_sources (arxiv_id, has_source, file_count, tex_text)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(arxiv_id) DO UPDATE SET
                has_source=excluded.has_source, file_count=excluded.file_count, tex_text=excluded.tex_text",
            params![arxiv_id, tex_text.is_some() as i64, file_count as i64, tex_text],
        )?;
        Ok(())
    }

    /// まだ`paper_sources`に記録の無い`arxiv_id`（`mathesis-ingest`の
    /// `papers`テーブル基準）を、`limit`件まで返す。`ORDER BY p.arxiv_id`
    /// ではなく`RANDOM()`にしている——旧形式のarxiv_idはカテゴリ接頭辞
    /// （"alg-geom/"「math/"「hep-th/"等）で始まるため、単純な辞書順だと
    /// 手前のカテゴリだけを延々取り続けてしまう（実際に最初の検証で
    /// alg-geomばかり60件取得してしまい、他分野のLaTeX記法の多様性
    /// （`\newenvironment`での証明環境の別名定義等）を見落としかけた）。
    pub fn list_arxiv_ids_needing_source(&self, limit: usize) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT p.arxiv_id FROM papers p
             LEFT JOIN paper_sources s ON s.arxiv_id = p.arxiv_id
             WHERE s.arxiv_id IS NULL
             ORDER BY RANDOM()
             LIMIT ?1",
        )?;
        let rows =
            stmt.query_map(params![limit as i64], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(rows)
    }

    pub fn source_stats(&self) -> Result<(usize, usize)> {
        let total: i64 = self.conn.query_row("SELECT COUNT(*) FROM paper_sources", [], |r| r.get(0))?;
        let with_source: i64 =
            self.conn.query_row("SELECT COUNT(*) FROM paper_sources WHERE has_source = 1", [], |r| r.get(0))?;
        Ok((total as usize, with_source as usize))
    }

    /// テキストが取得できた全論文の (arxiv_id, tex_text) を返す
    /// （`extract-theorems`の入力）。
    pub fn list_fetched_tex(&self) -> Result<Vec<(String, String)>> {
        let mut stmt =
            self.conn.prepare("SELECT arxiv_id, tex_text FROM paper_sources WHERE has_source = 1 AND tex_text IS NOT NULL")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<(String, String)>>>()?;
        Ok(rows)
    }

    /// 1論文分の抽出結果を置き換える（再実行が冪等になるよう、既存分は
    /// 先に削除してから入れ直す——`TaxonomyStore::replace_all`と同じ方針）。
    pub fn save_theorems(&mut self, arxiv_id: &str, records: &[TheoremRecord]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM paper_theorems WHERE arxiv_id = ?1", params![arxiv_id])?;
        tx.execute("DELETE FROM theorem_dependencies WHERE arxiv_id = ?1", params![arxiv_id])?;
        {
            let mut thm_stmt = tx.prepare(
                "INSERT INTO paper_theorems
                    (arxiv_id, order_index, kind, label, has_proof, proof_match, cites_json,
                     statement_text, source_line)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            let mut dep_stmt = tx.prepare(
                "INSERT INTO theorem_dependencies (arxiv_id, from_order_index, to_label) VALUES (?1, ?2, ?3)",
            )?;
            for r in records {
                thm_stmt.execute(params![
                    arxiv_id,
                    r.order as i64,
                    r.kind,
                    r.label,
                    r.has_proof as i64,
                    proof_match_to_str(r.proof_match),
                    serde_json::to_string(&r.cites).unwrap_or_else(|_| "[]".to_string()),
                    r.statement_text,
                    r.line,
                ])?;
                for to_label in &r.depends_on_labels {
                    dep_stmt.execute(params![arxiv_id, r.order as i64, to_label])?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 診断⑥（Statementノード）のブリッジ（`mathesis-fulltext bridge-to-graph`）
    /// が読む、1論文分の定理・依存関係。`arxiv_id`ごとに独立して処理する
    /// ——ラベル→JudgmentIdの対応も論文内で完結させるため
    /// （`mathesis-importer::dependencies`が1回のインポート単位で名前解決を
    /// 閉じているのと同じ理由）。
    pub fn list_theorem_arxiv_ids(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT DISTINCT arxiv_id FROM paper_theorems ORDER BY arxiv_id")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(rows)
    }

    pub fn theorems_for_paper(&self, arxiv_id: &str) -> Result<Vec<BridgeTheorem>> {
        let mut stmt = self.conn.prepare(
            "SELECT order_index, kind, label, statement_text, source_line, cites_json
             FROM paper_theorems WHERE arxiv_id = ?1 ORDER BY order_index",
        )?;
        let rows = stmt
            .query_map(params![arxiv_id], |r| {
                let cites_json: String = r.get(5)?;
                Ok(BridgeTheorem {
                    order: r.get(0)?,
                    kind: r.get(1)?,
                    label: r.get(2)?,
                    statement_text: r.get(3)?,
                    source_line: r.get::<_, i64>(4)? as u32,
                    cites: serde_json::from_str(&cites_json).unwrap_or_default(),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// 引用キー→arXiv IDの対応（`citation::extract_arxiv_id`が解決できた
    /// 分だけ）を1論文ぶん保存する。既存分は置き換える
    /// （`save_theorems`と同じ「再実行は蓄積ではなく置き換え」の方針）。
    pub fn save_bibitem_citations(&mut self, arxiv_id: &str, resolved: &[(String, String)]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM bibitem_citations WHERE arxiv_id = ?1", params![arxiv_id])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO bibitem_citations (arxiv_id, cite_key, target_arxiv_id) VALUES (?1, ?2, ?3)",
            )?;
            for (cite_key, target_arxiv_id) in resolved {
                stmt.execute(params![arxiv_id, cite_key, target_arxiv_id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 1論文ぶんの引用キー→arXiv IDの対応。`bridge.rs`が定理の`cites`
    /// （引用キー）をこれで引いて、論文単位の引用先を特定する。
    pub fn bibitem_citations_for_paper(&self, arxiv_id: &str) -> Result<HashMap<String, String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT cite_key, target_arxiv_id FROM bibitem_citations WHERE arxiv_id = ?1")?;
        let rows = stmt
            .query_map(params![arxiv_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows.into_iter().collect())
    }

    pub fn dependencies_for_paper(&self, arxiv_id: &str) -> Result<Vec<BridgeDependency>> {
        let mut stmt = self
            .conn
            .prepare("SELECT from_order_index, to_label FROM theorem_dependencies WHERE arxiv_id = ?1")?;
        let rows = stmt
            .query_map(params![arxiv_id], |r| Ok(BridgeDependency { from_order: r.get(0)?, to_label: r.get(1)? }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// `mathesis-ingest::PaperStore`が同じDBファイルに作る`papers`テーブルの
    /// 題名を引く。`mathesis-fulltext`はarxiv_idの一覧をそちら経由で得るため
    /// （`list_arxiv_ids_needing_source`）、既にスキーマの前提を共有している。
    pub fn paper_title(&self, arxiv_id: &str) -> Result<Option<String>> {
        let title = self
            .conn
            .prepare_cached("SELECT title FROM papers WHERE arxiv_id = ?1")?
            .query_row(params![arxiv_id], |r| r.get::<_, String>(0))
            .optional()?;
        Ok(title)
    }

    pub fn theorem_stats(&self) -> Result<TheoremStats> {
        let count = |sql: &str| -> Result<usize> { Ok(self.conn.query_row(sql, [], |r| r.get::<_, i64>(0))? as usize) };
        Ok(TheoremStats {
            total: count("SELECT COUNT(*) FROM paper_theorems")?,
            with_proof: count("SELECT COUNT(*) FROM paper_theorems WHERE has_proof = 1")?,
            // has_proof=1のものだけがtitled/adjacentのどちらかになるはずなので、
            // 内訳の合計はwith_proofと一致する（テストで確認）。
            titled_matches: count("SELECT COUNT(*) FROM paper_theorems WHERE proof_match = 'titled'")?,
            adjacent_matches: count("SELECT COUNT(*) FROM paper_theorems WHERE proof_match = 'adjacent'")?,
            dependency_edges: count("SELECT COUNT(*) FROM theorem_dependencies")?,
        })
    }
}

/// 証明ペアリングの内訳込みの集計。`titled`は`\begin{proof}[...\ref{X}]`で
/// 明示的に紐付けられた高確度な一致、`adjacent`は直前の未証明の定理への
/// フォールバック一致——`extract-theorems`のCLI出力で両方を分けて報告する
/// ことで、「本当に証明が拾えているか」の確度を利用者が判断できるようにする。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TheoremStats {
    pub total: usize,
    pub with_proof: usize,
    pub titled_matches: usize,
    pub adjacent_matches: usize,
    pub dependency_edges: usize,
}

/// `theorems_for_paper`が返す、ブリッジ（`mathesis-graph`への取り込み）に
/// 必要なだけの1定理ぶんの情報。
#[derive(Debug, Clone)]
pub struct BridgeTheorem {
    pub order: i64,
    pub kind: String,
    pub label: Option<String>,
    pub statement_text: String,
    pub source_line: u32,
    /// 証明中で使われた文献引用キー（`paper_theorems.cites_json`）。
    /// `bridge.rs`が`bibitem_citations_for_paper`と突き合わせて、論文単位の
    /// 引用先（診断⑥拡張）を特定するのに使う。
    pub cites: Vec<String>,
}

/// `dependencies_for_paper`が返す1本の依存関係（`from_order`の定理が
/// ラベル`to_label`の定理を`\ref`/`\eqref`で参照している）。
#[derive(Debug, Clone)]
pub struct BridgeDependency {
    pub from_order: i64,
    pub to_label: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theorem::TheoremRecord;

    #[test]
    fn list_arxiv_ids_needing_source_excludes_already_fetched_ones() {
        let mut store = FulltextStore::open_in_memory().unwrap();
        store.seed_papers(&["a", "b", "c"]).unwrap();
        store.save_source("a", Some("tex"), 1).unwrap();

        let mut needing = store.list_arxiv_ids_needing_source(10).unwrap();
        needing.sort(); // RANDOM()で返るので順序ではなく集合として比較する
        assert_eq!(needing, vec!["b".to_string(), "c".to_string()]);
    }

    #[test]
    fn save_source_records_a_miss_so_it_is_not_retried() {
        let mut store = FulltextStore::open_in_memory().unwrap();
        store.seed_papers(&["a"]).unwrap();
        store.save_source("a", None, 0).unwrap();

        assert!(store.list_arxiv_ids_needing_source(10).unwrap().is_empty(), "既に記録済みなので再取得対象に出てはいけない");
        let (total, with_source) = store.source_stats().unwrap();
        assert_eq!((total, with_source), (1, 0));
    }

    #[test]
    fn save_source_is_idempotent_on_the_same_arxiv_id() {
        let mut store = FulltextStore::open_in_memory().unwrap();
        store.seed_papers(&["a"]).unwrap();
        store.save_source("a", None, 0).unwrap();
        store.save_source("a", Some("updated tex"), 2).unwrap();

        let (total, with_source) = store.source_stats().unwrap();
        assert_eq!((total, with_source), (1, 1), "同じarxiv_idの再保存は上書きであるべき");
    }

    fn record(order: usize, kind: &str, label: Option<&str>, has_proof: bool, depends_on: &[&str], cites: &[&str]) -> TheoremRecord {
        TheoremRecord {
            kind: kind.to_string(),
            label: label.map(str::to_string),
            order,
            has_proof,
            proof_match: if has_proof { crate::theorem::ProofMatch::Adjacent } else { crate::theorem::ProofMatch::None },
            depends_on_labels: depends_on.iter().map(|s| s.to_string()).collect(),
            cites: cites.iter().map(|s| s.to_string()).collect(),
            statement_text: format!("statement of {order}"),
            line: (order + 1) as u32,
        }
    }

    #[test]
    fn save_theorems_persists_records_and_dependency_edges() {
        let mut store = FulltextStore::open_in_memory().unwrap();
        let records = vec![
            record(0, "Theorem", Some("first"), true, &[], &["smith99"]),
            record(1, "Theorem", Some("second"), true, &["first"], &[]),
        ];
        store.save_theorems("paper-1", &records).unwrap();

        let stats = store.theorem_stats().unwrap();
        assert_eq!((stats.total, stats.with_proof, stats.dependency_edges), (2, 2, 1));
        assert_eq!(stats.adjacent_matches, 2, "record()ヘルパーはhas_proof=trueをAdjacentとして作る");
        assert_eq!(stats.titled_matches, 0);
    }

    #[test]
    fn save_theorems_replaces_previous_extraction_for_the_same_paper() {
        let mut store = FulltextStore::open_in_memory().unwrap();
        store.save_theorems("paper-1", &[record(0, "Theorem", Some("a"), true, &[], &[])]).unwrap();
        store.save_theorems("paper-1", &[record(0, "Lemma", Some("b"), false, &[], &[])]).unwrap();

        let stats = store.theorem_stats().unwrap();
        assert_eq!(stats.total, 1, "再抽出は置き換えであって蓄積であってはいけない");
    }

    #[test]
    fn theorems_for_paper_carries_the_cites_recorded_at_save_time() {
        let mut store = FulltextStore::open_in_memory().unwrap();
        store.save_theorems("paper-1", &[record(0, "Theorem", Some("a"), true, &[], &["smith99", "jones"])]).unwrap();

        let theorems = store.theorems_for_paper("paper-1").unwrap();
        assert_eq!(theorems.len(), 1);
        assert_eq!(theorems[0].cites, vec!["smith99".to_string(), "jones".to_string()]);
    }

    #[test]
    fn bibitem_citations_round_trip() {
        let mut store = FulltextStore::open_in_memory().unwrap();
        store
            .save_bibitem_citations("paper-1", &[("smith99".to_string(), "math/9901234".to_string())])
            .unwrap();

        let resolved = store.bibitem_citations_for_paper("paper-1").unwrap();
        assert_eq!(resolved.get("smith99"), Some(&"math/9901234".to_string()));
        assert_eq!(resolved.len(), 1, "解決できなかった引用キーは対応表に含めない前提");
    }

    #[test]
    fn save_bibitem_citations_replaces_previous_resolution_for_the_same_paper() {
        let mut store = FulltextStore::open_in_memory().unwrap();
        store.save_bibitem_citations("paper-1", &[("old".to_string(), "math/0000001".to_string())]).unwrap();
        store.save_bibitem_citations("paper-1", &[("new".to_string(), "math/0000002".to_string())]).unwrap();

        let resolved = store.bibitem_citations_for_paper("paper-1").unwrap();
        assert_eq!(resolved.len(), 1, "再抽出は置き換えであって蓄積であってはいけない");
        assert_eq!(resolved.get("new"), Some(&"math/0000002".to_string()));
    }
}

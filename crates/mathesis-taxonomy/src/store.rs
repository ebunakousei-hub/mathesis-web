//! 抽出結果の永続化。`mathesis-ingest::store::PaperStore` と同じDBファイルに
//! テーブルを追加する（アーキテクチャ.txt 5.6: Phase 1同様、Postgresは
//! まだ立てず、papers用のSQLiteに相乗りする）。

use crate::alignment::{ClusterAlignment, InferredAlignment};
use crate::concepts::{ConceptCandidate, ExtractionResult};
use crate::embed::{bytes_to_f32, f32_to_bytes};
use crate::relations::{RelationEdge, RelationKind, RelationStatus};
use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use std::path::Path;

pub struct TaxonomyStore {
    conn: Connection,
}

/// Web版が「フレーズ」ではなく「文書」を見せられるようにするための、
/// 論文1件の最小メタデータ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaperMeta {
    pub arxiv_id: String,
    pub title: String,
    /// 投稿年。取れない場合は `None`（`submitted` はarXivの生文字列
    /// "Wed, 31 May 2006 …" で、ISO形式とは限らない——
    /// アーキテクチャ.txt に記録済みの落とし穴なので、ここでも
    /// 先頭10文字を切るような扱いはしない）。
    pub year: Option<u16>,
    pub categories: Vec<String>,
}

/// タイトルは改行やインデントを含んだまま格納されている（OAI-PMHの
/// XMLがそう返す）。表示にそのまま使えるよう空白を1つに畳む。
fn normalize_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `submitted` から西暦4桁を拾う。形式に依存しないよう、19xx/20xx の
/// 並びを探す——"Wed, 31 May 2006 12:00:00 GMT" でも "2006-05-31" でも
/// 同じ答えになる。
fn submitted_year(submitted: &str) -> Option<u16> {
    let bytes: Vec<char> = submitted.chars().collect();
    for window in bytes.windows(4) {
        if window.iter().all(|c| c.is_ascii_digit()) {
            let year: u16 = window.iter().collect::<String>().parse().ok()?;
            if (1900..=2100).contains(&year) {
                return Some(year);
            }
        }
    }
    None
}

impl TaxonomyStore {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS concept_candidates (
                phrase          TEXT PRIMARY KEY,
                doc_freq        INTEGER NOT NULL,
                mean_score      REAL NOT NULL,
                msc_code        TEXT,
                sample_arxiv_ids_json TEXT NOT NULL,
                field_concentration REAL
            );
            CREATE TABLE IF NOT EXISTS paper_concepts (
                arxiv_id TEXT NOT NULL,
                phrase   TEXT NOT NULL,
                PRIMARY KEY (arxiv_id, phrase)
            );
            CREATE INDEX IF NOT EXISTS idx_paper_concepts_phrase ON paper_concepts(phrase);
            CREATE TABLE IF NOT EXISTS concept_embeddings (
                phrase TEXT PRIMARY KEY,
                model  TEXT NOT NULL,
                dim    INTEGER NOT NULL,
                vector BLOB NOT NULL
            );
            CREATE TABLE IF NOT EXISTS concept_context_vectors (
                phrase TEXT PRIMARY KEY,
                dim    INTEGER NOT NULL,
                vector BLOB NOT NULL
            );
            CREATE TABLE IF NOT EXISTS concept_relations (
                subject           TEXT NOT NULL,
                object            TEXT NOT NULL,
                kind              TEXT NOT NULL,
                status            TEXT NOT NULL,
                confidence        REAL NOT NULL,
                evidence_sentence TEXT,
                evidence_arxiv_id TEXT,
                PRIMARY KEY (subject, object, kind)
            );
            CREATE TABLE IF NOT EXISTS concept_clusters (
                phrase     TEXT PRIMARY KEY,
                cluster_id INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_concept_clusters_cluster ON concept_clusters(cluster_id);
            CREATE TABLE IF NOT EXISTS cluster_alignments (
                cluster_id     INTEGER PRIMARY KEY,
                size           INTEGER NOT NULL,
                grounded_count INTEGER NOT NULL,
                dominant_code  TEXT,
                dominant_name  TEXT,
                confidence     REAL NOT NULL
            );
            CREATE TABLE IF NOT EXISTS phrase_msc_inferred (
                phrase        TEXT PRIMARY KEY,
                cluster_id    INTEGER NOT NULL,
                inferred_code TEXT NOT NULL,
                inferred_name TEXT NOT NULL,
                confidence    REAL NOT NULL
            );",
        )?;
        // `field_concentration` は後から足した列なので、それ以前に作られた
        // DB（`CREATE TABLE IF NOT EXISTS` では更新されない）には明示的に
        // 追加する。既に列があるなら何もしない。
        let has_concentration: bool = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('concept_candidates') WHERE name = 'field_concentration'",
            [],
            |r| r.get::<_, i64>(0),
        )? > 0;
        if !has_concentration {
            conn.execute("ALTER TABLE concept_candidates ADD COLUMN field_concentration REAL", [])?;
        }
        Ok(Self { conn })
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "CREATE TABLE concept_candidates (
                phrase          TEXT PRIMARY KEY,
                doc_freq        INTEGER NOT NULL,
                mean_score      REAL NOT NULL,
                msc_code        TEXT,
                sample_arxiv_ids_json TEXT NOT NULL,
                field_concentration REAL
            );
            CREATE TABLE paper_concepts (
                arxiv_id TEXT NOT NULL,
                phrase   TEXT NOT NULL,
                PRIMARY KEY (arxiv_id, phrase)
            );
            CREATE TABLE concept_embeddings (
                phrase TEXT PRIMARY KEY,
                model  TEXT NOT NULL,
                dim    INTEGER NOT NULL,
                vector BLOB NOT NULL
            );
            CREATE TABLE concept_context_vectors (
                phrase TEXT PRIMARY KEY,
                dim    INTEGER NOT NULL,
                vector BLOB NOT NULL
            );
            CREATE TABLE concept_relations (
                subject           TEXT NOT NULL,
                object            TEXT NOT NULL,
                kind              TEXT NOT NULL,
                status            TEXT NOT NULL,
                confidence        REAL NOT NULL,
                evidence_sentence TEXT,
                evidence_arxiv_id TEXT,
                PRIMARY KEY (subject, object, kind)
            );
            CREATE TABLE papers (
                arxiv_id        TEXT PRIMARY KEY,
                title           TEXT NOT NULL,
                abstract_text   TEXT NOT NULL,
                authors_json    TEXT NOT NULL,
                categories_json TEXT NOT NULL,
                msc_codes_json  TEXT NOT NULL,
                submitted       TEXT NOT NULL,
                fetched_at      TEXT NOT NULL
            );
            CREATE TABLE concept_clusters (
                phrase     TEXT PRIMARY KEY,
                cluster_id INTEGER NOT NULL
            );
            CREATE TABLE cluster_alignments (
                cluster_id     INTEGER PRIMARY KEY,
                size           INTEGER NOT NULL,
                grounded_count INTEGER NOT NULL,
                dominant_code  TEXT,
                dominant_name  TEXT,
                confidence     REAL NOT NULL
            );
            CREATE TABLE phrase_msc_inferred (
                phrase        TEXT PRIMARY KEY,
                cluster_id    INTEGER NOT NULL,
                inferred_code TEXT NOT NULL,
                inferred_name TEXT NOT NULL,
                confidence    REAL NOT NULL
            );",
        )?;
        Ok(Self { conn })
    }

    /// 既存の抽出結果を一括で置き換える（Phase 2 は毎回フルスキャンでの
    /// 再抽出を前提としており、増分更新はまだ実装しない——インクリメンタル化は
    /// mathesis-graphのヒューリスティック提案で行ったのと同様、実際に
    /// コストが問題になってから着手する）。
    pub fn replace_all(&mut self, result: &ExtractionResult) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM concept_candidates", [])?;
        tx.execute("DELETE FROM paper_concepts", [])?;
        {
            let mut cand_stmt = tx.prepare(
                "INSERT INTO concept_candidates (phrase, doc_freq, mean_score, msc_code, sample_arxiv_ids_json, field_concentration)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for c in &result.candidates {
                cand_stmt.execute(params![
                    c.phrase,
                    c.doc_freq as i64,
                    c.mean_score,
                    c.msc_code,
                    serde_json::to_string(&c.sample_arxiv_ids)?,
                    c.field_concentration,
                ])?;
            }

            let mut link_stmt =
                tx.prepare("INSERT OR IGNORE INTO paper_concepts (arxiv_id, phrase) VALUES (?1, ?2)")?;
            for (arxiv_id, phrase) in &result.paper_links {
                link_stmt.execute(params![arxiv_id, phrase])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn candidate_count(&self) -> Result<usize> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM concept_candidates", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    pub fn msc_grounded_count(&self) -> Result<usize> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM concept_candidates WHERE msc_code IS NOT NULL",
            [],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }

    /// embedding生成の対象となるフレーズ一覧（`extract`で保存済みの候補全て）。
    pub fn list_candidate_phrases(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT phrase FROM concept_candidates")?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(rows)
    }

    /// 抽出済みの候補を`ConceptCandidate`として全件読み出す（クラスタリング
    /// のグラフ構築で doc_freq / msc_code が必要なため）。
    pub fn list_candidates(&self) -> Result<Vec<ConceptCandidate>> {
        let mut stmt = self.conn.prepare(
            "SELECT phrase, doc_freq, mean_score, msc_code, sample_arxiv_ids_json, field_concentration
             FROM concept_candidates",
        )?;
        let rows = stmt
            .query_map([], |r| {
                let phrase: String = r.get(0)?;
                let doc_freq: i64 = r.get(1)?;
                let mean_score: f64 = r.get(2)?;
                let msc_code: Option<String> = r.get(3)?;
                let sample_json: String = r.get(4)?;
                let field_concentration: Option<f32> = r.get(5)?;
                Ok((phrase, doc_freq as usize, mean_score, msc_code, sample_json, field_concentration))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        rows.into_iter()
            .map(|(phrase, doc_freq, mean_score, msc_code, sample_json, field_concentration)| {
                let sample_arxiv_ids: Vec<String> = serde_json::from_str(&sample_json)?;
                Ok(ConceptCandidate {
                    word_count: phrase.split(' ').count(),
                    phrase,
                    doc_freq,
                    mean_score,
                    msc_code,
                    sample_arxiv_ids,
                    field_concentration,
                })
            })
            .collect()
    }

    /// (arxiv_id, phrase) の共起テーブルを全件読み出す（クラスタリングの
    /// 共起シグナル用）。
    pub fn load_paper_concepts(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare("SELECT arxiv_id, phrase FROM paper_concepts")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<(String, String)>>>()?;
        Ok(rows)
    }

    /// 1つのモデルで生成したembeddingを一括で保存する（既存分はUPSERT）。
    /// 1件ずつ自動コミットさせず1トランザクションにまとめる理由は
    /// `mathesis_ingest::store::PaperStore::upsert_all` と同じ
    /// （行数分のfsync待ちを避ける）。
    pub fn save_embeddings(&mut self, model: &str, embeddings: &[(String, Vec<f32>)]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO concept_embeddings (phrase, model, dim, vector)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(phrase) DO UPDATE SET
                    model=excluded.model, dim=excluded.dim, vector=excluded.vector",
            )?;
            for (phrase, vector) in embeddings {
                stmt.execute(params![phrase, model, vector.len() as i64, f32_to_bytes(vector)])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn load_embeddings(&self) -> Result<Vec<(String, Vec<f32>)>> {
        let mut stmt = self.conn.prepare("SELECT phrase, vector FROM concept_embeddings")?;
        let rows = stmt
            .query_map([], |r| {
                let phrase: String = r.get(0)?;
                let bytes: Vec<u8> = r.get(1)?;
                Ok((phrase, bytes_to_f32(&bytes)))
            })?
            .collect::<rusqlite::Result<Vec<(String, Vec<f32>)>>>()?;
        Ok(rows)
    }

    pub fn embedding_count(&self) -> Result<usize> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM concept_embeddings", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    /// `extract` を再実行してConcept候補の集合が変わった後、もはや存在しない
    /// フレーズのembeddingが `concept_embeddings` に孤児として残ることがある
    /// （`extract`は`concept_candidates`/`paper_concepts`だけを置き換え、
    /// embeddingには触れないため）。実際にこの環境でトークナイザのバグを
    /// 直して再抽出した際に、削除されたはずの候補("-1"等)のembeddingが
    /// 22件残っているのを発見した。それを掃除する。
    pub fn prune_orphaned_embeddings(&mut self) -> Result<usize> {
        let n = self.conn.execute(
            "DELETE FROM concept_embeddings
             WHERE phrase NOT IN (SELECT phrase FROM concept_candidates)",
            [],
        )?;
        Ok(n)
    }

    /// クラスタリング対象一式（フレーズ・embedding・doc_freq・msc_code）を
    /// 1回のクエリでまとめて読み出す。`build_concept_graph` の入力を
    /// 組み立てやすくするための便宜メソッド——embeddingが無い候補
    /// （`embed`未実行分）は対象から除く。
    pub fn list_candidates_with_embeddings(&self) -> Result<Vec<(ConceptCandidate, Vec<f32>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.phrase, c.doc_freq, c.mean_score, c.msc_code, c.sample_arxiv_ids_json,
                    c.field_concentration, e.vector
             FROM concept_candidates c
             JOIN concept_embeddings e ON e.phrase = c.phrase",
        )?;
        let rows = stmt
            .query_map([], |r| {
                let phrase: String = r.get(0)?;
                let doc_freq: i64 = r.get(1)?;
                let mean_score: f64 = r.get(2)?;
                let msc_code: Option<String> = r.get(3)?;
                let sample_json: String = r.get(4)?;
                let field_concentration: Option<f32> = r.get(5)?;
                let vector_bytes: Vec<u8> = r.get(6)?;
                Ok((phrase, doc_freq as usize, mean_score, msc_code, sample_json, field_concentration, vector_bytes))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        rows.into_iter()
            .map(|(phrase, doc_freq, mean_score, msc_code, sample_json, field_concentration, vector_bytes)| {
                let sample_arxiv_ids: Vec<String> = serde_json::from_str(&sample_json)?;
                let candidate = ConceptCandidate {
                    word_count: phrase.split(' ').count(),
                    phrase,
                    doc_freq,
                    mean_score,
                    msc_code,
                    sample_arxiv_ids,
                    field_concentration,
                };
                Ok((candidate, bytes_to_f32(&vector_bytes)))
            })
            .collect()
    }

    /// 文脈ベクトル（`context.rs`）を保存する。`concept_embeddings` とは
    /// 別テーブルに置く——同じグラフ構築関数に両方を通して、NMI・純度・
    /// 関連リストの中身で優劣を数字で比べられるようにするため。
    pub fn save_context_vectors(&mut self, vectors: &[(String, Vec<f32>)]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM concept_context_vectors", [])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO concept_context_vectors (phrase, dim, vector) VALUES (?1, ?2, ?3)",
            )?;
            for (phrase, vector) in vectors {
                stmt.execute(params![phrase, vector.len() as i64, f32_to_bytes(vector)])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 零ベクトル（共起の証拠が無かった概念）は返さない——`load_embeddings`
    /// が「embedding済みのものだけ」を返すのと同じ意味にするため。
    pub fn load_context_vectors(&self) -> Result<Vec<(String, Vec<f32>)>> {
        let mut stmt = self.conn.prepare("SELECT phrase, vector FROM concept_context_vectors")?;
        let rows = stmt
            .query_map([], |r| {
                let phrase: String = r.get(0)?;
                let bytes: Vec<u8> = r.get(1)?;
                Ok((phrase, bytes_to_f32(&bytes)))
            })?
            .collect::<rusqlite::Result<Vec<(String, Vec<f32>)>>>()?;
        Ok(rows.into_iter().filter(|(_, v)| v.iter().any(|&x| x != 0.0)).collect())
    }

    pub fn context_vector_count(&self) -> Result<usize> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM concept_context_vectors", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    /// 型付き関係（`relations.rs`）を一括で置き換える。
    pub fn save_relations(&mut self, edges: &[RelationEdge]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM concept_relations", [])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO concept_relations
                    (subject, object, kind, status, confidence, evidence_sentence, evidence_arxiv_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for e in edges {
                stmt.execute(params![
                    e.subject,
                    e.object,
                    relation_kind_to_str(e.kind),
                    relation_status_to_str(e.status),
                    e.confidence,
                    e.evidence_sentence,
                    e.evidence_arxiv_id,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn load_relations(&self) -> Result<Vec<RelationEdge>> {
        let mut stmt = self.conn.prepare(
            "SELECT subject, object, kind, status, confidence, evidence_sentence, evidence_arxiv_id
             FROM concept_relations",
        )?;
        let rows = stmt
            .query_map([], |r| {
                let subject: String = r.get(0)?;
                let object: String = r.get(1)?;
                let kind: String = r.get(2)?;
                let status: String = r.get(3)?;
                let confidence: f32 = r.get(4)?;
                let evidence_sentence: Option<String> = r.get(5)?;
                let evidence_arxiv_id: Option<String> = r.get(6)?;
                Ok((subject, object, kind, status, confidence, evidence_sentence, evidence_arxiv_id))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        rows.into_iter()
            .map(|(subject, object, kind, status, confidence, evidence_sentence, evidence_arxiv_id)| {
                Ok(RelationEdge {
                    subject,
                    object,
                    kind: relation_kind_from_str(&kind)?,
                    status: relation_status_from_str(&status)?,
                    confidence,
                    evidence_sentence,
                    evidence_arxiv_id,
                })
            })
            .collect()
    }

    /// `list_candidates_with_embeddings` の文脈ベクトル版。
    pub fn list_candidates_with_context_vectors(&self) -> Result<Vec<(ConceptCandidate, Vec<f32>)>> {
        self.list_candidates_joined("concept_context_vectors")
    }

    fn list_candidates_joined(&self, vector_table: &str) -> Result<Vec<(ConceptCandidate, Vec<f32>)>> {
        let sql = format!(
            "SELECT c.phrase, c.doc_freq, c.mean_score, c.msc_code, c.sample_arxiv_ids_json,
                    c.field_concentration, v.vector
             FROM concept_candidates c
             JOIN {vector_table} v ON v.phrase = c.phrase"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map([], |r| {
                let phrase: String = r.get(0)?;
                let doc_freq: i64 = r.get(1)?;
                let mean_score: f64 = r.get(2)?;
                let msc_code: Option<String> = r.get(3)?;
                let sample_json: String = r.get(4)?;
                let field_concentration: Option<f32> = r.get(5)?;
                let vector_bytes: Vec<u8> = r.get(6)?;
                Ok((phrase, doc_freq as usize, mean_score, msc_code, sample_json, field_concentration, vector_bytes))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        rows.into_iter()
            .map(|(phrase, doc_freq, mean_score, msc_code, sample_json, field_concentration, vector_bytes)| {
                let sample_arxiv_ids: Vec<String> = serde_json::from_str(&sample_json)?;
                let candidate = ConceptCandidate {
                    word_count: phrase.split(' ').count(),
                    phrase,
                    doc_freq,
                    mean_score,
                    msc_code,
                    sample_arxiv_ids,
                    field_concentration,
                };
                Ok((candidate, bytes_to_f32(&vector_bytes)))
            })
            .collect()
    }

    /// 論文のメタデータ。`mathesis-ingest` が作った `papers` テーブルを読む
    /// （同じDBファイルに相乗りしている）。
    ///
    /// これまでタクソノミー側は論文を**IDの文字列としてしか**扱っておらず、
    /// Web版へもフレーズごとの`sample_arxiv_ids`（最大3件の生ID）しか
    /// 出していなかった——利用者はタイトルすら見られず、サイトの外へ
    /// 出るしかなかった。検索結果の単位を「フレーズ」から「文書」へ
    /// 変えるために、ここで初めて論文本体を読む。
    pub fn load_paper_metadata(&self) -> Result<Vec<PaperMeta>> {
        // `papers` は `mathesis-ingest` が作るテーブルなので、
        // タクソノミー側だけで作られたDBには存在しない。無い場合は
        // 「論文0件」として扱う（エラーにすると、論文を取り込む前に
        // extractを試す使い方が全部落ちる）。
        let has_papers: bool = self.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='papers'",
            [],
            |r| r.get::<_, i64>(0),
        )? > 0;
        if !has_papers {
            return Ok(Vec::new());
        }
        let mut stmt = self
            .conn
            .prepare("SELECT arxiv_id, title, submitted, categories_json FROM papers")?;
        let rows = stmt
            .query_map([], |r| {
                let arxiv_id: String = r.get(0)?;
                let title: String = r.get(1)?;
                let submitted: String = r.get(2)?;
                let categories_json: String = r.get(3)?;
                Ok((arxiv_id, title, submitted, categories_json))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows
            .into_iter()
            .map(|(arxiv_id, title, submitted, categories_json)| PaperMeta {
                arxiv_id,
                title: normalize_whitespace(&title),
                year: submitted_year(&submitted),
                categories: serde_json::from_str(&categories_json).unwrap_or_default(),
            })
            .collect())
    }

    /// 既存のクラスタ割り当てを一括で置き換える。
    pub fn save_clusters(&mut self, assignments: &[(String, usize)]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM concept_clusters", [])?;
        {
            let mut stmt =
                tx.prepare("INSERT INTO concept_clusters (phrase, cluster_id) VALUES (?1, ?2)")?;
            for (phrase, cluster_id) in assignments {
                stmt.execute(params![phrase, *cluster_id as i64])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn load_clusters(&self) -> Result<Vec<(String, usize)>> {
        let mut stmt = self.conn.prepare("SELECT phrase, cluster_id FROM concept_clusters")?;
        let rows = stmt
            .query_map([], |r| {
                let phrase: String = r.get(0)?;
                let cluster_id: i64 = r.get(1)?;
                Ok((phrase, cluster_id as usize))
            })?
            .collect::<rusqlite::Result<Vec<(String, usize)>>>()?;
        Ok(rows)
    }

    /// クラスタ単位のMSC alignment結果と、それに基づく推定コードの
    /// 伝播結果を一括で置き換える。
    pub fn save_alignment(&mut self, alignments: &[ClusterAlignment], inferred: &[InferredAlignment]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM cluster_alignments", [])?;
        tx.execute("DELETE FROM phrase_msc_inferred", [])?;
        {
            let mut align_stmt = tx.prepare(
                "INSERT INTO cluster_alignments (cluster_id, size, grounded_count, dominant_code, dominant_name, confidence)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for a in alignments {
                align_stmt.execute(params![
                    a.cluster_id as i64,
                    a.size as i64,
                    a.grounded_count as i64,
                    a.dominant_code,
                    a.dominant_name,
                    a.confidence,
                ])?;
            }

            let mut infer_stmt = tx.prepare(
                "INSERT INTO phrase_msc_inferred (phrase, cluster_id, inferred_code, inferred_name, confidence)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for i in inferred {
                infer_stmt.execute(params![i.phrase, i.cluster_id as i64, i.inferred_code, i.inferred_name, i.confidence])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn load_cluster_alignments(&self) -> Result<Vec<ClusterAlignment>> {
        let mut stmt = self.conn.prepare(
            "SELECT cluster_id, size, grounded_count, dominant_code, dominant_name, confidence FROM cluster_alignments",
        )?;
        let rows = stmt
            .query_map([], |r| {
                let cluster_id: i64 = r.get(0)?;
                let size: i64 = r.get(1)?;
                let grounded_count: i64 = r.get(2)?;
                let dominant_code: Option<String> = r.get(3)?;
                let dominant_name: Option<String> = r.get(4)?;
                let confidence: f32 = r.get(5)?;
                Ok(ClusterAlignment {
                    cluster_id: cluster_id as usize,
                    size: size as usize,
                    grounded_count: grounded_count as usize,
                    dominant_code,
                    dominant_name,
                    confidence,
                })
            })?
            .collect::<rusqlite::Result<Vec<ClusterAlignment>>>()?;
        Ok(rows)
    }

    pub fn inferred_alignment_count(&self) -> Result<usize> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM phrase_msc_inferred", [], |r| r.get(0))?;
        Ok(n as usize)
    }
}

/// `RelationKind`/`RelationStatus`はSQLiteに人間が読めるTEXTとして持たせる
/// （整数コードにすると、DBを直接覗いたときに意味が分からない）。
fn relation_kind_to_str(kind: RelationKind) -> &'static str {
    match kind {
        RelationKind::SpecializationOf => "specialization_of",
        RelationKind::EquivalentTo => "equivalent_to",
    }
}

fn relation_kind_from_str(s: &str) -> Result<RelationKind> {
    match s {
        "specialization_of" => Ok(RelationKind::SpecializationOf),
        "equivalent_to" => Ok(RelationKind::EquivalentTo),
        other => Err(anyhow!("未知のRelationKind: {other}")),
    }
}

fn relation_status_to_str(status: RelationStatus) -> &'static str {
    match status {
        RelationStatus::Proposed => "proposed",
        RelationStatus::Grounded => "grounded",
        RelationStatus::Confirmed => "confirmed",
    }
}

fn relation_status_from_str(s: &str) -> Result<RelationStatus> {
    match s {
        "proposed" => Ok(RelationStatus::Proposed),
        "grounded" => Ok(RelationStatus::Grounded),
        "confirmed" => Ok(RelationStatus::Confirmed),
        other => Err(anyhow!("未知のRelationStatus: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::concepts::ConceptCandidate;

    fn sample_result() -> ExtractionResult {
        ExtractionResult {
            candidates: vec![
                ConceptCandidate {
                    phrase: "kähler manifolds".to_string(),
                    word_count: 2,
                    doc_freq: 2,
                    mean_score: 6.5,
                    msc_code: Some("32Q15".to_string()),
                    sample_arxiv_ids: vec!["1".to_string(), "2".to_string()],
                    field_concentration: Some(0.87),
                },
                ConceptCandidate {
                    phrase: "some novel gadget".to_string(),
                    word_count: 3,
                    doc_freq: 2,
                    mean_score: 4.0,
                    msc_code: None,
                    sample_arxiv_ids: vec!["1".to_string()],
                    // 分野ラベル付き論文が少なく集中度を出せなかった候補。
                    field_concentration: None,
                },
            ],
            paper_links: vec![
                ("1".to_string(), "kähler manifolds".to_string()),
                ("2".to_string(), "kähler manifolds".to_string()),
                ("1".to_string(), "some novel gadget".to_string()),
            ],
            dropped_as_boilerplate: vec![],
        }
    }

    #[test]
    fn field_concentration_round_trips_including_the_unscored_case() {
        let mut store = TaxonomyStore::open_in_memory().unwrap();
        store.replace_all(&sample_result()).unwrap();
        let candidates = store.list_candidates().unwrap();

        let scored = candidates.iter().find(|c| c.phrase == "kähler manifolds").unwrap();
        assert!((scored.field_concentration.unwrap() - 0.87).abs() < 1e-6);
        let unscored = candidates.iter().find(|c| c.phrase == "some novel gadget").unwrap();
        assert_eq!(unscored.field_concentration, None, "NULL must come back as None, not 0.0");
    }

    /// `field_concentration` はこの列が無い頃のDBに後から足したもの。
    /// 既存のDBを開いたときに `ALTER TABLE` で追加され、以降の読み書きが
    /// 成立することを確認する（この移行が無いと、旧DBに対する
    /// `list_candidates` が "no such column" で落ちる）。
    #[test]
    fn opening_a_db_created_before_the_field_concentration_column_migrates_it() {
        let dir = std::env::temp_dir().join(format!("mathesis-migration-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("legacy.db");
        let _ = std::fs::remove_file(&path);

        // この列が無かった頃のスキーマをそのまま作る。
        let legacy = Connection::open(&path).unwrap();
        legacy
            .execute_batch(
                "CREATE TABLE concept_candidates (
                    phrase          TEXT PRIMARY KEY,
                    doc_freq        INTEGER NOT NULL,
                    mean_score      REAL NOT NULL,
                    msc_code        TEXT,
                    sample_arxiv_ids_json TEXT NOT NULL
                );
                INSERT INTO concept_candidates VALUES ('old phrase', 7, 1.5, NULL, '[]');",
            )
            .unwrap();
        drop(legacy);

        let store = TaxonomyStore::open(&path).unwrap();
        let candidates = store.list_candidates().unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].phrase, "old phrase");
        assert_eq!(candidates[0].field_concentration, None);

        // 二度開いても ALTER TABLE が重複して走らないこと。
        drop(store);
        assert!(TaxonomyStore::open(&path).is_ok());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn replace_all_persists_candidates_and_links() {
        let mut store = TaxonomyStore::open_in_memory().unwrap();
        store.replace_all(&sample_result()).unwrap();
        assert_eq!(store.candidate_count().unwrap(), 2);
        assert_eq!(store.msc_grounded_count().unwrap(), 1);
    }

    #[test]
    fn replace_all_clears_previous_extraction_first() {
        let mut store = TaxonomyStore::open_in_memory().unwrap();
        store.replace_all(&sample_result()).unwrap();

        let smaller = ExtractionResult {
            candidates: vec![sample_result().candidates.into_iter().next().unwrap()],
            paper_links: vec![("1".to_string(), "kähler manifolds".to_string())],
            dropped_as_boilerplate: vec![],
        };
        store.replace_all(&smaller).unwrap();
        assert_eq!(store.candidate_count().unwrap(), 1, "re-extraction must replace, not accumulate");
    }

    #[test]
    fn list_candidate_phrases_returns_all_extracted_phrases() {
        let mut store = TaxonomyStore::open_in_memory().unwrap();
        store.replace_all(&sample_result()).unwrap();
        let mut phrases = store.list_candidate_phrases().unwrap();
        phrases.sort();
        assert_eq!(phrases, vec!["kähler manifolds".to_string(), "some novel gadget".to_string()]);
    }

    #[test]
    fn list_candidates_round_trips_full_records() {
        let mut store = TaxonomyStore::open_in_memory().unwrap();
        store.replace_all(&sample_result()).unwrap();
        let mut candidates = store.list_candidates().unwrap();
        candidates.sort_by(|a, b| a.phrase.cmp(&b.phrase));

        let mut expected = sample_result().candidates;
        expected.sort_by(|a, b| a.phrase.cmp(&b.phrase));
        assert_eq!(candidates, expected);
    }

    #[test]
    fn load_paper_concepts_returns_all_links() {
        let mut store = TaxonomyStore::open_in_memory().unwrap();
        store.replace_all(&sample_result()).unwrap();
        let mut links = store.load_paper_concepts().unwrap();
        links.sort();
        let mut expected = sample_result().paper_links;
        expected.sort();
        assert_eq!(links, expected);
    }

    #[test]
    fn save_and_load_embeddings_round_trips() {
        let mut store = TaxonomyStore::open_in_memory().unwrap();
        let embeddings = vec![
            ("kähler manifolds".to_string(), vec![0.1f32, 0.2, 0.3]),
            ("some novel gadget".to_string(), vec![-0.5f32, 0.0, 1.5]),
        ];
        store.save_embeddings("all-minilm", &embeddings).unwrap();

        assert_eq!(store.embedding_count().unwrap(), 2);
        let mut loaded = store.load_embeddings().unwrap();
        loaded.sort_by(|a, b| a.0.cmp(&b.0));
        let mut expected = embeddings;
        expected.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(loaded, expected);
    }

    #[test]
    fn save_embeddings_upserts_on_re_run() {
        let mut store = TaxonomyStore::open_in_memory().unwrap();
        store
            .save_embeddings("all-minilm", &[("phrase".to_string(), vec![1.0, 0.0])])
            .unwrap();
        store
            .save_embeddings("all-minilm", &[("phrase".to_string(), vec![0.0, 1.0])])
            .unwrap();

        assert_eq!(store.embedding_count().unwrap(), 1, "re-embedding the same phrase must not duplicate rows");
        let loaded = store.load_embeddings().unwrap();
        assert_eq!(loaded, vec![("phrase".to_string(), vec![0.0, 1.0])]);
    }

    #[test]
    fn prune_orphaned_embeddings_removes_rows_for_phrases_no_longer_extracted() {
        let mut store = TaxonomyStore::open_in_memory().unwrap();
        store.replace_all(&sample_result()).unwrap();
        store
            .save_embeddings(
                "all-minilm",
                &[
                    ("kähler manifolds".to_string(), vec![0.1, 0.2]),
                    ("some novel gadget".to_string(), vec![0.3, 0.4]),
                    ("a phrase from a stale extraction".to_string(), vec![0.5, 0.6]),
                ],
            )
            .unwrap();
        assert_eq!(store.embedding_count().unwrap(), 3);

        let pruned = store.prune_orphaned_embeddings().unwrap();
        assert_eq!(pruned, 1);
        assert_eq!(store.embedding_count().unwrap(), 2);
        let remaining: Vec<String> = store.load_embeddings().unwrap().into_iter().map(|(p, _)| p).collect();
        assert!(!remaining.contains(&"a phrase from a stale extraction".to_string()));
    }

    #[test]
    fn list_candidates_with_embeddings_only_returns_embedded_candidates() {
        let mut store = TaxonomyStore::open_in_memory().unwrap();
        store.replace_all(&sample_result()).unwrap();
        // "some novel gadget" は埋め込みを与えない——結果に出てはいけない。
        store
            .save_embeddings("all-minilm", &[("kähler manifolds".to_string(), vec![0.1, 0.2, 0.3])])
            .unwrap();

        let joined = store.list_candidates_with_embeddings().unwrap();
        assert_eq!(joined.len(), 1);
        assert_eq!(joined[0].0.phrase, "kähler manifolds");
        assert_eq!(joined[0].0.doc_freq, 2);
        assert_eq!(joined[0].1, vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn save_and_load_clusters_round_trips() {
        let mut store = TaxonomyStore::open_in_memory().unwrap();
        store.replace_all(&sample_result()).unwrap();
        let assignments = vec![("kähler manifolds".to_string(), 0), ("some novel gadget".to_string(), 1)];
        store.save_clusters(&assignments).unwrap();

        let mut loaded = store.load_clusters().unwrap();
        loaded.sort();
        let mut expected = assignments;
        expected.sort();
        assert_eq!(loaded, expected);
    }

    #[test]
    fn save_clusters_replaces_previous_assignment() {
        let mut store = TaxonomyStore::open_in_memory().unwrap();
        store.replace_all(&sample_result()).unwrap();
        store
            .save_clusters(&[("kähler manifolds".to_string(), 0), ("some novel gadget".to_string(), 0)])
            .unwrap();
        store.save_clusters(&[("kähler manifolds".to_string(), 7)]).unwrap();

        let loaded = store.load_clusters().unwrap();
        assert_eq!(loaded, vec![("kähler manifolds".to_string(), 7)], "re-clustering must replace, not accumulate");
    }

    #[test]
    fn save_and_load_cluster_alignments_round_trips() {
        let mut store = TaxonomyStore::open_in_memory().unwrap();
        let alignments = vec![
            ClusterAlignment {
                cluster_id: 0,
                size: 3,
                grounded_count: 2,
                dominant_code: Some("18Axx".to_string()),
                dominant_name: Some("General theory of categories and functors".to_string()),
                confidence: 1.0,
            },
            ClusterAlignment { cluster_id: 1, size: 2, grounded_count: 0, dominant_code: None, dominant_name: None, confidence: 0.0 },
        ];
        let inferred = vec![InferredAlignment {
            phrase: "some ungrounded term".to_string(),
            cluster_id: 0,
            inferred_code: "18Axx".to_string(),
            inferred_name: "General theory of categories and functors".to_string(),
            confidence: 1.0,
        }];
        store.save_alignment(&alignments, &inferred).unwrap();

        let mut loaded = store.load_cluster_alignments().unwrap();
        loaded.sort_by_key(|a| a.cluster_id);
        assert_eq!(loaded, alignments);
        assert_eq!(store.inferred_alignment_count().unwrap(), 1);
    }

    #[test]
    fn save_alignment_replaces_previous_results() {
        let mut store = TaxonomyStore::open_in_memory().unwrap();
        let first = vec![ClusterAlignment { cluster_id: 0, size: 1, grounded_count: 0, dominant_code: None, dominant_name: None, confidence: 0.0 }];
        store.save_alignment(&first, &[]).unwrap();
        assert_eq!(store.load_cluster_alignments().unwrap().len(), 1);

        let second: Vec<ClusterAlignment> = vec![];
        store.save_alignment(&second, &[]).unwrap();
        assert_eq!(store.load_cluster_alignments().unwrap().len(), 0, "re-running align must replace, not accumulate");
    }

    fn sample_relation(evidence: bool) -> RelationEdge {
        RelationEdge {
            subject: "elliptic curves".to_string(),
            object: "abelian varieties".to_string(),
            kind: RelationKind::SpecializationOf,
            status: if evidence { RelationStatus::Confirmed } else { RelationStatus::Proposed },
            confidence: 0.83,
            evidence_sentence: evidence.then(|| "An elliptic curve is a special case of an abelian variety.".to_string()),
            evidence_arxiv_id: evidence.then(|| "math/0001".to_string()),
        }
    }

    #[test]
    fn save_and_load_relations_round_trips_with_and_without_evidence() {
        let mut store = TaxonomyStore::open_in_memory().unwrap();
        let edges = vec![
            sample_relation(true),
            RelationEdge {
                subject: "brownian motion".to_string(),
                object: "wiener process".to_string(),
                kind: RelationKind::EquivalentTo,
                ..sample_relation(false)
            },
        ];
        store.save_relations(&edges).unwrap();
        let mut loaded = store.load_relations().unwrap();
        loaded.sort_by(|a, b| a.subject.cmp(&b.subject));
        let mut expected = edges;
        expected.sort_by(|a, b| a.subject.cmp(&b.subject));
        assert_eq!(loaded, expected);
    }

    #[test]
    fn save_relations_replaces_previous_results() {
        let mut store = TaxonomyStore::open_in_memory().unwrap();
        store.save_relations(&[sample_relation(true)]).unwrap();
        assert_eq!(store.load_relations().unwrap().len(), 1);
        store.save_relations(&[]).unwrap();
        assert_eq!(store.load_relations().unwrap().len(), 0, "再実行は蓄積ではなく置き換え");
    }
}

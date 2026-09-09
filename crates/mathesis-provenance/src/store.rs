//! SQLiteを永続化バックエンドとする証拠層ストア。
//!
//! `mathesis-graph::store::GraphStore`と同じ方針: 1本の`SCHEMA`定数
//! （`CREATE TABLE IF NOT EXISTS`の列挙）を`open`のたびに冪等に流し込む。
//! マイグレーションファイルは無い——追加のみ、既存列の変更はまだ起きていない。

use rusqlite::Connection;
use std::path::Path;

pub type Result<T> = rusqlite::Result<T>;

pub struct ProvenanceStore {
    pub(crate) conn: Connection,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS releases (
    id                 INTEGER PRIMARY KEY,
    tag                TEXT NOT NULL UNIQUE,
    git_commit         TEXT,
    generated_at_unix  INTEGER NOT NULL,
    notes              TEXT
);

-- ARCHITECTURE_NEXT.md §5.1。`(provider, provider_id, provider_revision)`が
-- 同一ソースの同一性キー——同じarXiv論文が`mathesis-graph`側の判断由来と
-- taxonomy側の根拠文の両方から参照されても、SourceRecordは1行に集約される。
-- `provider_revision`を含めるのは外部レビュー(2026-09-05)の指摘への対応:
-- 以前は(provider, provider_id)だけだったため、同じ論文を別リビジョンで
-- 再取得しても古い行が黙って再利用され、「不変なsource envelope」という
-- ARCHITECTURE_NEXT.md §5.1の前提と矛盾していた。SQLiteはUNIQUE制約で
-- NULL同士を別物として扱う(複数のNULL revisionが並存しうる)ため、実質的な
-- 重複排除は`source_record.rs`の`IS`を使ったSELECTが担う——この制約は
-- リビジョンが実際に埋まっている場合の保険。
CREATE TABLE IF NOT EXISTS source_records (
    id                 INTEGER PRIMARY KEY,
    provider           TEXT NOT NULL,
    provider_id        TEXT NOT NULL,
    provider_revision  TEXT,
    retrieved_at_unix  INTEGER,
    content_hash       TEXT,
    licence            TEXT,
    attribution        TEXT,
    raw_payload_uri    TEXT,
    adapter_name       TEXT NOT NULL,
    adapter_version    TEXT NOT NULL,
    parser_version     TEXT,
    UNIQUE(provider, provider_id, provider_revision)
);

-- ARCHITECTURE_NEXT.md §5.3。`subject_ref`/`object_ref`はタグ付き文字列
-- （`docs/DATA_DICTIONARY.md`設計判断2、model.rs参照）。`legacy_ref`は
-- レガシースナップショットアダプタの冪等な再実行キー——(release_id, legacy_ref)
-- の組が同じなら再実行しても行は増えない。
CREATE TABLE IF NOT EXISTS relation_assertions (
    id                  INTEGER PRIMARY KEY,
    subject_ref         TEXT NOT NULL,
    subject_entity_id   INTEGER REFERENCES entities(id),
    predicate           TEXT NOT NULL,
    object_ref          TEXT NOT NULL,
    object_entity_id    INTEGER REFERENCES entities(id),
    epistemic_state     TEXT NOT NULL,
    score               REAL,
    policy_version      TEXT,
    created_by_run_id   TEXT,
    supersedes_id       INTEGER REFERENCES relation_assertions(id),
    release_id          INTEGER NOT NULL REFERENCES releases(id),
    legacy_ref          TEXT,
    UNIQUE(release_id, legacy_ref)
);
CREATE INDEX IF NOT EXISTS idx_assertions_subject ON relation_assertions(subject_ref);
CREATE INDEX IF NOT EXISTS idx_assertions_object ON relation_assertions(object_ref);
CREATE INDEX IF NOT EXISTS idx_assertions_predicate_state ON relation_assertions(predicate, epistemic_state);

-- ARCHITECTURE_NEXT.md §5.3。「手書きのreviewer_noteを除き、全ての assertion に
-- Evidenceが必須」——レガシーアダプタはこれを厳守する（`legacy_adapter.rs`参照）。
-- `metric_name`/`metric_value`: 較正されていない検出器の生スコア（例:
-- taxonomy側のdistributional detectorが出す"invCL"）専用の置き場。
-- `relation_assertions.score`は較正済み・比較可能な値専用に空けておく
-- （`docs/DATA_DICTIONARY.md`「Evidence multiplicity, not epistemic-state
-- inflation」参照）。
CREATE TABLE IF NOT EXISTS evidence (
    id                    INTEGER PRIMARY KEY,
    assertion_id          INTEGER NOT NULL REFERENCES relation_assertions(id),
    source_record_id      INTEGER NOT NULL REFERENCES source_records(id),
    locator               TEXT,
    evidence_kind         TEXT NOT NULL,
    extractor_or_model    TEXT,
    version               TEXT,
    input_hash            TEXT,
    output_hash           TEXT,
    metric_name           TEXT,
    metric_value          REAL
);
CREATE INDEX IF NOT EXISTS idx_evidence_assertion ON evidence(assertion_id);

CREATE TABLE IF NOT EXISTS review_decisions (
    id                 INTEGER PRIMARY KEY,
    assertion_id       INTEGER NOT NULL REFERENCES relation_assertions(id),
    decision           TEXT NOT NULL,
    reviewer_id        TEXT,
    scope              TEXT,
    rationale          TEXT,
    decided_at_unix    INTEGER NOT NULL,
    dataset_version    TEXT
);
CREATE INDEX IF NOT EXISTS idx_review_decisions_assertion ON review_decisions(assertion_id);

-- P3, Increment 1（ARCHITECTURE_NEXT.md §5.2、docs/P3_STATUS.md）。
-- `entities`は実世界の対象（judgment/concept/paper）1件につき1行。
-- `subject_ref`/`object_ref`が使う`"kind:id"`タグ付き文字列とは違う
-- 独立した主キー空間で、これが将来の本物のFK移行の土台になる。
CREATE TABLE IF NOT EXISTS entities (
    id                INTEGER PRIMARY KEY,
    kind              TEXT NOT NULL,
    display_label     TEXT NOT NULL,
    source_record_id  INTEGER REFERENCES source_records(id)
);
CREATE INDEX IF NOT EXISTS idx_entities_kind ON entities(kind);

-- 「このタグ付き文字列は、どの実在エンティティを指すか」の解決表。
-- concept は表記ゆれ（alias）ごとに複数行を持ちうる——
-- "kahler manifold"と"kahler manifolds"のような別表記が同じentity_idへ
-- 解決されるようにするため（judgment/paperは常に1entity=1行）。
CREATE TABLE IF NOT EXISTS entity_refs (
    ref_string  TEXT PRIMARY KEY,
    entity_id   INTEGER NOT NULL REFERENCES entities(id)
);
CREATE INDEX IF NOT EXISTS idx_entity_refs_entity ON entity_refs(entity_id);

CREATE TABLE IF NOT EXISTS entity_labels (
    entity_id   INTEGER PRIMARY KEY REFERENCES entities(id),
    origin      TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS catalog_metadata (
    id                          INTEGER PRIMARY KEY CHECK (id = 1),
    schema_version              INTEGER NOT NULL,
    build_version               TEXT NOT NULL,
    entity_resolution_version   TEXT NOT NULL,
    graph_input_sha256          TEXT NOT NULL,
    taxonomy_input_sha256       TEXT NOT NULL,
    entity_count                INTEGER NOT NULL,
    alias_count                 INTEGER NOT NULL
);
"#;

impl ProvenanceStore {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(SCHEMA)?;
        Self::ensure_assertion_entity_columns(&conn)?;
        Self::ensure_p6_1_columns(&conn)?;
        Self::ensure_p6_3_columns(&conn)?;
        Self::ensure_p7_4_columns(&conn)?;
        Ok(ProvenanceStore { conn })
    }

    /// P7.4（`docs/P7_4_STATUS.md`）: `ensure_p6_1_columns`と同じパターン。
    /// 「このevidenceが外部データセット由来として、どう分類されるか」
    /// (`external_literal_dependency`/`external_typeclass_hierarchy`)——
    /// `dependency_origin`(生のedge_type: sig/proof/def/...)とは別軸。
    /// Mathesis自身の証拠は常に`NULL`のまま。
    fn ensure_p7_4_columns(conn: &Connection) -> Result<()> {
        let evidence_columns: std::collections::HashSet<String> = conn
            .prepare("PRAGMA table_info(evidence)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<_>>()?;
        if !evidence_columns.contains("external_classification") {
            conn.execute("ALTER TABLE evidence ADD COLUMN external_classification TEXT", [])?;
        }
        Ok(())
    }

    fn ensure_assertion_entity_columns(conn: &Connection) -> Result<()> {
        let columns: std::collections::HashSet<String> = conn
            .prepare("PRAGMA table_info(relation_assertions)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<_>>()?;
        if !columns.contains("subject_entity_id") {
            conn.execute("ALTER TABLE relation_assertions ADD COLUMN subject_entity_id INTEGER REFERENCES entities(id)", [])?;
        }
        if !columns.contains("object_entity_id") {
            conn.execute("ALTER TABLE relation_assertions ADD COLUMN object_entity_id INTEGER REFERENCES entities(id)", [])?;
        }
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_assertions_subject_entity ON relation_assertions(subject_entity_id);
             CREATE INDEX IF NOT EXISTS idx_assertions_object_entity ON relation_assertions(object_entity_id);",
        )?;
        Ok(())
    }

    /// P6.1（`docs/LEAN_DEPENDENCY_POLICY.md`）: `ensure_assertion_entity_columns`と
    /// 同じ「追加のみ・冪等ALTER」パターン。`evidence.dependency_origin`
    /// (type/body/both)と`source_records.reproducibility_json`
    /// (Lean/mathlib版・抽出器版・フィルタポリシー版・raw/normalizedハッシュの
    /// 小さなJSON blob)を追加する。
    fn ensure_p6_1_columns(conn: &Connection) -> Result<()> {
        let evidence_columns: std::collections::HashSet<String> = conn
            .prepare("PRAGMA table_info(evidence)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<_>>()?;
        if !evidence_columns.contains("dependency_origin") {
            conn.execute("ALTER TABLE evidence ADD COLUMN dependency_origin TEXT", [])?;
        }
        let source_record_columns: std::collections::HashSet<String> = conn
            .prepare("PRAGMA table_info(source_records)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<_>>()?;
        if !source_record_columns.contains("reproducibility_json") {
            conn.execute("ALTER TABLE source_records ADD COLUMN reproducibility_json TEXT", [])?;
        }
        Ok(())
    }

    /// P6.3（`docs/P6_3_STATUS.md`）: `ensure_p6_1_columns`と同じ「追加のみ・
    /// 冪等ALTER」パターン。本人確認済みレビューに「どんな資格で承認したか」
    /// (`authorization_level`)・「いつ失効するか」(`expires_at_unix`)・
    /// 「どの過去の判断を置き換えるか」(`supersedes_review_id`)を追加する。
    fn ensure_p6_3_columns(conn: &Connection) -> Result<()> {
        let review_columns: std::collections::HashSet<String> = conn
            .prepare("PRAGMA table_info(review_decisions)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<_>>()?;
        if !review_columns.contains("authorization_level") {
            conn.execute("ALTER TABLE review_decisions ADD COLUMN authorization_level TEXT", [])?;
        }
        if !review_columns.contains("expires_at_unix") {
            conn.execute("ALTER TABLE review_decisions ADD COLUMN expires_at_unix INTEGER", [])?;
        }
        if !review_columns.contains("supersedes_review_id") {
            conn.execute("ALTER TABLE review_decisions ADD COLUMN supersedes_review_id INTEGER REFERENCES review_decisions(id)", [])?;
        }
        Ok(())
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(SCHEMA)?;
        Self::ensure_assertion_entity_columns(&conn)?;
        Self::ensure_p6_1_columns(&conn)?;
        Self::ensure_p6_3_columns(&conn)?;
        Self::ensure_p7_4_columns(&conn)?;
        Ok(ProvenanceStore { conn })
    }

    /// `mathesis_graph::GraphStore::transaction`と同じ方針: 個々の
    /// insert系メソッドは1呼び出し=1トランザクション(SQLiteの自動コミット)
    /// なので、数千行を素朴にループすると行数分のfsync待ちが発生する
    /// （store.rsのdoc commentに実測記録あり）。呼び出し側はレガシー
    /// アダプタのような一括インポートをこれで包むことでコミットを1回に
    /// 減らせる。
    pub fn transaction<F, T, E>(&self, f: F) -> std::result::Result<T, E>
    where
        F: FnOnce() -> std::result::Result<T, E>,
        E: From<rusqlite::Error>,
    {
        self.conn.execute_batch("BEGIN")?;
        match f() {
            Ok(v) => {
                self.conn.execute_batch("COMMIT")?;
                Ok(v)
            }
            Err(e) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }
}

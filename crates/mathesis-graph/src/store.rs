//! SQLite を永続化バックエンドとするグラフストア。
//!
//! アーキテクチャ設計上の本命ストレージは Neo4j/Memgraph 等のグラフDBだが、
//! フェーズ2では判断間の射を `morphisms` テーブルに載せる。受理済み同値は
//! `judgments.representative_id` 列に縮約する（フェーズ3アーキテクチャレビューの
//! 指摘どおり、判断ごとに別テーブルでクラスIDを管理する設計は大規模で
//! `rebuild_quotient()` のたびに全件書き換えが必要になり不利なため、判断ノード
//! 自体が代表元を直接持つ設計に切り替えている。代表元が自分自身の場合は
//! NULL を入れ、「代表元あり」の判定・検索をインデックス付きの1列で済ませる）。
//! `GraphStore` の公開 API をノード/エッジ中心に保つことで、フェーズ5でグラフDBへ
//! 載せ替える際もこのクレートの利用側（インポーター等）への影響を局所化できる
//! ようにしている。
//!
//! 式は `canonical_hash` で重複排除（インターン）する。α同値な式（例: `∀ x, x^2≥0`
//! と `∀ y, y^2≥0`）は同一の式ノードに解決されるため、これは Blob ストレージ＋
//! ハッシュインデックスによる証明項サブツリー再利用（DAG圧縮）を、層1の式ノードに
//! 対して先取りして適用したものにあたる。
//!
//! 頻繁に呼ばれるクエリは `Connection::prepare_cached` で SQL のパース・最適化コストを
//! 呼び出し間で使い回す（フェーズ3レビュー問題4「Prepared Statements のキャッシング欠如」
//! への対応。rusqlite 組み込みの LRU キャッシュを使うため、自前でキャッシュ層を
//! 実装する必要はない）。

use crate::heuristics::JudgmentLiteCache;
use crate::model::{
    ExprId, Hypothesis, JudgmentId, JudgmentKind, JudgmentRecord, NewJudgment, ParseStatus,
};
use crate::morphism::EdgeStatus;
use crate::proof::ProofTerm;
use crate::quotient::QuotientCache;
use mathesis_ast::Expr;
use rusqlite::{params, Connection, OptionalExtension};
use std::cell::RefCell;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub type Result<T> = rusqlite::Result<T>;

pub struct GraphStore {
    pub(crate) conn: Connection,
    /// フェーズ3.2「Union-Find永続化」: 受理済み同値エッジから作る商グラフの
    /// Union-Find をプロセス内に保持し、新規に受理された同値エッジの分だけ
    /// 差分更新する（`edges.rs` の `sync_quotient`/`materialize_quotient` 参照）。
    /// `&self` の各メソッドから更新できるよう `RefCell` に包んでいる——この型は
    /// 元々 `rusqlite::Connection` を介して単一スレッドでの逐次利用しか想定して
    /// いないため、`Mutex` ではなく `RefCell` で十分。
    pub(crate) quotient_cache: RefCell<QuotientCache>,
    /// フェーズ3.2「インクリメンタル提案」: `propose_morphisms()` が使う
    /// `JudgmentLite` 一覧をプロセス内に蓄積し、呼び出しのたびに新規判断だけを
    /// SQLite から読み足す（`edges.rs` の `sync_judgment_lites` 参照）。
    pub(crate) heuristic_cache: RefCell<JudgmentLiteCache>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS expressions (
    id              INTEGER PRIMARY KEY,
    canonical_hash  TEXT NOT NULL UNIQUE,
    ast_json        TEXT NOT NULL,
    display         TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_expressions_hash ON expressions(canonical_hash);

-- Phase 9: 判断ノードの由来となった論文（arXiv id等）。詳細は`paper.rs`参照。
-- `judgments.source_paper_id`が参照するため、`judgments`より先に作る。
CREATE TABLE IF NOT EXISTS papers (
    id        INTEGER PRIMARY KEY,
    arxiv_id  TEXT NOT NULL UNIQUE,
    title     TEXT
);

CREATE TABLE IF NOT EXISTS judgments (
    id                  INTEGER PRIMARY KEY,
    kind                TEXT NOT NULL,
    name                TEXT,
    context_json        TEXT NOT NULL,
    statement_expr_id   INTEGER NOT NULL REFERENCES expressions(id),
    definition_body_raw TEXT,
    source_file         TEXT NOT NULL,
    source_line         INTEGER NOT NULL,
    raw_text            TEXT NOT NULL,
    parse_status        TEXT NOT NULL,
    representative_id   INTEGER REFERENCES judgments(id),
    source_paper_id     INTEGER REFERENCES papers(id)
);
CREATE INDEX IF NOT EXISTS idx_judgments_statement ON judgments(statement_expr_id);
CREATE INDEX IF NOT EXISTS idx_judgments_name ON judgments(name);
CREATE INDEX IF NOT EXISTS idx_judgments_representative ON judgments(representative_id);
CREATE INDEX IF NOT EXISTS idx_judgments_source_paper ON judgments(source_paper_id);

-- Phase 9: 判断ノード間の「証明が参照している」依存関係（論理的な射である
-- morphismsとは別物）。詳細は`judgment_dependency.rs`参照。
CREATE TABLE IF NOT EXISTS judgment_dependencies (
    from_judgment  INTEGER NOT NULL REFERENCES judgments(id),
    to_judgment    INTEGER NOT NULL REFERENCES judgments(id),
    PRIMARY KEY (from_judgment, to_judgment)
);
CREATE INDEX IF NOT EXISTS idx_judgment_dependencies_to ON judgment_dependencies(to_judgment);

-- 診断⑥拡張: `\cite`が指す論文単位の引用関係。judgment_dependenciesとは
-- 別物（詳細は`paper_citation.rs`参照）——`\cite`が指すのは特定の判断では
-- なく論文全体なので、judgment同士の辺に無理に押し込めない。
CREATE TABLE IF NOT EXISTS paper_citations (
    from_paper  INTEGER NOT NULL REFERENCES papers(id),
    to_paper    INTEGER NOT NULL REFERENCES papers(id),
    PRIMARY KEY (from_paper, to_paper)
);
CREATE INDEX IF NOT EXISTS idx_paper_citations_to ON paper_citations(to_paper);

CREATE TABLE IF NOT EXISTS morphisms (
    id                    INTEGER PRIMARY KEY,
    src                   INTEGER NOT NULL REFERENCES judgments(id),
    dst                   INTEGER NOT NULL REFERENCES judgments(id),
    kind                  TEXT NOT NULL,
    origin                TEXT NOT NULL,
    status                TEXT NOT NULL,
    rationale             TEXT,
    proof_term_hash       TEXT,
    dependency_signature  TEXT,
    created_at            INTEGER NOT NULL,
    UNIQUE(src, dst, kind)
);
-- 複合インデックスで大規模クエリの高速化（フェーズ3レビュー問題3への対応）
CREATE INDEX IF NOT EXISTS idx_morphisms_src_dst_kind ON morphisms(src, dst, kind);
CREATE INDEX IF NOT EXISTS idx_morphisms_status_src ON morphisms(status, src);
CREATE INDEX IF NOT EXISTS idx_morphisms_status_dst ON morphisms(status, dst);
CREATE INDEX IF NOT EXISTS idx_morphisms_kind_status ON morphisms(kind, status);

CREATE TABLE IF NOT EXISTS proof_terms (
    id          INTEGER PRIMARY KEY,
    hash        TEXT NOT NULL UNIQUE,
    ast_json    TEXT NOT NULL,
    raw_text    TEXT NOT NULL,
    created_at  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_proof_terms_hash ON proof_terms(hash);

-- 層4: 戦略・メタ層（Strategy/Meta Layer）
CREATE TABLE IF NOT EXISTS strategies (
    id           INTEGER PRIMARY KEY,
    name         TEXT NOT NULL UNIQUE,
    description  TEXT
);

-- 層3の射（証明）に「どの戦略で導かれたか」を多対多でタグ付けする
CREATE TABLE IF NOT EXISTS morphism_strategies (
    morphism_id  INTEGER NOT NULL REFERENCES morphisms(id),
    strategy_id  INTEGER NOT NULL REFERENCES strategies(id),
    PRIMARY KEY (morphism_id, strategy_id)
);
CREATE INDEX IF NOT EXISTS idx_morphism_strategies_strategy ON morphism_strategies(strategy_id);

-- 失敗した証明試行（FailedPath）。反例で判断が偽と判明した場合（FalseTheorem 相当）も
-- pattern = 'counterexample' として同じテーブルに追記する。既存ノードは書き換えない。
CREATE TABLE IF NOT EXISTS failed_attempts (
    id                INTEGER PRIMARY KEY,
    target_judgment   INTEGER REFERENCES judgments(id),
    goal_text         TEXT NOT NULL,
    pattern           TEXT NOT NULL,
    detail            TEXT,
    proof_term_hash   TEXT,
    source_file       TEXT,
    source_line       INTEGER,
    created_at        INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_failed_attempts_target ON failed_attempts(target_judgment);
CREATE INDEX IF NOT EXISTS idx_failed_attempts_pattern ON failed_attempts(pattern);

CREATE TABLE IF NOT EXISTS failed_attempt_strategies (
    failed_attempt_id  INTEGER NOT NULL REFERENCES failed_attempts(id),
    strategy_id         INTEGER NOT NULL REFERENCES strategies(id),
    PRIMARY KEY (failed_attempt_id, strategy_id)
);
CREATE INDEX IF NOT EXISTS idx_failed_attempt_strategies_strategy ON failed_attempt_strategies(strategy_id);
"#;

impl GraphStore {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(GraphStore {
            conn,
            quotient_cache: RefCell::new(QuotientCache::default()),
            heuristic_cache: RefCell::new(JudgmentLiteCache::default()),
        })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(GraphStore {
            conn,
            quotient_cache: RefCell::new(QuotientCache::default()),
            heuristic_cache: RefCell::new(JudgmentLiteCache::default()),
        })
    }

    /// 複数の書き込みを1つの明示的な SQLite トランザクションにまとめて実行する。
    ///
    /// `insert_judgment`/`intern_expr`/`insert_morphism` 等の個々のメソッドは
    /// 自明にトランザクションで囲んでいない（＝SQLiteの自動コミットモードにより
    /// 1呼び出し＝1トランザクション＝1回のfsync）。実データ（DeGiorgiコーパス
    /// 94ファイル・1431判断）を素朴にループ挿入した実測では import が
    /// 1件あたり約9.8ms、`propose_morphisms` の射挿入が1件あたり約4.9msかかって
    /// おり、これは同じ処理をインメモリ criterion ベンチマークで測ると
    /// マイクロ秒オーダーであることと比べて桁違いに遅い——CPU計算ではなく
    /// ディスクへのコミット待ちがボトルネックであることを示している
    /// （プロファイリングの原則どおり、まず実測でここを特定してから対処した）。
    /// 大量件数を挿入する呼び出し側はこれで包むことで、コミット回数を
    /// 「行数分」から「1回」に減らせる。
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

    /// 式を正規化ハッシュでインターンし、既存なら再利用、なければ新規挿入する。
    pub fn intern_expr(&self, e: &Expr) -> Result<ExprId> {
        let hash = e.canonical_hash_hex();
        if let Some(id) = self
            .conn
            .prepare_cached("SELECT id FROM expressions WHERE canonical_hash = ?1")?
            .query_row(params![hash], |row| row.get::<_, i64>(0))
            .optional()?
        {
            return Ok(ExprId(id));
        }
        let ast_json = serde_json::to_string(e).expect("Expr は常にシリアライズ可能");
        let display = format!("{e}");
        self.conn.prepare_cached(
            "INSERT INTO expressions (canonical_hash, ast_json, display) VALUES (?1, ?2, ?3)",
        )?
        .execute(params![hash, ast_json, display])?;
        Ok(ExprId(self.conn.last_insert_rowid()))
    }

    pub fn get_expr(&self, id: ExprId) -> Result<Expr> {
        let ast_json: String = self
            .conn
            .prepare_cached("SELECT ast_json FROM expressions WHERE id = ?1")?
            .query_row(params![id.0], |row| row.get(0))?;
        Ok(serde_json::from_str(&ast_json).expect("保存済み AST は常に妥当な JSON"))
    }

    pub fn expr_count(&self) -> Result<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM expressions", [], |r| r.get(0))
    }

    /// 証明項を正規化ハッシュでインターンし、既存なら再利用、なければ新規挿入する。
    /// ハッシュ (canonical_hash_hex) を返す。
    pub fn intern_proof_term(&self, term: &ProofTerm) -> Result<String> {
        let hash = term.canonical_hash_hex();
        if let Some(existing_hash) = self
            .conn
            .prepare_cached("SELECT hash FROM proof_terms WHERE hash = ?1")?
            .query_row(params![hash], |row| row.get::<_, String>(0))
            .optional()?
        {
            return Ok(existing_hash);
        }

        let ast_json = serde_json::to_string(term).expect("ProofTerm はシリアライズ可能");
        let raw_text = match term {
            ProofTerm::TacticScript(s) | ProofTerm::Raw(s) => s.clone(),
            _ => format!("{:?}", term),
        };

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        self.conn
            .prepare_cached(
                "INSERT INTO proof_terms (hash, ast_json, raw_text, created_at) VALUES (?1, ?2, ?3, ?4)",
            )?
            .execute(params![hash, ast_json, raw_text, now])?;
        Ok(hash)
    }

    pub fn get_proof_term(&self, hash: &str) -> Result<ProofTerm> {
        let ast_json: String = self
            .conn
            .prepare_cached("SELECT ast_json FROM proof_terms WHERE hash = ?1")?
            .query_row(params![hash], |row| row.get(0))?;
        Ok(serde_json::from_str(&ast_json).expect("保存済み ProofTerm AST は妥当な JSON"))
    }

    pub fn proof_term_count(&self) -> Result<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM proof_terms", [], |r| r.get(0))
    }

    /// 与えたハッシュを持つ式（＝互いにα同値な式群）を参照している判断ノードを探す。
    /// フェーズ2で「同値」エッジを検出する際の土台になる。
    pub fn find_judgments_by_statement_hash(&self, hash: &str) -> Result<Vec<JudgmentId>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT j.id FROM judgments j JOIN expressions e ON j.statement_expr_id = e.id
             WHERE e.canonical_hash = ?1",
        )?;
        let ids = stmt
            .query_map(params![hash], |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(ids.into_iter().map(JudgmentId).collect())
    }

    pub fn insert_judgment(&self, j: &NewJudgment) -> Result<JudgmentId> {
        let context_entries: Vec<(String, i64)> = j
            .context
            .iter()
            .map(|h| (h.name.clone(), h.ty.0))
            .collect();
        let context_json = serde_json::to_string(&context_entries).unwrap();

        self.conn
            .prepare_cached(
                "INSERT INTO judgments
                    (kind, name, context_json, statement_expr_id, definition_body_raw,
                     source_file, source_line, raw_text, parse_status, source_paper_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?
            .execute(params![
                j.kind.as_str(),
                j.name,
                context_json,
                j.statement.0,
                j.definition_body_raw,
                j.source.file,
                j.source.line,
                j.raw_text,
                j.parse_status.as_str(),
                j.source_paper.map(|p| p.0),
            ])?;
        Ok(JudgmentId(self.conn.last_insert_rowid()))
    }

    pub fn get_judgment(&self, id: JudgmentId) -> Result<JudgmentRecord> {
        let row = self
            .conn
            .prepare_cached(
                "SELECT kind, name, context_json, statement_expr_id, definition_body_raw,
                        source_file, source_line, raw_text, parse_status, source_paper_id
                 FROM judgments WHERE id = ?1",
            )?
            .query_row(params![id.0], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, u32>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, Option<i64>>(9)?,
                ))
            })?;
        self.hydrate_judgment(id, row)
    }

    pub fn list_judgments(&self) -> Result<Vec<JudgmentRecord>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, kind, name, context_json, statement_expr_id, definition_body_raw,
                    source_file, source_line, raw_text, parse_status, source_paper_id
             FROM judgments ORDER BY id",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, u32>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, Option<i64>>(10)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);

        let mut out = Vec::with_capacity(rows.len());
        for (id, kind, name, context_json, stmt_id, def_body, file, line, raw, status, paper) in rows {
            out.push(self.hydrate_judgment(
                JudgmentId(id),
                (kind, name, context_json, stmt_id, def_body, file, line, raw, status, paper),
            )?);
        }
        Ok(out)
    }

    #[allow(clippy::type_complexity)]
    fn hydrate_judgment(
        &self,
        id: JudgmentId,
        row: (
            String,
            Option<String>,
            String,
            i64,
            Option<String>,
            String,
            u32,
            String,
            String,
            Option<i64>,
        ),
    ) -> Result<JudgmentRecord> {
        let (kind, name, context_json, stmt_id, def_body, file, line, raw, status, paper) = row;

        // Phase 1 最適化: context が空の大多数のケースでは JSON パースと式取得を丸ごとスキップする
        let context = if context_json.is_empty() || context_json == "[]" {
            Vec::new()
        } else {
            let context_entries: Vec<(String, i64)> =
                serde_json::from_str(&context_json).unwrap_or_default();

            let mut ctx = Vec::with_capacity(context_entries.len());
            for (entry_name, ty_id) in context_entries {
                let ty = self.get_expr(ExprId(ty_id))?;
                ctx.push((entry_name, ty));
            }
            ctx
        };

        let statement = self.get_expr(ExprId(stmt_id))?;
        let statement_hash = statement.canonical_hash_hex();
        Ok(JudgmentRecord {
            id,
            kind: JudgmentKind::from_str(&kind).expect("保存済み kind は常に既知の値"),
            name,
            context,
            statement,
            statement_hash,
            definition_body_raw: def_body,
            source_file: file,
            source_line: line,
            raw_text: raw,
            parse_status: ParseStatus::from_str(&status),
            source_paper: paper.map(crate::paper::PaperId),
        })
    }
}

/// `Hypothesis` を組み立てる補助（型を式ストアへインターンしてから ExprId を得る）
pub fn intern_hypotheses(
    store: &GraphStore,
    bindings: &[(String, Expr)],
) -> Result<Vec<Hypothesis>> {
    let mut out = Vec::with_capacity(bindings.len());
    for (name, ty) in bindings {
        let ty_id = store.intern_expr(ty)?;
        out.push(Hypothesis {
            name: name.clone(),
            ty: ty_id,
        });
    }
    Ok(out)
}

impl GraphStore {
    /// ストレージの整合性をチェックする。存在しない式IDへの参照などを検出。
    pub fn validate(&self) -> Result<ValidationReport> {
        let mut report = ValidationReport::default();

        // Check all judgments reference valid expression IDs
        let mut stmt = self
            .conn
            .prepare_cached("SELECT j.id, j.statement_expr_id, j.context_json FROM judgments j")?;

        let judgment_ids: Vec<(i64, i64, String)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);

        for (jid, stmt_id, ctx_json) in &judgment_ids {
            // Check statement expression exists
            if self.get_expr(ExprId(*stmt_id)).is_err() {
                report.broken_references.push(format!(
                    "Judgment {} references missing expression {}",
                    jid, stmt_id
                ));
            }

            // Check context expressions exist
            if let Ok(ctx_entries) = serde_json::from_str::<Vec<(String, i64)>>(ctx_json) {
                for (_, ty_id) in ctx_entries {
                    if self.get_expr(ExprId(ty_id)).is_err() {
                        report.broken_references.push(format!(
                            "Judgment {} context references missing expression {}",
                            jid, ty_id
                        ));
                    }
                }
            }
        }

        report.total_expressions = self.expr_count().unwrap_or(0);
        report.total_judgments = judgment_ids.len() as i64;
        report.total_morphisms = self.morphism_count().unwrap_or(0);
        report.accepted_morphisms = self
            .count_morphisms_with_status(EdgeStatus::Accepted)
            .unwrap_or(0);

        Ok(report)
    }
}

/// ストレージ検証レポート
#[derive(Debug, Default)]
pub struct ValidationReport {
    pub total_expressions: i64,
    pub total_judgments: i64,
    pub total_morphisms: i64,
    pub accepted_morphisms: i64,
    pub broken_references: Vec<String>,
}

impl ValidationReport {
    pub fn is_valid(&self) -> bool {
        self.broken_references.is_empty()
    }

    pub fn summary(&self) -> String {
        if self.is_valid() {
            format!(
                "✓ Valid: {} expressions, {} judgments, {} morphisms ({} accepted)",
                self.total_expressions,
                self.total_judgments,
                self.total_morphisms,
                self.accepted_morphisms
            )
        } else {
            format!(
                "✗ Invalid: {} expressions, {} judgments, {} morphisms, {} broken references",
                self.total_expressions,
                self.total_judgments,
                self.total_morphisms,
                self.broken_references.len()
            )
        }
    }
}

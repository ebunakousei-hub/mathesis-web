use crate::error::{ProvenanceResult, ValidationError};
use crate::model::{AssertionId, EpistemicState, NewRelationAssertion, RelationAssertion, RelationKind, ReleaseId};
use crate::relation_policy::{valid_entity_kinds, SOURCE_MAPPING_POLICY_VERSION};
use crate::store::{ProvenanceStore, Result};
use rusqlite::{params, OptionalExtension};

/// `"kind:id"`形式の`subject_ref`/`object_ref`から先頭のkindタグだけを取り出す。
fn ref_kind(reference: &str) -> &str {
    reference.split_once(':').map(|(k, _)| k).unwrap_or(reference)
}

/// このクレートの実際のプロデューサ（`legacy_adapter.rs`）が作る組み合わせだけを
/// 検証するガードレール。ARCHITECTURE_NEXT.md §5.2の型付きカタログ（Entity/
/// RelationSchema）が無いこの増分でも、`paper:X specializes paper:Y`のような
/// 明らかに無意味な行が紛れ込むのを防ぐ（外部レビュー2026-09-05指摘）。
/// まだどのアダプタも作らない述語（`imports`/`related_to`/`uses_concept`）は
/// ルール未定義のため素通しする——将来の用途を先回りして禁止しない。
fn validate_relation_kinds(predicate: RelationKind, subject_ref: &str, object_ref: &str) -> ProvenanceResult<()> {
    let (s, o) = (ref_kind(subject_ref), ref_kind(object_ref));
    let ok = crate::model::EntityKind::from_str(s)
        .zip(crate::model::EntityKind::from_str(o))
        .map(|(s, o)| valid_entity_kinds(predicate, s, o))
        .unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err(ValidationError::RelationKindMismatch {
            predicate,
            subject_ref: subject_ref.to_string(),
            object_ref: object_ref.to_string(),
        }
        .into())
    }
}

impl ProvenanceStore {
    /// レガシースナップショットアダプタの冪等性を支える鍵引き。同じ
    /// (release, legacy_ref) を再実行しても重複挿入されないよう、呼び出し側は
    /// 挿入前にこれで既存行の有無を確かめる。
    pub fn get_assertion_by_legacy_ref(&self, release: ReleaseId, legacy_ref: &str) -> Result<Option<AssertionId>> {
        self.conn
            .prepare_cached(
                "SELECT id FROM relation_assertions WHERE release_id = ?1 AND legacy_ref = ?2",
            )?
            .query_row(params![release.0, legacy_ref], |r| r.get::<_, i64>(0))
            .optional()
            .map(|opt| opt.map(AssertionId))
    }

    pub fn insert_assertion(&self, new: &NewRelationAssertion) -> ProvenanceResult<AssertionId> {
        validate_relation_kinds(new.predicate, &new.subject_ref, &new.object_ref)?;
        self.conn
            .prepare_cached(
                "INSERT INTO relation_assertions
                    (subject_ref, predicate, object_ref, epistemic_state, score, policy_version,
                     created_by_run_id, supersedes_id, release_id, legacy_ref)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?
            .execute(params![
                new.subject_ref,
                new.predicate.as_str(),
                new.object_ref,
                new.epistemic_state.as_str(),
                new.score,
                new.policy_version.as_deref().or(Some(SOURCE_MAPPING_POLICY_VERSION)),
                new.created_by_run_id,
                new.supersedes_id.map(|id| id.0),
                new.release_id.0,
                new.legacy_ref,
            ])?;
        Ok(AssertionId(self.conn.last_insert_rowid()))
    }

    pub fn get_assertion(&self, id: AssertionId) -> Result<RelationAssertion> {
        self.conn
            .prepare_cached(
                "SELECT id, subject_ref, predicate, object_ref, epistemic_state, score, policy_version,
                        created_by_run_id, supersedes_id, release_id, legacy_ref
                 FROM relation_assertions WHERE id = ?1",
            )?
            .query_row(params![id.0], Self::assertion_row)
    }

    /// `get_assertion`のOption版。`verify.rs`が「サイドカーが指すassertion
    /// idが実在するか」を、存在しない場合にエラーにせず確かめるのに使う。
    pub fn try_get_assertion(&self, id: AssertionId) -> Result<Option<RelationAssertion>> {
        self.conn
            .prepare_cached(
                "SELECT id, subject_ref, predicate, object_ref, epistemic_state, score, policy_version,
                        created_by_run_id, supersedes_id, release_id, legacy_ref
                 FROM relation_assertions WHERE id = ?1",
            )?
            .query_row(params![id.0], Self::assertion_row)
            .optional()
    }

    pub fn list_assertions(&self) -> Result<Vec<RelationAssertion>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, subject_ref, predicate, object_ref, epistemic_state, score, policy_version,
                    created_by_run_id, supersedes_id, release_id, legacy_ref
             FROM relation_assertions ORDER BY id",
        )?;
        let rows = stmt.query_map([], Self::assertion_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// リリース1件ぶんの全assertion。`web_export`はこれ**だけ**を入口にする
    /// ——`mathesis-graph`/`mathesis-taxonomy`のSQLiteを開き直さない
    /// （docs/P2_STATUS.md参照）。
    pub fn list_assertions_for_release(&self, release: ReleaseId) -> Result<Vec<RelationAssertion>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, subject_ref, predicate, object_ref, epistemic_state, score, policy_version,
                    created_by_run_id, supersedes_id, release_id, legacy_ref
             FROM relation_assertions WHERE release_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![release.0], Self::assertion_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn assertion_count(&self) -> Result<i64> {
        self.conn.query_row("SELECT COUNT(*) FROM relation_assertions", [], |r| r.get(0))
    }

    /// 述語ごとの件数（`stats`サブコマンド用）。
    pub fn assertion_count_by_predicate(&self) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT predicate, COUNT(*) FROM relation_assertions GROUP BY predicate ORDER BY predicate",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// 認識状態ごとの件数（`stats`サブコマンド用）。
    pub fn assertion_count_by_state(&self) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT epistemic_state, COUNT(*) FROM relation_assertions GROUP BY epistemic_state ORDER BY epistemic_state",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    fn assertion_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RelationAssertion> {
        let predicate_str: String = row.get(2)?;
        let state_str: String = row.get(4)?;
        Ok(RelationAssertion {
            id: AssertionId(row.get(0)?),
            subject_ref: row.get(1)?,
            predicate: RelationKind::from_str(&predicate_str).expect("保存済み predicate は常に既知の値"),
            object_ref: row.get(3)?,
            epistemic_state: EpistemicState::from_str(&state_str).expect("保存済み epistemic_state は常に既知の値"),
            score: row.get(5)?,
            policy_version: row.get(6)?,
            created_by_run_id: row.get(7)?,
            supersedes_id: row.get::<_, Option<i64>>(8)?.map(AssertionId),
            release_id: ReleaseId(row.get(9)?),
            legacy_ref: row.get(10)?,
        })
    }
}

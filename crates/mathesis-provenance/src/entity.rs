//! P3, Increment 1（ARCHITECTURE_NEXT.md §5.2、`docs/P3_STATUS.md`）:
//! `entities`/`entity_refs`テーブルへのCRUD。`mathesis-graph`の
//! `store.rs`と同じ「専用ファイル1本にそのテーブルの操作を集める」方針。
//!
//! `entity_refs`が実質的な冪等キー——`entities`自体には自然キーが無く
//! （conceptは複数のref文字列を持ちうるため`entities`単独では一意に
//! 引けない）、必ず`entity_refs`経由で解決する。

use crate::model::{Entity, EntityId, EntityKind, LabelOrigin, NewEntity};
use crate::store::{ProvenanceStore, Result};
use rusqlite::{params, OptionalExtension};

/// `backfill_assertion_entity_ids`の結果。`import_graph`の`ImportStats`
/// (`imported`/`skipped_existing`)や`catalog_adapter::CatalogStats`
/// (`was_new`集計)と同じ「再実行のたびに同じ数字が+Nと出て、あたかも
/// 毎回新規に増えているように見える」ことを避ける規律——実データで最初に
/// 実装したときは`unresolved==0`なら常に全件を無条件にUPDATEし、2回目の
/// 実行でも"104708 backfilled"と出た(実際には1件も変わっていなかった)。
#[derive(Debug, Default, Clone, Copy)]
pub struct BackfillStats {
    pub newly_backfilled: usize,
    pub already_correct: usize,
    pub unresolved: usize,
}

impl ProvenanceStore {
    /// Populate the additive assertion endpoint columns from the catalog.
    /// Never invents an entity — an assertion whose subject/object isn't
    /// cataloged yet is counted as `unresolved`, not silently skipped.
    ///
    /// 実データ(104,708件)で最初に踏んだ実測: 1件ずつ`self.conn.execute`する
    /// と各UPDATEがSQLiteの自動コミットで独立トランザクションになり、
    /// `mathesis-graph::store`が既に文書化している「1行=1トランザクションだと
    /// fsync待ちが支配的（実測9.8ms/行）」と同じ罠を踏んで15分以上かかった
    /// ——`legacy_adapter::import_graph`と同じ「呼び出し全体を1トランザクション
    /// に包む」規律をここにも適用する（修正後、実データで53秒）。
    pub fn backfill_assertion_entity_ids(&self) -> Result<BackfillStats> {
        let assertions = self.list_assertions()?;
        let mut updates = Vec::with_capacity(assertions.len());
        let mut already_correct = 0;
        let mut unresolved = 0;
        for assertion in &assertions {
            let subject = self.resolve_entity_ref(&assertion.subject_ref)?;
            let object = self.resolve_entity_ref(&assertion.object_ref)?;
            match (subject, object) {
                (Some(subject), Some(object)) => {
                    if assertion.subject_entity_id == Some(subject) && assertion.object_entity_id == Some(object) {
                        already_correct += 1;
                    } else {
                        updates.push((assertion.id.0, subject.0, object.0));
                    }
                }
                _ => unresolved += 1,
            }
        }
        if unresolved > 0 {
            return Ok(BackfillStats { newly_backfilled: 0, already_correct: 0, unresolved });
        }
        let newly_backfilled = updates.len();
        self.transaction(|| -> Result<()> {
            for (assertion_id, subject_id, object_id) in updates.iter().copied() {
                self.conn.execute(
                    "UPDATE relation_assertions
                     SET subject_entity_id = ?1, object_entity_id = ?2
                     WHERE id = ?3",
                    params![subject_id, object_id, assertion_id],
                )?;
            }
            Ok(())
        })?;
        Ok(BackfillStats { newly_backfilled, already_correct, unresolved: 0 })
    }

    pub fn assertion_entity_id_coverage(&self) -> Result<(i64, i64)> {
        let total: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM relation_assertions", [], |row| row.get(0))?;
        let complete: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM relation_assertions
             WHERE subject_entity_id IS NOT NULL AND object_entity_id IS NOT NULL",
            [], |row| row.get(0))?;
        Ok((complete, total))
    }

    /// `ref_string`が指すエンティティのidを引く。まだ知らない参照なら`None`
    /// ——「実在しない参照」と「まだカタログを作っていない」を呼び出し側で
    /// 区別する必要はここでは持たない（`catalog_adapter`のカバレッジ集計が
    /// その解釈を担う）。
    pub fn resolve_entity_ref(&self, ref_string: &str) -> Result<Option<EntityId>> {
        self.conn
            .prepare_cached("SELECT entity_id FROM entity_refs WHERE ref_string = ?1")?
            .query_row(params![ref_string], |r| r.get::<_, i64>(0))
            .optional()
            .map(|opt| opt.map(EntityId))
    }

    /// P5, Item 2 step 4（`docs/P5_PLAN.md`）: `EntityId`から、その判断
    /// 自身の`mathesis-graph`上の数値idへ逆引きする。`web_export.rs`が
    /// `subject_ref`の文字列プレフィックスを剥がす代わりに、`subject_entity_id`
    /// (FK)を出典として使うために要る——conceptやpaperと違い、Judgment
    /// エンティティは`catalog_adapter::build_judgment_paper_catalog`により
    /// 常にちょうど1つのref(`"judgment:<id>"`)しか持たない(aliasが無い)ので、
    /// この逆引きは曖昧にならない。
    pub fn judgment_id_for_entity(&self, id: EntityId) -> Result<Option<i64>> {
        self.conn
            .prepare_cached("SELECT ref_string FROM entity_refs WHERE entity_id = ?1 AND ref_string LIKE 'judgment:%' LIMIT 1")?
            .query_row(params![id.0], |r| r.get::<_, String>(0))
            .optional()
            .map(|opt| opt.and_then(|s| s.strip_prefix("judgment:").and_then(|n| n.parse().ok())))
    }

    pub fn resolve_entity_ref_with_kind(&self, ref_string: &str) -> Result<Option<(EntityId, EntityKind)>> {
        self.conn
            .prepare_cached(
                "SELECT r.entity_id, e.kind
                 FROM entity_refs r JOIN entities e ON e.id = r.entity_id
                 WHERE r.ref_string = ?1",
            )?
            .query_row(params![ref_string], |r| {
                let kind: String = r.get(1)?;
                let kind = EntityKind::from_str(&kind).ok_or_else(|| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "unknown entity kind")),
                    )
                })?;
                Ok((EntityId(r.get(0)?), kind))
            })
            .optional()
    }

    /// 呼び方が1つしか無いエンティティ（judgment/paper）を冪等に登録する。
    /// 戻り値の`bool`は「今回新しく作ったか」——呼び出し側が
    /// `import_graph`と同じ「+N件（skip M件）」形式で正直に報告できるように
    /// 返す（`entities`テーブルは同じ入力に対して増え続けないが、
    /// 「毎回+N件と表示され続ける」という見た目の紛らわしさは別問題なので、
    /// これは削らない）。
    pub fn get_or_insert_entity(&self, new: &NewEntity, ref_string: &str) -> Result<(EntityId, bool)> {
        self.get_or_insert_entity_with_refs(new, std::slice::from_ref(&ref_string.to_string()))
    }

    /// `refs`は代表となる呼び方を先頭に持つ配列（conceptなら
    /// `["concept:<representative>", "concept:<alias1>", ...]`）。
    /// 冪等性は先頭の呼び方だけで判定する——`resolve::resolve`は同じ入力
    /// DBに対して決定的な代表選び（文書頻度最大）をするので、再実行しても
    /// 先頭は変わらない。再実行時に新しいaliasが増えていれば
    /// （`INSERT OR IGNORE`で）そのぶんだけ紐付けを追加する。
    pub fn get_or_insert_entity_with_refs(&self, new: &NewEntity, refs: &[String]) -> Result<(EntityId, bool)> {
        let canonical = refs.first().expect("refsは常に1件以上");
        if let Some(id) = self.resolve_entity_ref(canonical)? {
            for r in &refs[1..] {
                self.conn
                    .prepare_cached("INSERT OR IGNORE INTO entity_refs (ref_string, entity_id) VALUES (?1, ?2)")?
                    .execute(params![r, id.0])?;
            }
            return Ok((id, false));
        }
        self.conn
            .prepare_cached("INSERT INTO entities (kind, display_label, source_record_id) VALUES (?1, ?2, ?3)")?
            .execute(params![new.kind.as_str(), new.display_label, new.source_record_id.map(|s| s.0)])?;
        let id = EntityId(self.conn.last_insert_rowid());
        for r in refs {
            self.conn
                .prepare_cached("INSERT INTO entity_refs (ref_string, entity_id) VALUES (?1, ?2)")?
                .execute(params![r, id.0])?;
        }
        Ok((id, true))
    }

    pub fn get_entity(&self, id: EntityId) -> Result<Entity> {
        self.conn
            .prepare_cached("SELECT id, kind, display_label, source_record_id FROM entities WHERE id = ?1")?
            .query_row(params![id.0], Self::entity_row)
    }

    pub fn set_label_origin(&self, id: EntityId, origin: LabelOrigin) -> Result<()> {
        self.conn.execute(
            "INSERT INTO entity_labels (entity_id, origin) VALUES (?1, ?2)
             ON CONFLICT(entity_id) DO UPDATE SET origin=excluded.origin",
            params![id.0, origin.as_str()],
        )?;
        Ok(())
    }

    pub fn label_origin(&self, id: EntityId) -> Result<Option<LabelOrigin>> {
        self.conn
            .query_row("SELECT origin FROM entity_labels WHERE entity_id = ?1", params![id.0], |row| {
                let value: String = row.get(0)?;
                Ok(match value.as_str() {
                    "source_provided" => LabelOrigin::SourceProvided,
                    "canonicalized" => LabelOrigin::Canonicalized,
                    "derived" => LabelOrigin::Derived,
                    "fallback_identifier" => LabelOrigin::FallbackIdentifier,
                    _ => return Err(rusqlite::Error::InvalidQuery),
                })
            })
            .optional()
    }

    /// `get_entity`のOption版（`assertion_export.rs`が「ラベルが引ければ
    /// 見せる、引けなければ黙って省く」ために使う）。
    pub fn try_get_entity(&self, id: EntityId) -> Result<Option<Entity>> {
        self.conn
            .prepare_cached("SELECT id, kind, display_label, source_record_id FROM entities WHERE id = ?1")?
            .query_row(params![id.0], Self::entity_row)
            .optional()
    }

    pub fn entity_count(&self) -> Result<i64> {
        self.conn.query_row("SELECT COUNT(*) FROM entities", [], |r| r.get(0))
    }

    /// 種別ごとの件数（`stats`サブコマンド用）。
    pub fn entity_count_by_kind(&self) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare_cached("SELECT kind, COUNT(*) FROM entities GROUP BY kind ORDER BY kind")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// P8.4（`docs/P8_4_STATUS.md`）: `ref_string`の前置きで絞り込んだ
    /// entity一覧——`math_graph_adapter::remove_project`が
    /// `"judgment:mathgraph:"`で使う。ワイルドカード文字（`%`/`_`）を含む
    /// `prefix`を渡すと`LIKE`の意味が壊れるが、呼び出し元は固定の名前空間
    /// 接頭辞しか渡さないため実害はない。
    pub fn entity_ids_with_ref_prefix(&self, prefix: &str) -> Result<Vec<EntityId>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT DISTINCT entity_id FROM entity_refs WHERE ref_string LIKE ?1 || '%' ORDER BY entity_id",
        )?;
        let rows = stmt.query_map(params![prefix], |r| r.get::<_, i64>(0).map(EntityId))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// P8.4: `retract.rs::retract_entity`用。先に`assertion_ids_touching_entity`
    /// が0件になっていることを呼び出し側が保証する（`entity_refs`にFKは無いが、
    /// 意味的な後始末——参照を残したまま実体だけ消すと`resolve_entity_ref`が
    /// 存在しないentity_idを返すようになる）。
    pub fn delete_entity_refs_for_entity(&self, id: EntityId) -> Result<usize> {
        self.conn.prepare_cached("DELETE FROM entity_refs WHERE entity_id = ?1")?.execute(params![id.0])
    }

    /// P8.4: このリポジトリで最初のentity削除メソッド——呼び出し順は
    /// `retract.rs::retract_entity`参照（evidence→review→assertion→
    /// entity_refs→entity→source_record）。
    pub fn delete_entity(&self, id: EntityId) -> Result<()> {
        self.conn.prepare_cached("DELETE FROM entities WHERE id = ?1")?.execute(params![id.0])?;
        Ok(())
    }

    /// このエンティティが知られているすべての呼び方（conceptなら代表+alias群）。
    pub fn refs_for_entity(&self, id: EntityId) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare_cached("SELECT ref_string FROM entity_refs WHERE entity_id = ?1 ORDER BY ref_string")?;
        let rows = stmt.query_map(params![id.0], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    fn entity_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Entity> {
        let kind_str: String = row.get(1)?;
        Ok(Entity {
            id: EntityId(row.get(0)?),
            kind: EntityKind::from_str(&kind_str).expect("保存済み kind は常に既知の値"),
            display_label: row.get(2)?,
            source_record_id: row.get::<_, Option<i64>>(3)?.map(crate::model::SourceRecordId),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EntityKind;

    fn judgment_entity(label: &str) -> NewEntity {
        NewEntity { kind: EntityKind::Judgment, display_label: label.to_string(), source_record_id: None }
    }

    #[test]
    fn single_ref_entity_is_interned_idempotently() {
        let prov = ProvenanceStore::open_in_memory().unwrap();
        let (a, a_new) = prov.get_or_insert_entity(&judgment_entity("unitBallApproxEps"), "judgment:1").unwrap();
        let (b, b_new) = prov.get_or_insert_entity(&judgment_entity("unitBallApproxEps"), "judgment:1").unwrap();
        assert_eq!(a, b);
        assert!(a_new, "初回は新規作成のはず");
        assert!(!b_new, "2回目は既存を再利用したはず");
        assert_eq!(prov.entity_count().unwrap(), 1);
    }

    #[test]
    fn concept_with_aliases_resolves_every_alias_to_the_same_entity() {
        let prov = ProvenanceStore::open_in_memory().unwrap();
        let new = NewEntity { kind: EntityKind::Concept, display_label: "kahler manifold".into(), source_record_id: None };
        let refs = vec!["concept:kahler manifold".to_string(), "concept:kahler manifolds".to_string()];
        let (id, was_new) = prov.get_or_insert_entity_with_refs(&new, &refs).unwrap();
        assert!(was_new);

        assert_eq!(prov.resolve_entity_ref("concept:kahler manifold").unwrap(), Some(id));
        assert_eq!(prov.resolve_entity_ref("concept:kahler manifolds").unwrap(), Some(id));
        assert_eq!(prov.resolve_entity_ref("concept:unrelated phrase").unwrap(), None);
        assert_eq!(prov.entity_count().unwrap(), 1, "表記ゆれは1エンティティに集約される");

        let mut refs_back = prov.refs_for_entity(id).unwrap();
        refs_back.sort();
        assert_eq!(refs_back, vec!["concept:kahler manifold".to_string(), "concept:kahler manifolds".to_string()]);
    }

    #[test]
    fn rerunning_with_a_newly_discovered_alias_adds_it_without_duplicating_the_entity() {
        let prov = ProvenanceStore::open_in_memory().unwrap();
        let new = NewEntity { kind: EntityKind::Concept, display_label: "kahler manifold".into(), source_record_id: None };
        let (id1, first_new) = prov.get_or_insert_entity_with_refs(&new, &["concept:kahler manifold".to_string()]).unwrap();
        assert!(first_new);
        // 2回目の実行で新しい表記ゆれが1件増えたと仮定する。
        let (id2, second_new) = prov
            .get_or_insert_entity_with_refs(&new, &["concept:kahler manifold".to_string(), "concept:kaehler manifold".to_string()])
            .unwrap();
        assert_eq!(id1, id2);
        assert!(!second_new, "既存エンティティへのalias追加は「新規」ではない");
        assert_eq!(prov.entity_count().unwrap(), 1);
        assert_eq!(prov.resolve_entity_ref("concept:kaehler manifold").unwrap(), Some(id1));
    }

    #[test]
    fn unknown_reference_does_not_resolve() {
        let prov = ProvenanceStore::open_in_memory().unwrap();
        assert_eq!(prov.resolve_entity_ref("judgment:999").unwrap(), None);
        assert!(prov.try_get_entity(crate::model::EntityId(999)).unwrap().is_none());
    }

    // P5, Item 2（`docs/P5_PLAN.md`）: additive EntityId endpoint columns.

    use crate::model::{EpistemicState, NewRelationAssertion, NewRelease, RelationKind};

    fn store_with_release() -> (ProvenanceStore, crate::model::ReleaseId) {
        let prov = ProvenanceStore::open_in_memory().unwrap();
        let release = prov
            .get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None })
            .unwrap();
        (prov, release)
    }

    fn assertion(prov: &ProvenanceStore, release: crate::model::ReleaseId, subject: &str, object: &str, legacy_ref: &str) -> crate::model::AssertionId {
        prov.insert_assertion(&NewRelationAssertion {
            subject_ref: subject.into(),
            predicate: RelationKind::DependsOn,
            object_ref: object.into(),
            epistemic_state: EpistemicState::Extracted,
            score: None,
            policy_version: None,
            created_by_run_id: None,
            supersedes_id: None,
            release_id: release,
            legacy_ref: Some(legacy_ref.into()),
        })
        .unwrap()
    }

    #[test]
    fn insert_assertion_populates_entity_ids_immediately_when_the_catalog_already_has_both_endpoints() {
        let (prov, release) = store_with_release();
        let subject_id = prov.get_or_insert_entity(&judgment_entity("a"), "judgment:1").unwrap().0;
        let object_id = prov.get_or_insert_entity(&judgment_entity("b"), "judgment:2").unwrap().0;
        let a = assertion(&prov, release, "judgment:1", "judgment:2", "d1");
        let stored = prov.get_assertion(a).unwrap();
        assert_eq!(stored.subject_entity_id, Some(subject_id));
        assert_eq!(stored.object_entity_id, Some(object_id));
    }

    #[test]
    fn insert_assertion_leaves_entity_ids_null_when_the_catalog_does_not_have_the_endpoint_yet() {
        let (prov, release) = store_with_release();
        // カタログ構築より前にアサーションだけが先に入る、という実際に
        // 起こりうる順序（`import-legacy`の後、`build-catalog`の前）。
        let a = assertion(&prov, release, "judgment:1", "judgment:2", "d1");
        let stored = prov.get_assertion(a).unwrap();
        assert_eq!(stored.subject_entity_id, None, "無い情報を捏造しない — カタログに無ければNULLのまま");
        assert_eq!(stored.object_entity_id, None);
    }

    #[test]
    fn backfill_populates_ids_once_both_endpoints_are_cataloged() {
        let (prov, release) = store_with_release();
        let a = assertion(&prov, release, "judgment:1", "judgment:2", "d1");
        let subject_id = prov.get_or_insert_entity(&judgment_entity("a"), "judgment:1").unwrap().0;
        let object_id = prov.get_or_insert_entity(&judgment_entity("b"), "judgment:2").unwrap().0;

        let stats = prov.backfill_assertion_entity_ids().unwrap();
        assert_eq!(stats.newly_backfilled, 1);
        assert_eq!(stats.already_correct, 0);
        assert_eq!(stats.unresolved, 0);

        let stored = prov.get_assertion(a).unwrap();
        assert_eq!(stored.subject_entity_id, Some(subject_id));
        assert_eq!(stored.object_entity_id, Some(object_id));

        // 再実行では「新規に埋めた」ではなく「既に正しい」と正直に報告する
        // ——`catalog_adapter.rs`のwas_new集計と同じ規律
        // (`docs/P3_STATUS.md`「毎回+Nと表示され続ける」バグの再発防止)。
        let rerun = prov.backfill_assertion_entity_ids().unwrap();
        assert_eq!(rerun.newly_backfilled, 0, "2回目は1件も新規に変わっていない");
        assert_eq!(rerun.already_correct, 1);
    }

    /// `backfill_assertion_entity_ids`の全か無かの契約:
    /// 1件でも解決できない参照があれば、解決できたぶんも含めて**一切**
    /// 書き込まない。`entities`/`entity_refs`は片方だけ実データを持ち
    /// 半端に埋まった状態にはならない——`verify-release`が1件でも失敗すれば
    /// 全体を非ゼロ終了させるのと同じ「部分的な成功を成功と呼ばない」規律。
    #[test]
    fn backfill_applies_nothing_at_all_when_any_assertion_endpoint_is_still_unresolved() {
        let (prov, release) = store_with_release();
        let a1 = assertion(&prov, release, "judgment:1", "judgment:2", "d1");
        let a2 = assertion(&prov, release, "judgment:3", "judgment:4", "d2");
        // a1の両端だけカタログに入れる。a2は両端とも未カタログのまま。
        prov.get_or_insert_entity(&judgment_entity("a"), "judgment:1").unwrap();
        prov.get_or_insert_entity(&judgment_entity("b"), "judgment:2").unwrap();

        let stats = prov.backfill_assertion_entity_ids().unwrap();
        assert_eq!(stats.newly_backfilled, 0, "a2が未解決である限り、a1すら書き込まれない");
        assert_eq!(stats.already_correct, 0);
        assert_eq!(stats.unresolved, 1);

        assert_eq!(prov.get_assertion(a1).unwrap().subject_entity_id, None, "全か無かなのでa1も更新されていないはず");
        let _ = a2;
    }

    #[test]
    fn assertion_entity_id_coverage_reports_total_and_complete_counts() {
        let (prov, release) = store_with_release();
        assertion(&prov, release, "judgment:1", "judgment:2", "d1");
        assertion(&prov, release, "judgment:3", "judgment:4", "d2");
        prov.get_or_insert_entity(&judgment_entity("a"), "judgment:1").unwrap();
        prov.get_or_insert_entity(&judgment_entity("b"), "judgment:2").unwrap();
        prov.get_or_insert_entity(&judgment_entity("c"), "judgment:3").unwrap();
        prov.get_or_insert_entity(&judgment_entity("d"), "judgment:4").unwrap();

        let (complete_before, total) = prov.assertion_entity_id_coverage().unwrap();
        assert_eq!(total, 2);
        assert_eq!(complete_before, 0, "backfillを走らせるまではまだ0件");

        prov.backfill_assertion_entity_ids().unwrap();
        let (complete_after, total_after) = prov.assertion_entity_id_coverage().unwrap();
        assert_eq!(total_after, 2);
        assert_eq!(complete_after, 2);
    }

    #[test]
    fn judgment_id_for_entity_reverses_the_canonical_ref() {
        let prov = ProvenanceStore::open_in_memory().unwrap();
        let (id, _) = prov.get_or_insert_entity(&judgment_entity("a"), "judgment:42").unwrap();
        assert_eq!(prov.judgment_id_for_entity(id).unwrap(), Some(42));
    }

    #[test]
    fn judgment_id_for_entity_is_none_for_a_concept_entity() {
        let prov = ProvenanceStore::open_in_memory().unwrap();
        let new = NewEntity { kind: EntityKind::Concept, display_label: "kahler manifold".into(), source_record_id: None };
        let (id, _) = prov.get_or_insert_entity(&new, "concept:kahler manifold").unwrap();
        assert_eq!(prov.judgment_id_for_entity(id).unwrap(), None, "concept参照にはjudgment:プレフィックスのrefが無い");
    }
}

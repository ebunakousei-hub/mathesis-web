//! P3, Increment 1（ARCHITECTURE_NEXT.md §5.2、`docs/P3_STATUS.md`）:
//! `entities`/`entity_refs`テーブルへのCRUD。`mathesis-graph`の
//! `store.rs`と同じ「専用ファイル1本にそのテーブルの操作を集める」方針。
//!
//! `entity_refs`が実質的な冪等キー——`entities`自体には自然キーが無く
//! （conceptは複数のref文字列を持ちうるため`entities`単独では一意に
//! 引けない）、必ず`entity_refs`経由で解決する。

use crate::model::{Entity, EntityId, EntityKind, NewEntity};
use crate::store::{ProvenanceStore, Result};
use rusqlite::{params, OptionalExtension};

impl ProvenanceStore {
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
}

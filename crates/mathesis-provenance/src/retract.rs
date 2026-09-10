//! P8.4（`docs/P8_4_STATUS.md`）: 「Each project should be independently
//! importable and removable」（ディレクティブ Stage 5）を実装する、この
//! リポジトリで最初の削除系プリミティブ。`import`側（`get_or_insert_*`）は
//! P0から一貫して冪等な追記専用だった——`ProvenanceStore`にDELETEが1つも
//! 無かったのはサボりではなく、これまでのどのフェーズも「取り込んだものを
//! 後で消す」必要が無かったため。ここで初めて要る。
//!
//! `retract_entity`は1entity単位の汎用プリミティブ（`math_graph_adapter`
//! に限らず、将来他のアダプタが取り込みを取り消したくなったときにも使える
//! 形にしてある）。呼び出し順は外部キー制約（`PRAGMA foreign_keys = ON`、
//! `store.rs::open`）が要求する向きそのもの——子から親へ:
//! evidence → review_decisions → relation_assertions → entity_refs →
//! entities → source_records。
//!
//! `source_records`の削除だけは「本当にもう誰も参照していないか」を
//! 呼び出し側で確かめない——DB自身のFK制約に判定を委ねる
//! （`source_record.rs::delete_source_record`のdocコメント参照）。
//! Math-Graphパイロットのデータでは実際に安全（各宣言の
//! source_recordは`(provider, provider_id=statement_id, provider_revision)`
//! で一意に1entityへ対応し、その宣言が起点の辺のevidenceだけがそれを
//! 参照する——`math_graph_adapter.rs`の設計そのもの）だが、他の
//! アダプタがsource_recordを複数entityで共有するようになった場合には、
//! このメソッドはエラーを返して中断する（サイレントな破壊よりずっと良い）。

use crate::model::EntityId;
use crate::store::{ProvenanceStore, Result};

#[derive(Debug, Default, Clone, Copy)]
pub struct RetractStats {
    pub assertions_removed: usize,
    pub evidence_removed: usize,
    pub review_decisions_removed: usize,
    pub source_record_removed: bool,
}

/// 1つのentityと、それに依存する全ての行を消す。`prov.transaction`の中で
/// 呼ぶこと——呼び出し元（`math_graph_adapter::remove_project`）が複数の
/// entityをまとめて1トランザクションにする。
pub fn retract_entity(prov: &ProvenanceStore, entity_id: EntityId) -> Result<RetractStats> {
    let mut stats = RetractStats::default();

    let assertion_ids = prov.assertion_ids_touching_entity(entity_id)?;
    for assertion_id in &assertion_ids {
        stats.evidence_removed += prov.delete_evidence_for_assertion(*assertion_id)?;
        stats.review_decisions_removed += prov.delete_review_decisions_for_assertion(*assertion_id)?;
        prov.delete_assertion(*assertion_id)?;
        stats.assertions_removed += 1;
    }

    prov.delete_entity_refs_for_entity(entity_id)?;

    let source_record_id = prov.try_get_entity(entity_id)?.and_then(|e| e.source_record_id);
    prov.delete_entity(entity_id)?;

    if let Some(source_record_id) = source_record_id {
        prov.delete_source_record(source_record_id)?;
        stats.source_record_removed = true;
    }

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math_graph_adapter::{self, ExternalClassification, PilotEdge, PilotStatement};
    use crate::model::NewRelease;

    fn stmt(id: &str, decl_name: &str) -> PilotStatement {
        PilotStatement {
            statement_id: id.into(),
            decl_name: decl_name.into(),
            module: "Test.Mod".into(),
            kind: "instance".into(),
            file_path: "Test/Mod.lean".into(),
            is_instance: true,
            repo_slug: "TestProject".into(),
            lean_toolchain: None,
            mathlib_rev: None,
            git_commit: None,
            classification: ExternalClassification::ExternalTypeclassHierarchy,
        }
    }

    #[test]
    fn retract_entity_removes_its_assertions_evidence_refs_and_source_record() {
        let prov = ProvenanceStore::open_in_memory().unwrap();
        let release = prov
            .get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None })
            .unwrap();
        let statements = vec![stmt("s1", "Foo.one"), stmt("s2", "Foo.two")];
        let edges = vec![PilotEdge { src_id: "s1".into(), dep_id: "s2".into(), edge_type: "def".into(), role: None, via_proj: false }];
        prov.transaction(|| math_graph_adapter::import_pilot(&prov, release, &statements, &edges, "rev")).unwrap();

        assert_eq!(prov.entity_count().unwrap(), 2);
        assert_eq!(prov.assertion_count().unwrap(), 1);
        assert_eq!(prov.source_record_count().unwrap(), 2);

        let s1_entity = prov.resolve_entity_ref("judgment:mathgraph:s1").unwrap().unwrap();
        let stats = prov.transaction(|| retract_entity(&prov, s1_entity)).unwrap();

        assert_eq!(stats.assertions_removed, 1, "s1 was the subject of the only assertion");
        assert_eq!(stats.evidence_removed, 1);
        assert!(stats.source_record_removed);

        assert_eq!(prov.entity_count().unwrap(), 1, "only s2's entity should remain");
        assert_eq!(prov.assertion_count().unwrap(), 0);
        assert_eq!(prov.source_record_count().unwrap(), 1, "only s2's source_record should remain");
        assert!(prov.resolve_entity_ref("judgment:mathgraph:s1").unwrap().is_none(), "s1's ref must be gone too");
        assert!(prov.resolve_entity_ref("judgment:mathgraph:s2").unwrap().is_some(), "s2 must be untouched");
    }

    #[test]
    fn retracting_an_entity_with_no_assertions_still_removes_it_cleanly() {
        let prov = ProvenanceStore::open_in_memory().unwrap();
        let release = prov
            .get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None })
            .unwrap();
        let statements = vec![stmt("s1", "Foo.lonely")];
        prov.transaction(|| math_graph_adapter::import_pilot(&prov, release, &statements, &[], "rev")).unwrap();

        let entity = prov.resolve_entity_ref("judgment:mathgraph:s1").unwrap().unwrap();
        let stats = prov.transaction(|| retract_entity(&prov, entity)).unwrap();

        assert_eq!(stats.assertions_removed, 0);
        assert!(stats.source_record_removed);
        assert_eq!(prov.entity_count().unwrap(), 0);
        assert_eq!(prov.source_record_count().unwrap(), 0);
    }
}

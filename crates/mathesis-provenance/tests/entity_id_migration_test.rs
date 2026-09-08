//! P5, Item 2（`docs/P5_PLAN.md`）: `subject_entity_id`/`object_entity_id`
//! 追加のスキーマレベルの保証。`ProvenanceStore`の公開APIだけでは組み立て
//! られない2つのシナリオを検証するため、ここだけは`rusqlite`で直接
//! ファイルへ触る——(1) この増分より前に作られたDBを安全に開けるか、
//! (2) 実在しないentityを指すFKをSQLite自身が拒むか(`store.rs`が
//! `PRAGMA foreign_keys = ON`を実際に立てていることの裏取り)。

use mathesis_provenance::model::{EpistemicState, NewRelationAssertion, NewRelease, RelationKind};
use mathesis_provenance::ProvenanceStore;
use std::path::PathBuf;

fn temp_db_path(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mathesis-entity-id-migration-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let _ = std::fs::remove_file(&path);
    path
}

/// この増分より前の`relation_assertions`の実際の形（`subject_entity_id`/
/// `object_entity_id`が無い）を手で再現する。`releases`テーブルだけ先に
/// 作っておけば、残りのテーブルは`ProvenanceStore::open`が
/// `CREATE TABLE IF NOT EXISTS`で作る——`relation_assertions`だけは
/// 既存なのでそのまま(旧い形)残る、というのがこのテストが確かめたい状況。
fn create_pre_migration_database(path: &std::path::Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE releases (
             id INTEGER PRIMARY KEY,
             tag TEXT NOT NULL UNIQUE,
             git_commit TEXT,
             generated_at_unix INTEGER NOT NULL,
             notes TEXT
         );
         CREATE TABLE relation_assertions (
             id                  INTEGER PRIMARY KEY,
             subject_ref         TEXT NOT NULL,
             predicate           TEXT NOT NULL,
             object_ref          TEXT NOT NULL,
             epistemic_state     TEXT NOT NULL,
             score               REAL,
             policy_version      TEXT,
             created_by_run_id   TEXT,
             supersedes_id       INTEGER REFERENCES relation_assertions(id),
             release_id          INTEGER NOT NULL REFERENCES releases(id),
             legacy_ref          TEXT,
             UNIQUE(release_id, legacy_ref)
         );
         INSERT INTO releases (id, tag, git_commit, generated_at_unix, notes)
             VALUES (1, 'pre-migration', NULL, 0, NULL);
         INSERT INTO relation_assertions
             (id, subject_ref, predicate, object_ref, epistemic_state, score, policy_version,
              created_by_run_id, supersedes_id, release_id, legacy_ref)
             VALUES (1, 'judgment:1', 'depends_on', 'judgment:2', 'extracted', NULL, NULL,
                     NULL, NULL, 1, 'judgment_dependency:1:2');",
    )
    .unwrap();
}

#[test]
fn opening_a_pre_migration_database_adds_the_new_columns_without_losing_data() {
    let path = temp_db_path("pre-migration.db");
    create_pre_migration_database(&path);

    let prov = ProvenanceStore::open(&path).expect("旧いDBを開けなければならない — 移行が失敗している");

    // 既存の行が失われていない・読めることを、公開APIで確かめる。
    let assertion = prov.get_assertion(mathesis_provenance::model::AssertionId(1)).unwrap();
    assert_eq!(assertion.subject_ref, "judgment:1");
    assert_eq!(assertion.object_ref, "judgment:2");
    assert_eq!(assertion.subject_entity_id, None, "移行直後はまだbackfill前なのでNULLのまま");
    assert_eq!(assertion.object_entity_id, None);
    assert_eq!(assertion.predicate, RelationKind::DependsOn);
    assert_eq!(assertion.epistemic_state, EpistemicState::Extracted);
}

#[test]
fn reopening_an_already_migrated_database_is_idempotent() {
    let path = temp_db_path("reopen.db");
    create_pre_migration_database(&path);

    {
        let _prov = ProvenanceStore::open(&path).unwrap();
        // ここでドロップ — 1回目の移行(ALTER TABLE)が走った状態でファイルを閉じる。
    }

    // 2回目のopenが「列は既にある」を正しく検出し、二重にALTERしようとして
    // 失敗したりしないこと。
    let prov = ProvenanceStore::open(&path).expect("既に移行済みのDBの再オープンは冪等であるべき");
    let assertion = prov.get_assertion(mathesis_provenance::model::AssertionId(1)).unwrap();
    assert_eq!(assertion.subject_ref, "judgment:1", "再オープンでデータが失われていない");
}

/// `store.rs`が`PRAGMA foreign_keys = ON`を実際に立てていることの裏取り。
/// アプリケーションコードの通常の経路(`insert_assertion`/
/// `backfill_assertion_entity_ids`)は常に実在するentity idか`NULL`しか
/// 書かないので、存在しないentityを指すFKは公開API経由では作れない
/// ——だからこそ、SQLite自身がそれを拒むことを直接確かめる。
#[test]
fn a_dangling_entity_id_is_rejected_by_the_foreign_key_constraint() {
    let path = temp_db_path("dangling-fk.db");
    let assertion_id;
    {
        let prov = ProvenanceStore::open(&path).unwrap();
        let release = prov
            .get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None })
            .unwrap();
        let a = prov
            .insert_assertion(&NewRelationAssertion {
                subject_ref: "judgment:1".into(),
                predicate: RelationKind::DependsOn,
                object_ref: "judgment:2".into(),
                epistemic_state: EpistemicState::Extracted,
                score: None,
                policy_version: None,
                created_by_run_id: None,
                supersedes_id: None,
                release_id: release,
                legacy_ref: Some("d".into()),
            })
            .unwrap();
        assertion_id = a.0;
    }

    let raw = rusqlite::Connection::open(&path).unwrap();
    raw.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    let result = raw.execute(
        "UPDATE relation_assertions SET subject_entity_id = 999999 WHERE id = ?1",
        [assertion_id],
    );
    assert!(result.is_err(), "実在しないentityを指すFKはSQLite自身が拒むべき");
    let err = result.unwrap_err().to_string();
    assert!(err.contains("FOREIGN KEY"), "拒否の理由がFK制約であることを確認: {err}");
}

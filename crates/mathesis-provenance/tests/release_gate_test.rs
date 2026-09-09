//! P1/P2 stabilization pass: `release_gate::verify_web_export` covers what
//! `verify.rs` alone cannot — that the *shipped* `dependencies.json`/
//! `morphisms.json`/`relations.json` still match what the live
//! `ProvenanceStore` would produce right now, not a stale snapshot from an
//! earlier commit/release. Seeds a tiny store with one dependency, one
//! morphism (heuristic-proposed), and one grounded relation, runs
//! `web_export::build_web_export`, writes it to a temp directory exactly
//! like `mathesis-provenance web-export` does, then exercises the gate
//! against both a clean copy and deliberately corrupted variants.

use mathesis_provenance::manifest::{
    input_file_hash, WebExportCounts, WebExportManifest, WEB_EXPORT_SCHEMA_VERSION,
};
use mathesis_provenance::model::{
    EntityKind, EpistemicState, EvidenceKind, NewEntity, NewEvidence, NewRelationAssertion, NewRelease, NewSourceRecord,
    RelationKind,
};
use mathesis_provenance::release_gate::verify_web_export;
use mathesis_provenance::web_export::build_web_export;
use mathesis_provenance::ProvenanceStore;
use std::path::PathBuf;

/// 依存関係1件・射1件・根拠文つき関係1件を仕込んだ`ProvenanceStore`と、
/// そのリリースタグ・idを返す。
fn seed_store() -> (ProvenanceStore, String, i64) {
    let prov = ProvenanceStore::open_in_memory().unwrap();
    let release = prov
        .get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: Some("abc123".into()), generated_at_unix: 0, notes: None })
        .unwrap();
    let source = prov
        .get_or_insert_source_record(&NewSourceRecord {
            provider: "mathesis-legacy-snapshot".into(),
            provider_id: "t".into(),
            provider_revision: None,
            retrieved_at_unix: None,
            content_hash: None,
            licence: None,
            attribution: None,
            raw_payload_uri: None,
            adapter_name: "test".into(),
            adapter_version: "0".into(),
            parser_version: None,
            reproducibility_json: None,
        })
        .unwrap();

    // P5, Item 2 step 4（`docs/P5_PLAN.md`）: `build_web_export`は今や
    // `subject_entity_id`/`object_entity_id`(FK)経由でしか辺を組み立てない
    // ——`import-legacy`の後`build-catalog`を走らせる実運用と同じ順で、
    // assertionを挿入する前にカタログへ登録しておく。
    for (kind, ref_string, label) in [
        (EntityKind::Judgment, "judgment:1", "j1"),
        (EntityKind::Judgment, "judgment:2", "j2"),
        (EntityKind::Judgment, "judgment:3", "j3"),
        (EntityKind::Judgment, "judgment:4", "j4"),
        (EntityKind::Concept, "concept:a", "a"),
        (EntityKind::Concept, "concept:b", "b"),
    ] {
        prov.get_or_insert_entity(&NewEntity { kind, display_label: label.into(), source_record_id: None }, ref_string)
            .unwrap();
    }

    let dep = prov
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
            legacy_ref: Some("judgment_dependency:1:2".into()),
        })
        .unwrap();
    prov.insert_evidence(&NewEvidence {
        assertion_id: dep,
        source_record_id: source,
        locator: Some("f.lean:1".into()),
        evidence_kind: EvidenceKind::SourceSpan,
        extractor_or_model: None,
        version: None,
        input_hash: None,
        output_hash: None,
        metric_name: None,
        metric_value: None,
        dependency_origin: None,
        external_classification: None,
    })
    .unwrap();

    let morphism = prov
        .insert_assertion(&NewRelationAssertion {
            subject_ref: "judgment:3".into(),
            predicate: RelationKind::Specializes,
            object_ref: "judgment:4".into(),
            epistemic_state: EpistemicState::Proposed,
            score: None,
            policy_version: None,
            created_by_run_id: None,
            supersedes_id: None,
            release_id: release,
            legacy_ref: Some("morphism:1".into()),
        })
        .unwrap();
    prov.insert_evidence(&NewEvidence {
        assertion_id: morphism,
        source_record_id: source,
        locator: None,
        evidence_kind: EvidenceKind::ModelOutput,
        extractor_or_model: Some("mathesis-graph::heuristics".into()),
        version: None,
        input_hash: None,
        output_hash: None,
        metric_name: None,
        metric_value: None,
        dependency_origin: None,
        external_classification: None,
    })
    .unwrap();

    let relation = prov
        .insert_assertion(&NewRelationAssertion {
            subject_ref: "concept:a".into(),
            predicate: RelationKind::Specializes,
            object_ref: "concept:b".into(),
            epistemic_state: EpistemicState::Extracted,
            score: None,
            policy_version: None,
            created_by_run_id: None,
            supersedes_id: None,
            release_id: release,
            legacy_ref: Some("concept_relation:a|b|specialization_of".into()),
        })
        .unwrap();
    prov.insert_evidence(&NewEvidence {
        assertion_id: relation,
        source_record_id: source,
        locator: Some("a is a special case of b.".into()),
        evidence_kind: EvidenceKind::SourceSpan,
        extractor_or_model: Some("hearst-pattern".into()),
        version: None,
        input_hash: None,
        output_hash: None,
        metric_name: None,
        metric_value: None,
        dependency_origin: None,
        external_classification: None,
    })
    .unwrap();

    (prov, "t".to_string(), release.0)
}

/// `web-export`のCLIが実際にやること(build_web_export → 3ファイル書き出し
/// → マニフェスト作成)を、一時ディレクトリに対してそのまま再現する。
fn write_web_export(prov: &ProvenanceStore, release_tag: &str, release_id: i64, dir: &std::path::Path) -> WebExportManifest {
    let release = mathesis_provenance::model::ReleaseId(release_id);
    let export = build_web_export(prov, release).unwrap();

    let dep_path = dir.join("dependencies.json");
    let morph_path = dir.join("morphisms.json");
    let rel_path = dir.join("relations.json");
    std::fs::write(&dep_path, serde_json::to_string(&export.dependencies).unwrap()).unwrap();
    std::fs::write(&morph_path, serde_json::to_string(&export.morphisms).unwrap()).unwrap();
    std::fs::write(&rel_path, serde_json::to_string(&export.relations).unwrap()).unwrap();

    WebExportManifest {
        schema_version: WEB_EXPORT_SCHEMA_VERSION,
        release_tag: release_tag.to_string(),
        release_id,
        release_git_commit: Some("abc123".into()),
        web_export_version: "test".into(),
        generated_at_unix: 0,
        counts: WebExportCounts { dependencies: export.dependencies.len(), morphisms: export.morphisms.len(), relations: export.relations.len() },
        output_files: vec![input_file_hash(&dep_path).unwrap(), input_file_hash(&morph_path).unwrap(), input_file_hash(&rel_path).unwrap()],
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mathesis-provenance-release-gate-test-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn happy_path_passes_every_check() {
    let (prov, tag, release_id) = seed_store();
    let dir = temp_dir("happy");
    let manifest = write_web_export(&prov, &tag, release_id, &dir);

    let failures = verify_web_export(&prov, &tag, &manifest, &dir).unwrap();
    assert!(failures.is_empty(), "expected clean pass, got: {failures:?}");
}

#[test]
fn detects_output_hash_drift_from_hand_edited_file() {
    let (prov, tag, release_id) = seed_store();
    let dir = temp_dir("hash-drift");
    let manifest = write_web_export(&prov, &tag, release_id, &dir);

    // ファイルをマニフェスト作成後に書き換える(手編集・転送破損のシミュレーション)。
    std::fs::write(dir.join("morphisms.json"), "[]").unwrap();

    let failures = verify_web_export(&prov, &tag, &manifest, &dir).unwrap();
    assert!(failures.iter().any(|f| f.check == "output_hash_mismatch"), "{failures:?}");
}

#[test]
fn detects_stale_export_after_the_store_changes() {
    let (prov, tag, release_id) = seed_store();
    let dir = temp_dir("stale");
    let manifest = write_web_export(&prov, &tag, release_id, &dir);

    // マニフェスト作成後にDB側へ新しい射を追加——`web-export`の再実行を
    // 忘れたまま古いファイルを配布してしまうシナリオ。カタログにも登録する
    // (P5, Item 2 step 4以降、未カタログの新規assertionはFKが無いため
    // `build_web_export`から素通しで除外され、この「変化」自体が起きなく
    // なってしまう——このテストが検査したいのはあくまで「有効な新規辺が
    // 増えたのに古いファイルのまま」というシナリオなので、実運用同様
    // カタログ登録込みで新規assertionを仕込む)。
    prov.get_or_insert_entity(
        &mathesis_provenance::model::NewEntity { kind: EntityKind::Judgment, display_label: "j5".into(), source_record_id: None },
        "judgment:5",
    )
    .unwrap();
    prov.get_or_insert_entity(
        &mathesis_provenance::model::NewEntity { kind: EntityKind::Judgment, display_label: "j6".into(), source_record_id: None },
        "judgment:6",
    )
    .unwrap();
    let source = prov
        .get_or_insert_source_record(&NewSourceRecord {
            provider: "mathesis-legacy-snapshot".into(),
            provider_id: "t".into(),
            provider_revision: None,
            retrieved_at_unix: None,
            content_hash: None,
            licence: None,
            attribution: None,
            raw_payload_uri: None,
            adapter_name: "test".into(),
            adapter_version: "0".into(),
            parser_version: None,
            reproducibility_json: None,
        })
        .unwrap();
    let new_morphism = prov
        .insert_assertion(&NewRelationAssertion {
            subject_ref: "judgment:5".into(),
            predicate: RelationKind::EquivalentTo,
            object_ref: "judgment:6".into(),
            epistemic_state: EpistemicState::Proposed,
            score: None,
            policy_version: None,
            created_by_run_id: None,
            supersedes_id: None,
            release_id: mathesis_provenance::model::ReleaseId(release_id),
            legacy_ref: Some("morphism:2".into()),
        })
        .unwrap();
    prov.insert_evidence(&NewEvidence {
        assertion_id: new_morphism,
        source_record_id: source,
        locator: None,
        evidence_kind: EvidenceKind::ModelOutput,
        extractor_or_model: None,
        version: None,
        input_hash: None,
        output_hash: None,
        metric_name: None,
        metric_value: None,
        dependency_origin: None,
        external_classification: None,
    })
    .unwrap();

    let failures = verify_web_export(&prov, &tag, &manifest, &dir).unwrap();
    assert!(failures.iter().any(|f| f.check == "web_export_stale" || f.check == "counts_mismatch"), "{failures:?}");
}

#[test]
fn detects_a_partially_copied_export_directory() {
    let (prov, tag, release_id) = seed_store();
    let dir = temp_dir("partial-copy");
    let manifest = write_web_export(&prov, &tag, release_id, &dir);

    std::fs::remove_file(dir.join("relations.json")).unwrap();

    let failures = verify_web_export(&prov, &tag, &manifest, &dir).unwrap();
    assert!(failures.iter().any(|f| f.check == "web_export_file_missing"), "{failures:?}");
}

#[test]
fn detects_unknown_schema_version() {
    let (prov, tag, release_id) = seed_store();
    let dir = temp_dir("schema-version");
    let mut manifest = write_web_export(&prov, &tag, release_id, &dir);
    manifest.schema_version = 999;

    let failures = verify_web_export(&prov, &tag, &manifest, &dir).unwrap();
    assert!(failures.iter().any(|f| f.check == "unknown_web_export_schema_version"), "{failures:?}");
}

#[test]
fn detects_release_tag_mismatch_against_the_expected_release() {
    let (prov, tag, release_id) = seed_store();
    let dir = temp_dir("release-mismatch");
    let manifest = write_web_export(&prov, &tag, release_id, &dir);

    // 呼び出し側(CIの--release引数)は別のタグを期待している——マニフェスト
    // 自身のタグと食い違う。
    let failures = verify_web_export(&prov, "some-other-release", &manifest, &dir).unwrap();
    assert!(failures.iter().any(|f| f.check == "web_export_release_mismatch"), "{failures:?}");
}

#[test]
fn detects_malformed_json() {
    let (prov, tag, release_id) = seed_store();
    let dir = temp_dir("malformed");
    let manifest = write_web_export(&prov, &tag, release_id, &dir);

    // マニフェストのハッシュと一致しない壊れたJSONに差し替える——
    // 手順としては「壊れたファイルにハッシュだけ再計算し忘れた」状態。
    std::fs::write(dir.join("dependencies.json"), "{not valid json").unwrap();

    let failures = verify_web_export(&prov, &tag, &manifest, &dir).unwrap();
    assert!(failures.iter().any(|f| f.check == "output_hash_mismatch" || f.check == "malformed_json"), "{failures:?}");
}

/// P7.4（`docs/P7_4_STATUS.md`「repeated stale relations.json issue」）:
/// P6.3で実際に production で起きたバグの再現テスト——`RelationEdge`へ
/// `traversalPolicy`フィールドを追加した後、`web/public/relations.json`が
/// 一度も再生成されず、`verify-release`も再実行されずに放置された。
/// この回帰は「行数が変わった」(`detects_stale_export_after_the_store_
/// changes`が既にカバー)ではなく「同じ行数・同じ内容だが、各行の
/// フィールド形状だけが古い」——ハッシュはその(古い形状の)ファイル自身に
/// 対しては正しく計算し直されている(=`output_hash_mismatch`は起きない)
/// という、より見つけにくいケース。`check_output_file`の構造比較
/// (`on_disk != live_json`)がハッシュ一致とは独立にこれを捕まえることを
/// 固定する。
#[test]
fn detects_a_field_added_to_the_row_schema_even_when_the_stale_files_own_hash_is_self_consistent() {
    let (prov, tag, release_id) = seed_store();
    let dir = temp_dir("schema-drift");
    let manifest = write_web_export(&prov, &tag, release_id, &dir);

    // 実際に生成された relations.json を読み、"traversalPolicy" フィールドを
    // 落とした「旧スキーマの」版に書き換える——行数・その他のフィールドは
    // 変えない。ハッシュはこの(古い形状の)バイト列に対して正しく再計算する
    // ——「再生成はしたが、古いコードのビルドで再生成した」を模する。
    let rel_path = dir.join("relations.json");
    let live: Vec<serde_json::Value> = serde_json::from_slice(&std::fs::read(&rel_path).unwrap()).unwrap();
    assert!(!live.is_empty(), "seed_store must produce at least one relation for this test to mean anything");
    let stale: Vec<serde_json::Value> = live
        .into_iter()
        .map(|mut row| {
            row.as_object_mut().unwrap().remove("traversalPolicy");
            row
        })
        .collect();
    let stale_bytes = serde_json::to_vec(&stale).unwrap();
    std::fs::write(&rel_path, &stale_bytes).unwrap();
    let mut manifest = manifest;
    let stale_hash = mathesis_provenance::manifest::sha256_file(&rel_path).unwrap();
    for f in &mut manifest.output_files {
        if f.path.ends_with("relations.json") {
            f.sha256 = stale_hash.clone();
        }
    }

    let failures = verify_web_export(&prov, &tag, &manifest, &dir).unwrap();
    assert!(
        failures.iter().any(|f| f.check == "web_export_stale" && f.detail.contains("relations.json")),
        "a row-schema change (missing field) with a self-consistent hash must still be caught: {failures:?}"
    );
    assert!(
        !failures.iter().any(|f| f.check == "output_hash_mismatch"),
        "this scenario specifically has a correctly-recomputed hash — hash mismatch must not be why it's caught: {failures:?}"
    );
}

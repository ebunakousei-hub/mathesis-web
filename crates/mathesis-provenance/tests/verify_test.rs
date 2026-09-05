//! `verify_release`の完全性ゲートを、正常系1本と外部レビュー(2026-09-05)が
//! 挙げた異常系で確かめる。`verify_release`は純粋にDB+デシリアライズ済みの
//! サイドカー構造体だけを見るので、`GraphStore`/`TaxonomyStore`の実データは
//! 要らない——`ProvenanceStore`に直接assertionを仕込むだけで足りる。

use mathesis_provenance::manifest::{InputFileHash, ManifestCounts, ProvenanceManifest, SCHEMA_VERSION};
use mathesis_provenance::model::{
    EpistemicState, EvidenceKind, NewEvidence, NewRelationAssertion, NewRelease, NewSourceRecord, RelationKind,
};
use mathesis_provenance::reconcile::{DependencyProvenance, JudgmentsProvenanceExport, MorphismProvenance, RelationsProvenanceExport};
use mathesis_provenance::verify::{verify_release, VerifyInputs};
use mathesis_provenance::ProvenanceStore;

/// 1件のdependency由来assertion(evidence込み)を仕込んで、そのassertion_idを返す。
fn seed_one_dependency_assertion(prov: &ProvenanceStore, release_id: mathesis_provenance::ReleaseId) -> i64 {
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
        })
        .unwrap();
    let assertion = prov
        .insert_assertion(&NewRelationAssertion {
            subject_ref: "judgment:1".into(),
            predicate: RelationKind::DependsOn,
            object_ref: "judgment:2".into(),
            epistemic_state: EpistemicState::Extracted,
            score: None,
            policy_version: None,
            created_by_run_id: None,
            supersedes_id: None,
            release_id,
            legacy_ref: Some("judgment_dependency:1:2".into()),
        })
        .unwrap();
    prov.insert_evidence(&NewEvidence {
        assertion_id: assertion,
        source_record_id: source,
        locator: Some("f.lean:1".into()),
        evidence_kind: EvidenceKind::SourceSpan,
        extractor_or_model: None,
        version: None,
        input_hash: None,
        output_hash: None,
        metric_name: None,
        metric_value: None,
    })
    .unwrap();
    assertion.0
}

fn base_manifest(release_id: i64) -> ProvenanceManifest {
    ProvenanceManifest {
        release_tag: "t".into(),
        release_id,
        release_git_commit: None,
        source_database_schema: SCHEMA_VERSION,
        adapter_name: "test".into(),
        adapter_version: "0".into(),
        input_files: vec![],
        generated_at_unix: 0,
        counts: ManifestCounts { dependencies: 1, citations: 0, morphisms: 0, relations: 0 },
    }
}

fn base_judgments_sidecar(assertion_id: i64) -> JudgmentsProvenanceExport {
    JudgmentsProvenanceExport {
        release_tag: "t".into(),
        release_git_commit: None,
        dependencies: vec![DependencyProvenance { from: 1, to: 2, assertion_id }],
        citations: vec![],
        morphisms: vec![],
    }
}

fn empty_relations_sidecar() -> RelationsProvenanceExport {
    RelationsProvenanceExport { release_tag: "t".into(), release_git_commit: None, relations: vec![] }
}

#[test]
fn happy_path_passes_every_check() {
    let prov = ProvenanceStore::open_in_memory().unwrap();
    let release =
        prov.get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None }).unwrap();
    let assertion_id = seed_one_dependency_assertion(&prov, release);

    let manifest = base_manifest(release.0);
    let judgments = base_judgments_sidecar(assertion_id);
    let relations = empty_relations_sidecar();

    let report = verify_release(
        &prov,
        &VerifyInputs { manifest: &manifest, judgments_provenance: &judgments, relations_provenance: &relations, live_input_files: &[] },
    )
    .unwrap();
    assert!(report.is_ok(), "expected clean pass, got: {:?}", report.failures);
    assert_eq!(report.checked_edges, 1);
}

#[test]
fn detects_sidecar_from_a_different_release() {
    let prov = ProvenanceStore::open_in_memory().unwrap();
    let release =
        prov.get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None }).unwrap();
    let assertion_id = seed_one_dependency_assertion(&prov, release);

    let manifest = base_manifest(release.0);
    let mut judgments = base_judgments_sidecar(assertion_id);
    judgments.release_tag = "some-other-release".into(); // サイドカーだけ別リリースを名乗る
    let relations = empty_relations_sidecar();

    let report = verify_release(
        &prov,
        &VerifyInputs { manifest: &manifest, judgments_provenance: &judgments, relations_provenance: &relations, live_input_files: &[] },
    )
    .unwrap();
    assert!(!report.is_ok());
    assert!(report.failures.iter().any(|f| f.check == "sidecar_release_mismatch"));
}

#[test]
fn detects_missing_assertion() {
    let prov = ProvenanceStore::open_in_memory().unwrap();
    let release =
        prov.get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None }).unwrap();
    // 実在しないassertion_idを指すサイドカー(DBには何も仕込まない)。
    let manifest = base_manifest(release.0);
    let judgments = base_judgments_sidecar(999_999);
    let relations = empty_relations_sidecar();

    let report = verify_release(
        &prov,
        &VerifyInputs { manifest: &manifest, judgments_provenance: &judgments, relations_provenance: &relations, live_input_files: &[] },
    )
    .unwrap();
    assert!(!report.is_ok());
    assert!(report.failures.iter().any(|f| f.check == "assertion_missing"));
}

#[test]
fn detects_assertion_without_evidence() {
    let prov = ProvenanceStore::open_in_memory().unwrap();
    let release =
        prov.get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None }).unwrap();
    // evidenceを一切挿入しないassertion。
    let assertion = prov
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

    let manifest = base_manifest(release.0);
    let judgments = base_judgments_sidecar(assertion.0);
    let relations = empty_relations_sidecar();

    let report = verify_release(
        &prov,
        &VerifyInputs { manifest: &manifest, judgments_provenance: &judgments, relations_provenance: &relations, live_input_files: &[] },
    )
    .unwrap();
    assert!(!report.is_ok());
    assert!(report.failures.iter().any(|f| f.check == "assertion_without_evidence"));
}

#[test]
fn detects_ambiguous_duplicate_identity_key() {
    let prov = ProvenanceStore::open_in_memory().unwrap();
    let release =
        prov.get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None }).unwrap();
    let a1 = seed_one_dependency_assertion(&prov, release);
    // 2件目の(別)assertionを仕込み、同じmorphism_idで別のassertion_idを指す
    // サイドカーを組み立てる——鍵は同じなのに解決先が割れている状態。
    let a2 = {
        let source = prov
            .get_or_insert_source_record(&NewSourceRecord {
                provider: "mathesis-legacy-snapshot".into(),
                provider_id: "t2".into(),
                provider_revision: None,
                retrieved_at_unix: None,
                content_hash: None,
                licence: None,
                attribution: None,
                raw_payload_uri: None,
                adapter_name: "test".into(),
                adapter_version: "0".into(),
                parser_version: None,
            })
            .unwrap();
        let assertion = prov
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
            assertion_id: assertion,
            source_record_id: source,
            locator: None,
            evidence_kind: EvidenceKind::ModelOutput,
            extractor_or_model: None,
            version: None,
            input_hash: None,
            output_hash: None,
            metric_name: None,
            metric_value: None,
        })
        .unwrap();
        assertion.0
    };

    let manifest = ProvenanceManifest { counts: ManifestCounts { dependencies: 1, citations: 0, morphisms: 2, relations: 0 }, ..base_manifest(release.0) };
    let mut judgments = base_judgments_sidecar(a1);
    // 同じmorphism_id=1が2件、別々のassertionを指す——曖昧な解決。
    judgments.morphisms.push(MorphismProvenance { morphism_id: 1, assertion_id: a1 });
    judgments.morphisms.push(MorphismProvenance { morphism_id: 1, assertion_id: a2 });
    let relations = empty_relations_sidecar();

    let report = verify_release(
        &prov,
        &VerifyInputs { manifest: &manifest, judgments_provenance: &judgments, relations_provenance: &relations, live_input_files: &[] },
    )
    .unwrap();
    assert!(!report.is_ok());
    assert!(report.failures.iter().any(|f| f.check == "ambiguous_identity_key"));
}

#[test]
fn detects_counts_mismatch() {
    let prov = ProvenanceStore::open_in_memory().unwrap();
    let release =
        prov.get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None }).unwrap();
    let assertion_id = seed_one_dependency_assertion(&prov, release);

    // マニフェストは2件と主張するが、サイドカーは実際には1件しか無い。
    let manifest = ProvenanceManifest { counts: ManifestCounts { dependencies: 2, citations: 0, morphisms: 0, relations: 0 }, ..base_manifest(release.0) };
    let judgments = base_judgments_sidecar(assertion_id);
    let relations = empty_relations_sidecar();

    let report = verify_release(
        &prov,
        &VerifyInputs { manifest: &manifest, judgments_provenance: &judgments, relations_provenance: &relations, live_input_files: &[] },
    )
    .unwrap();
    assert!(!report.is_ok());
    assert!(report.failures.iter().any(|f| f.check == "counts_mismatch"));
}

#[test]
fn detects_input_file_hash_drift() {
    let prov = ProvenanceStore::open_in_memory().unwrap();
    let release =
        prov.get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None }).unwrap();
    let assertion_id = seed_one_dependency_assertion(&prov, release);

    let mut manifest = base_manifest(release.0);
    manifest.input_files.push(InputFileHash { path: "judgments.db".into(), sha256: "aaaa".into() });
    let judgments = base_judgments_sidecar(assertion_id);
    let relations = empty_relations_sidecar();

    // 「今のファイル」は違うハッシュを持つ、というシナリオ(入力が生成後に変わった)。
    let live = vec![InputFileHash { path: "judgments.db".into(), sha256: "bbbb".into() }];

    let report = verify_release(
        &prov,
        &VerifyInputs { manifest: &manifest, judgments_provenance: &judgments, relations_provenance: &relations, live_input_files: &live },
    )
    .unwrap();
    assert!(!report.is_ok());
    assert!(report.failures.iter().any(|f| f.check == "input_file_hash_mismatch"));
}

#[test]
fn detects_manifest_release_id_mismatch_against_db() {
    let prov = ProvenanceStore::open_in_memory().unwrap();
    let release =
        prov.get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None }).unwrap();
    let assertion_id = seed_one_dependency_assertion(&prov, release);

    // マニフェストのrelease_idを実際の値とずらす。
    let manifest = ProvenanceManifest { release_id: release.0 + 999, ..base_manifest(release.0) };
    let judgments = base_judgments_sidecar(assertion_id);
    let relations = empty_relations_sidecar();

    let report = verify_release(
        &prov,
        &VerifyInputs { manifest: &manifest, judgments_provenance: &judgments, relations_provenance: &relations, live_input_files: &[] },
    )
    .unwrap();
    assert!(!report.is_ok());
    assert!(report.failures.iter().any(|f| f.check == "manifest_release_id_mismatch"));
}

/// `RelationKind::from_str`/`EpistemicState::from_str`は未知の文字列に
/// `None`を返す(パニックしない)——一方でこれらを読み出す`assertion_row`は
/// `mathesis-graph`と同じ規約で`.expect(...)`する（自分で書いた値は常に
/// 既知のはずという前提）。DBが外部から直接書き換えられて破損した場合は
/// パニックが正しい合図——`verify`はそのような壊れたDBを黙って通さない、
/// という現在の(意図的な)挙動をここで固定する。
#[test]
fn unknown_relation_kind_string_does_not_silently_succeed() {
    assert_eq!(RelationKind::from_str("not_a_real_kind"), None);
    assert_eq!(EpistemicState::from_str("not_a_real_state"), None);
}

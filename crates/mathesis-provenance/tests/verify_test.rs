//! `verify_release`の完全性ゲートを、正常系1本と外部レビュー(2026-09-05)が
//! 挙げた異常系で確かめる。`verify_release`は純粋にDB+デシリアライズ済みの
//! サイドカー構造体だけを見るので、`GraphStore`/`TaxonomyStore`の実データは
//! 要らない——`ProvenanceStore`に直接assertionを仕込むだけで足りる。

use mathesis_provenance::manifest::{InputFileHash, ManifestCounts, ProvenanceManifest, SCHEMA_VERSION};
use mathesis_provenance::model::{
    EntityKind, EpistemicState, EvidenceKind, NewEntity, NewEvidence, NewRelationAssertion, NewRelease, NewSourceRecord, RelationKind,
};
use mathesis_provenance::relation_policy::SOURCE_MAPPING_POLICY_VERSION;
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
            reproducibility_json: None,
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
        dependency_origin: None,
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
        source_mapping_policy_version: String::new(),
        input_files: vec![],
        generated_at_unix: 0,
        counts: ManifestCounts { dependencies: 1, citations: 0, morphisms: 0, relations: 0 },
        catalog: None,
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
        &VerifyInputs { manifest: &manifest, judgments_provenance: &judgments, relations_provenance: &relations, live_input_files: &[], now_unix: 0 },
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
        &VerifyInputs { manifest: &manifest, judgments_provenance: &judgments, relations_provenance: &relations, live_input_files: &[], now_unix: 0 },
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
        &VerifyInputs { manifest: &manifest, judgments_provenance: &judgments, relations_provenance: &relations, live_input_files: &[], now_unix: 0 },
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
        &VerifyInputs { manifest: &manifest, judgments_provenance: &judgments, relations_provenance: &relations, live_input_files: &[], now_unix: 0 },
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
                reproducibility_json: None,
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
            dependency_origin: None,
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
        &VerifyInputs { manifest: &manifest, judgments_provenance: &judgments, relations_provenance: &relations, live_input_files: &[], now_unix: 0 },
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
        &VerifyInputs { manifest: &manifest, judgments_provenance: &judgments, relations_provenance: &relations, live_input_files: &[], now_unix: 0 },
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
        &VerifyInputs { manifest: &manifest, judgments_provenance: &judgments, relations_provenance: &relations, live_input_files: &live, now_unix: 0 },
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
        &VerifyInputs { manifest: &manifest, judgments_provenance: &judgments, relations_provenance: &relations, live_input_files: &[], now_unix: 0 },
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

#[test]
fn rejects_catalog_with_unresolved_assertion_endpoint() {
    let prov = ProvenanceStore::open_in_memory().unwrap();
    let release = prov
        .get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None })
        .unwrap();
    let assertion_id = seed_one_dependency_assertion(&prov, release);
    prov.get_or_insert_entity(
        &NewEntity { kind: EntityKind::Judgment, display_label: "known".into(), source_record_id: None },
        "judgment:1",
    )
    .unwrap();

    let manifest = base_manifest(release.0);
    let report = verify_release(
        &prov,
        &VerifyInputs {
            manifest: &manifest,
            judgments_provenance: &base_judgments_sidecar(assertion_id),
            relations_provenance: &empty_relations_sidecar(),
            live_input_files: &[],
            now_unix: 0,
        },
    )
    .unwrap();
    assert!(report.failures.iter().any(|f| f.check == "unresolved_entity_reference"));
}

/// P5, Item 2（`docs/P5_PLAN.md`）: `judgment:1`/`judgment:2`とも
/// `insert_assertion`の時点ではまだカタログに無い(=`subject_entity_id`/
/// `object_entity_id`はNULLのまま挿入される)。その後カタログに両方とも
/// 追加されても、`backfill_assertion_entity_ids`を走らせない限り
/// アサーション自身のFK列は古い(NULLの)ままなので、`verify_release`は
/// これを"unresolved"ではなく"drift"(参照は解決できるのにFK列と食い違う)
/// として報告すべき——実運用で`import-legacy`の後`build-catalog`を
/// 忘れた/失敗したケースをそのまま再現している。
#[test]
fn detects_missing_entity_id_as_drift_once_the_catalog_can_resolve_both_endpoints() {
    let prov = ProvenanceStore::open_in_memory().unwrap();
    let release = prov
        .get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None })
        .unwrap();
    let assertion_id = seed_one_dependency_assertion(&prov, release);
    prov.get_or_insert_entity(
        &NewEntity { kind: EntityKind::Judgment, display_label: "one".into(), source_record_id: None },
        "judgment:1",
    )
    .unwrap();
    prov.get_or_insert_entity(
        &NewEntity { kind: EntityKind::Judgment, display_label: "two".into(), source_record_id: None },
        "judgment:2",
    )
    .unwrap();
    // 意図的に`backfill_assertion_entity_ids`を呼ばない——FK列が古いまま
    // 残っている状態を再現する。

    let manifest = base_manifest(release.0);
    let report = verify_release(
        &prov,
        &VerifyInputs {
            manifest: &manifest,
            judgments_provenance: &base_judgments_sidecar(assertion_id),
            relations_provenance: &empty_relations_sidecar(),
            live_input_files: &[],
            now_unix: 0,
        },
    )
    .unwrap();
    let drift_failures: Vec<_> = report.failures.iter().filter(|f| f.check == "entity_endpoint_drift").collect();
    assert_eq!(drift_failures.len(), 2, "subject/objectの両方がdriftとして検出されるべき: {:?}", report.failures);
}

/// 上のテストの裏返し: `backfill_assertion_entity_ids`を実際に走らせれば、
/// 同じ状況からdriftが0件になる——ゲートが「壊れた状態を検出する」だけで
/// なく「正しく直した状態を誤検出しない」ことも確かめる。
#[test]
fn backfilling_before_verify_release_clears_the_drift_that_would_otherwise_be_reported() {
    let prov = ProvenanceStore::open_in_memory().unwrap();
    let release = prov
        .get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None })
        .unwrap();
    let assertion_id = seed_one_dependency_assertion(&prov, release);
    prov.get_or_insert_entity(
        &NewEntity { kind: EntityKind::Judgment, display_label: "one".into(), source_record_id: None },
        "judgment:1",
    )
    .unwrap();
    prov.get_or_insert_entity(
        &NewEntity { kind: EntityKind::Judgment, display_label: "two".into(), source_record_id: None },
        "judgment:2",
    )
    .unwrap();
    let stats = prov.backfill_assertion_entity_ids().unwrap();
    assert_eq!((stats.newly_backfilled, stats.already_correct, stats.unresolved), (1, 0, 0));

    let manifest = base_manifest(release.0);
    let report = verify_release(
        &prov,
        &VerifyInputs {
            manifest: &manifest,
            judgments_provenance: &base_judgments_sidecar(assertion_id),
            relations_provenance: &empty_relations_sidecar(),
            live_input_files: &[],
            now_unix: 0,
        },
    )
    .unwrap();
    assert!(report.is_ok(), "backfill済みならdriftは0件のはず: {:?}", report.failures);
}

#[test]
fn rejects_manifest_with_unknown_mapping_policy() {
    let prov = ProvenanceStore::open_in_memory().unwrap();
    let release = prov
        .get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None })
        .unwrap();
    let assertion_id = seed_one_dependency_assertion(&prov, release);
    let mut manifest = base_manifest(release.0);
    manifest.source_mapping_policy_version = format!("{SOURCE_MAPPING_POLICY_VERSION}-future");
    let report = verify_release(
        &prov,
        &VerifyInputs {
            manifest: &manifest,
            judgments_provenance: &base_judgments_sidecar(assertion_id),
            relations_provenance: &empty_relations_sidecar(),
            live_input_files: &[],
            now_unix: 0,
        },
    )
    .unwrap();
    assert!(report.failures.iter().any(|f| f.check == "source_mapping_policy_mismatch"));
}

// Priority 2, step 2（ユーザー指示 2026-09-08）: 「extracted/proposedな辺が
// 事故で信頼される側へ紛れ込めない」ことを確かめる異常系群。

/// `Observed`(既定トラバース対象になるべき状態)なのに、根拠が
/// テキスト抽出由来(SourceSpan)しか無い——`epistemic_state`だけ手違いで
/// 書き換わり、根拠がついてきていない事故を想定。
#[test]
fn rejects_a_default_traversal_assertion_backed_only_by_text_extraction_evidence() {
    let prov = ProvenanceStore::open_in_memory().unwrap();
    let release = prov
        .get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None })
        .unwrap();
    let source = prov
        .get_or_insert_source_record(&NewSourceRecord {
            provider: "test".into(), provider_id: "t".into(), provider_revision: None, retrieved_at_unix: None,
            content_hash: None, licence: None, attribution: None, raw_payload_uri: None,
            adapter_name: "test".into(), adapter_version: "0".into(), parser_version: None,
            reproducibility_json: None,
        })
        .unwrap();
    let assertion_id = prov
        .insert_assertion(&NewRelationAssertion {
            subject_ref: "judgment:1".into(),
            predicate: RelationKind::DependsOn,
            object_ref: "judgment:2".into(),
            epistemic_state: EpistemicState::Observed, // 既定トラバース対象になる状態
            score: None, policy_version: None, created_by_run_id: None, supersedes_id: None,
            release_id: release, legacy_ref: Some("d1".into()),
        })
        .unwrap();
    prov.insert_evidence(&mathesis_provenance::model::NewEvidence {
        assertion_id, source_record_id: source, locator: Some("looks like text-extraction, not a Lean manifest".into()),
        evidence_kind: EvidenceKind::SourceSpan, // FormalExportではない
        extractor_or_model: None, version: None, input_hash: None, output_hash: None, metric_name: None, metric_value: None,
        dependency_origin: None,
    })
    .unwrap();

    let manifest = base_manifest(release.0);
    let report = verify_release(
        &prov,
        &VerifyInputs {
            manifest: &manifest,
            judgments_provenance: &base_judgments_sidecar(assertion_id.0),
            relations_provenance: &empty_relations_sidecar(),
            live_input_files: &[],
            now_unix: 0,
        },
    )
    .unwrap();
    assert!(
        report.failures.iter().any(|f| f.check == "trusted_assertion_missing_qualifying_evidence"),
        "observedなのにFormalExport証拠が無い場合は拒否すべき: {:?}",
        report.failures
    );
}

/// レビュー済みで既定トラバース対象になりうる意味的関係だが、
/// ReviewDecisionにreviewer_idが無い(誰が承認したか分からない)——
/// 「本人確認済みレビュー」の要件を満たさない。
#[test]
fn rejects_a_default_traversal_assertion_whose_review_decision_has_no_reviewer_identity() {
    use mathesis_provenance::model::{NewReviewDecision, ReviewOutcome};

    let prov = ProvenanceStore::open_in_memory().unwrap();
    let release = prov
        .get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None })
        .unwrap();
    let source = prov
        .get_or_insert_source_record(&NewSourceRecord {
            provider: "test".into(), provider_id: "t".into(), provider_revision: None, retrieved_at_unix: None,
            content_hash: None, licence: None, attribution: None, raw_payload_uri: None,
            adapter_name: "test".into(), adapter_version: "0".into(), parser_version: None,
            reproducibility_json: None,
        })
        .unwrap();
    let assertion_id = prov
        .insert_assertion(&NewRelationAssertion {
            subject_ref: "concept:a".into(),
            predicate: RelationKind::Specializes,
            object_ref: "concept:b".into(),
            epistemic_state: EpistemicState::Reviewed, // Specializesはreviewed/verifiedでdefault_traversal
            score: None, policy_version: None, created_by_run_id: None, supersedes_id: None,
            release_id: release, legacy_ref: Some("r1".into()),
        })
        .unwrap();
    prov.insert_evidence(&mathesis_provenance::model::NewEvidence {
        assertion_id, source_record_id: source, locator: Some("a is a special case of b".into()),
        evidence_kind: EvidenceKind::SourceSpan, extractor_or_model: None, version: None,
        input_hash: None, output_hash: None, metric_name: None, metric_value: None,
        dependency_origin: None,
    })
    .unwrap();
    prov.insert_review_decision(&NewReviewDecision {
        assertion_id, decision: ReviewOutcome::Accept,
        reviewer_id: None, // 承認者不明のまま
        authorization_level: None, scope: None, rationale: None, decided_at_unix: 0,
        dataset_version: None, expires_at_unix: None, supersedes_review_id: None,
    })
    .unwrap();

    let manifest = base_manifest(release.0);
    let report = verify_release(
        &prov,
        &VerifyInputs {
            manifest: &manifest,
            judgments_provenance: &JudgmentsProvenanceExport { release_tag: "t".into(), release_git_commit: None, dependencies: vec![], citations: vec![], morphisms: vec![] },
            relations_provenance: &empty_relations_sidecar(),
            live_input_files: &[],
            now_unix: 0,
        },
    )
    .unwrap();
    assert!(
        report.failures.iter().any(|f| f.check == "trusted_assertion_missing_qualifying_evidence"),
        "reviewer_idの無いacceptはauthenticatedと認めないべき: {:?}",
        report.failures
    );
}

/// 上2件の裏返し: 正式な証拠、または本人確認済みレビューがあれば
/// 既定トラバース対象は正当に通る——ゲートが正しい状態まで拒否しないことも確認する。
#[test]
fn accepts_default_traversal_assertions_with_qualifying_evidence() {
    use mathesis_provenance::model::{EntityKind, NewEntity, NewReviewDecision, ReviewOutcome};

    let prov = ProvenanceStore::open_in_memory().unwrap();
    let release = prov
        .get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None })
        .unwrap();
    // P6.1: このsource_recordはFormalExport証拠が指す側なので、
    // `verify_formal_evidence_has_reproducibility_metadata`が要求する
    // 6項目すべてを埋めた再現性メタデータが必要——さもなくば「正しい状態を
    // 誤って拒否しない」ことを確かめるこのテスト自体が壊れる。
    let reproducibility_json = serde_json::json!({
        "leanToolchain": "leanprover/lean4:v4.29.0-rc6",
        "mathlibRev": "abc123",
        "projectCommit": null,
        "extractorVersion": "mathesis-lean-extract-v2",
        "filteringPolicyVersion": mathesis_provenance::lean_manifest_adapter::FILTERING_POLICY_VERSION,
        "rawManifestHash": "sha256:aaaa",
        "normalizedManifestHash": "sha256:bbbb",
    })
    .to_string();
    let source = prov
        .get_or_insert_source_record(&NewSourceRecord {
            provider: "test".into(), provider_id: "t".into(), provider_revision: None, retrieved_at_unix: None,
            content_hash: None, licence: None, attribution: None, raw_payload_uri: None,
            adapter_name: "test".into(), adapter_version: "0".into(), parser_version: None,
            reproducibility_json: Some(reproducibility_json),
        })
        .unwrap();

    // 1件目: FormalExport証拠を持つobserved depends_on。
    let formal = prov
        .insert_assertion(&NewRelationAssertion {
            subject_ref: "judgment:1".into(), predicate: RelationKind::DependsOn, object_ref: "judgment:2".into(),
            epistemic_state: EpistemicState::Observed, score: None, policy_version: None, created_by_run_id: None,
            supersedes_id: None, release_id: release, legacy_ref: Some("d1".into()),
        })
        .unwrap();
    prov.insert_evidence(&mathesis_provenance::model::NewEvidence {
        assertion_id: formal, source_record_id: source, locator: Some("Mod.a -> Mod.b".into()),
        evidence_kind: EvidenceKind::FormalExport, extractor_or_model: None, version: None,
        input_hash: None, output_hash: None, metric_name: None, metric_value: None,
        dependency_origin: Some("body".into()),
    })
    .unwrap();

    // 2件目: reviewer_id付きのacceptを持つreviewedな意味的関係。
    prov.get_or_insert_entity(&NewEntity { kind: EntityKind::Concept, display_label: "a".into(), source_record_id: None }, "concept:a").unwrap();
    prov.get_or_insert_entity(&NewEntity { kind: EntityKind::Concept, display_label: "b".into(), source_record_id: None }, "concept:b").unwrap();
    let reviewed = prov
        .insert_assertion(&NewRelationAssertion {
            subject_ref: "concept:a".into(), predicate: RelationKind::Specializes, object_ref: "concept:b".into(),
            epistemic_state: EpistemicState::Reviewed, score: None, policy_version: None, created_by_run_id: None,
            supersedes_id: None, release_id: release, legacy_ref: Some("r1".into()),
        })
        .unwrap();
    prov.insert_evidence(&mathesis_provenance::model::NewEvidence {
        assertion_id: reviewed, source_record_id: source, locator: Some("a is a special case of b".into()),
        evidence_kind: EvidenceKind::SourceSpan, extractor_or_model: None, version: None,
        input_hash: None, output_hash: None, metric_name: None, metric_value: None,
        dependency_origin: None,
    })
    .unwrap();
    prov.insert_review_decision(&NewReviewDecision {
        assertion_id: reviewed, decision: ReviewOutcome::Accept, reviewer_id: Some("alice@example.invalid".into()),
        authorization_level: Some("maintainer".into()), scope: None, rationale: Some("checked by hand".into()),
        decided_at_unix: 0, dataset_version: Some("t".into()), expires_at_unix: None, supersedes_review_id: None,
    })
    .unwrap();

    let manifest = base_manifest(release.0);
    let report = verify_release(
        &prov,
        &VerifyInputs {
            manifest: &manifest,
            judgments_provenance: &base_judgments_sidecar(formal.0),
            relations_provenance: &empty_relations_sidecar(),
            live_input_files: &[],
            now_unix: 0,
        },
    )
    .unwrap();
    assert!(
        !report.failures.iter().any(|f| f.check == "trusted_assertion_missing_qualifying_evidence"),
        "正式な証拠/本人確認済みレビューがあるなら拒否されないべき: {:?}",
        report.failures
    );
    assert!(
        !report
            .failures
            .iter()
            .any(|f| f.check == "formal_evidence_missing_reproducibility_metadata" || f.check == "formal_evidence_filtering_policy_mismatch"),
        "P6.1: 完全な再現性メタデータと一致するフィルタポリシー版があるなら拒否されないべき: {:?}",
        report.failures
    );
}

/// P6.1（`docs/LEAN_DEPENDENCY_POLICY.md`）: `formal_export`証拠自体はあるが、
/// それを生んだSourceRecordに再現性メタデータが無い——`evidence_kind`だけ
/// 見る`verify_trusted_assertions_have_qualifying_evidence`はこれを見逃す
/// (evidence_kindはFormalExportのまま)。この一段深いチェックが検出する。
#[test]
fn rejects_a_default_traversal_assertion_whose_formal_evidence_has_no_reproducibility_metadata() {
    let prov = ProvenanceStore::open_in_memory().unwrap();
    let release = prov
        .get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None })
        .unwrap();
    let source = prov
        .get_or_insert_source_record(&NewSourceRecord {
            provider: "test".into(), provider_id: "t".into(), provider_revision: None, retrieved_at_unix: None,
            content_hash: None, licence: None, attribution: None, raw_payload_uri: None,
            adapter_name: "test".into(), adapter_version: "0".into(), parser_version: None,
            reproducibility_json: None, // 事故を想定: FormalExportなのに再現性メタデータが無い
        })
        .unwrap();
    let assertion_id = prov
        .insert_assertion(&NewRelationAssertion {
            subject_ref: "judgment:1".into(), predicate: RelationKind::DependsOn, object_ref: "judgment:2".into(),
            epistemic_state: EpistemicState::Observed, score: None, policy_version: None, created_by_run_id: None,
            supersedes_id: None, release_id: release, legacy_ref: Some("d1".into()),
        })
        .unwrap();
    prov.insert_evidence(&mathesis_provenance::model::NewEvidence {
        assertion_id, source_record_id: source, locator: Some("Mod.a -> Mod.b".into()),
        evidence_kind: EvidenceKind::FormalExport, extractor_or_model: None, version: None,
        input_hash: None, output_hash: None, metric_name: None, metric_value: None,
        dependency_origin: Some("body".into()),
    })
    .unwrap();

    let manifest = base_manifest(release.0);
    let report = verify_release(
        &prov,
        &VerifyInputs {
            manifest: &manifest,
            judgments_provenance: &base_judgments_sidecar(assertion_id.0),
            relations_provenance: &empty_relations_sidecar(),
            live_input_files: &[],
            now_unix: 0,
        },
    )
    .unwrap();
    assert!(
        report.failures.iter().any(|f| f.check == "formal_evidence_missing_reproducibility_metadata"),
        "reproducibility_jsonの無いformal_export証拠は拒否すべき: {:?}",
        report.failures
    );
}

/// P6.1: 再現性メタデータはあるが、`filteringPolicyVersion`が今のビルドの
/// ものと違う——フィルタの定義が変わったのに古いルールで取り込んだ証拠を
/// そのまま信頼してしまう事故を想定。
#[test]
fn rejects_a_default_traversal_assertion_whose_formal_evidence_used_a_different_filtering_policy_version() {
    let prov = ProvenanceStore::open_in_memory().unwrap();
    let release = prov
        .get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None })
        .unwrap();
    let stale_reproducibility_json = serde_json::json!({
        "leanToolchain": "leanprover/lean4:v4.29.0-rc6",
        "mathlibRev": "abc123",
        "projectCommit": null,
        "extractorVersion": "mathesis-lean-extract-v1",
        "filteringPolicyVersion": "mathesis-lean-dependency-filter-v0-stale",
        "rawManifestHash": "sha256:aaaa",
        "normalizedManifestHash": "sha256:bbbb",
    })
    .to_string();
    let source = prov
        .get_or_insert_source_record(&NewSourceRecord {
            provider: "test".into(), provider_id: "t".into(), provider_revision: None, retrieved_at_unix: None,
            content_hash: None, licence: None, attribution: None, raw_payload_uri: None,
            adapter_name: "test".into(), adapter_version: "0".into(), parser_version: None,
            reproducibility_json: Some(stale_reproducibility_json),
        })
        .unwrap();
    let assertion_id = prov
        .insert_assertion(&NewRelationAssertion {
            subject_ref: "judgment:1".into(), predicate: RelationKind::DependsOn, object_ref: "judgment:2".into(),
            epistemic_state: EpistemicState::Observed, score: None, policy_version: None, created_by_run_id: None,
            supersedes_id: None, release_id: release, legacy_ref: Some("d1".into()),
        })
        .unwrap();
    prov.insert_evidence(&mathesis_provenance::model::NewEvidence {
        assertion_id, source_record_id: source, locator: Some("Mod.a -> Mod.b".into()),
        evidence_kind: EvidenceKind::FormalExport, extractor_or_model: None, version: None,
        input_hash: None, output_hash: None, metric_name: None, metric_value: None,
        dependency_origin: Some("body".into()),
    })
    .unwrap();

    let manifest = base_manifest(release.0);
    let report = verify_release(
        &prov,
        &VerifyInputs {
            manifest: &manifest,
            judgments_provenance: &base_judgments_sidecar(assertion_id.0),
            relations_provenance: &empty_relations_sidecar(),
            live_input_files: &[],
            now_unix: 0,
        },
    )
    .unwrap();
    assert!(
        report.failures.iter().any(|f| f.check == "formal_evidence_filtering_policy_mismatch"),
        "古いfilteringPolicyVersionで取り込まれたformal_export証拠は拒否すべき: {:?}",
        report.failures
    );
}

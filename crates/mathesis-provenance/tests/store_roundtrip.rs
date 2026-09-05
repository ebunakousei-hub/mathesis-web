use mathesis_provenance::{
    EpistemicState, EvidenceKind, NewEvidence, NewRelationAssertion, NewRelease, NewReviewDecision,
    NewSourceRecord, ProvenanceStore, RelationKind, ReviewOutcome,
};

#[test]
fn release_is_interned_by_tag() {
    let store = ProvenanceStore::open_in_memory().unwrap();
    let a = store
        .get_or_insert_release(&NewRelease {
            tag: "v0-baseline-20260905".into(),
            git_commit: Some("325dba1".into()),
            generated_at_unix: 1000,
            notes: None,
        })
        .unwrap();
    let b = store
        .get_or_insert_release(&NewRelease {
            tag: "v0-baseline-20260905".into(),
            git_commit: Some("325dba1".into()),
            generated_at_unix: 1000,
            notes: None,
        })
        .unwrap();
    assert_eq!(a, b, "同じtagは同じReleaseIdに解決される");
    assert_eq!(store.get_release(a).unwrap().tag, "v0-baseline-20260905");
}

#[test]
fn source_record_is_interned_by_provider_and_provider_id() {
    let store = ProvenanceStore::open_in_memory().unwrap();
    let new = NewSourceRecord {
        provider: "arxiv".into(),
        provider_id: "2301.00001".into(),
        provider_revision: None,
        retrieved_at_unix: None,
        content_hash: None,
        licence: None,
        attribution: None,
        raw_payload_uri: None,
        adapter_name: "test".into(),
        adapter_version: "0".into(),
        parser_version: None,
    };
    let a = store.get_or_insert_source_record(&new).unwrap();
    let b = store.get_or_insert_source_record(&new).unwrap();
    assert_eq!(a, b);
    assert_eq!(store.source_record_count().unwrap(), 1);
}

#[test]
fn assertion_evidence_and_review_round_trip() {
    let store = ProvenanceStore::open_in_memory().unwrap();
    let release = store
        .get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None })
        .unwrap();
    let source = store
        .get_or_insert_source_record(&NewSourceRecord {
            provider: "arxiv".into(),
            provider_id: "math/0001".into(),
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

    let assertion = store
        .insert_assertion(&NewRelationAssertion {
            subject_ref: "concept:a".into(),
            predicate: RelationKind::Specializes,
            object_ref: "concept:b".into(),
            epistemic_state: EpistemicState::Extracted,
            score: None,
            policy_version: None,
            created_by_run_id: Some("test".into()),
            supersedes_id: None,
            release_id: release,
            legacy_ref: Some("concept_relation:a|b|specialization_of".into()),
        })
        .unwrap();

    // 冪等性: 同じ(release, legacy_ref)は既存を返す
    let existing = store
        .get_assertion_by_legacy_ref(release, "concept_relation:a|b|specialization_of")
        .unwrap();
    assert_eq!(existing, Some(assertion));

    store
        .insert_evidence(&NewEvidence {
            assertion_id: assertion,
            source_record_id: source,
            locator: Some("a is a special case of b.".into()),
            evidence_kind: EvidenceKind::SourceSpan,
            extractor_or_model: Some("hearst-pattern".into()),
            version: None,
            input_hash: None,
            output_hash: None,
            metric_name: None,
            metric_value: None,
        })
        .unwrap();
    assert_eq!(store.evidence_for(assertion).unwrap().len(), 1);

    store
        .insert_review_decision(&NewReviewDecision {
            assertion_id: assertion,
            decision: ReviewOutcome::Accept,
            reviewer_id: Some("reviewer-1".into()),
            scope: Some("t".into()),
            rationale: Some("looks right".into()),
            decided_at_unix: 42,
            dataset_version: None,
        })
        .unwrap();
    let decisions = store.review_decisions_for(assertion).unwrap();
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].decision, ReviewOutcome::Accept);
}

#[test]
fn relation_kind_and_epistemic_state_round_trip_through_str() {
    for kind in [
        RelationKind::DependsOn,
        RelationKind::Imports,
        RelationKind::Cites,
        RelationKind::Specializes,
        RelationKind::EquivalentTo,
        RelationKind::Generalizes,
        RelationKind::RelatedTo,
        RelationKind::UsesConcept,
        RelationKind::Implies,
    ] {
        assert_eq!(RelationKind::from_str(kind.as_str()), Some(kind));
    }
    for state in [
        EpistemicState::Observed,
        EpistemicState::Extracted,
        EpistemicState::Proposed,
        EpistemicState::Reviewed,
        EpistemicState::Verified,
        EpistemicState::Rejected,
    ] {
        assert_eq!(EpistemicState::from_str(state.as_str()), Some(state));
    }
}

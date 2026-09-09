//! P8.1（`docs/P8_1_STATUS.md`）CI validation: runs the whole offline
//! Math-Graph pilot pipeline — import_pilot -> ProvenanceStore ->
//! pilot_artifact::build_manifest — against small, synthetic,
//! committable fixtures (`tests/fixtures/pilot_sample_*.json`), not the
//! real 1GB+ dataset. Every declaration/edge in these fixtures is
//! invented (`Sample.*`/`SampleFLT.*`/`SampleMathlib`/
//! `SampleNonMathlibProject`) — nothing here is copied from the real
//! Math-Graph data, so this test carries no licensing question at all.

use mathesis_provenance::math_graph_adapter::{self, PilotEdge, PilotStatement};
use mathesis_provenance::pilot_artifact::{self, ProjectReport, ScopeReport, ScopeTotals};
use mathesis_provenance::{NewRelease, ProvenanceStore};

fn load_fixture_statements() -> Vec<PilotStatement> {
    let raw = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/pilot_sample_statements.json")).unwrap();
    serde_json::from_slice(&raw).unwrap()
}

fn load_fixture_edges() -> Vec<PilotEdge> {
    let raw = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/pilot_sample_edges.json")).unwrap();
    serde_json::from_slice(&raw).unwrap()
}

#[test]
fn the_whole_pilot_pipeline_runs_end_to_end_against_synthetic_fixtures() {
    let prov = ProvenanceStore::open_in_memory().unwrap();
    let release = prov
        .get_or_insert_release(&NewRelease {
            tag: "ci-fixture-release".into(),
            git_commit: None,
            generated_at_unix: 0,
            notes: None,
        })
        .unwrap();

    let statements = load_fixture_statements();
    let edges = load_fixture_edges();
    assert_eq!(statements.len(), 4, "fixture should have 4 synthetic declarations");
    assert_eq!(edges.len(), 3, "fixture should have 3 synthetic edges");

    let stats = math_graph_adapter::import_pilot(&prov, release, &statements, &edges, "fixture-revision-0000").unwrap();

    // 4 declarations, all pre-classified as safe (literal or
    // typeclass_hierarchy) — that's the contract this adapter has always
    // had: `excluded` declarations never reach it at all.
    assert_eq!(stats.declarations_imported, 4);
    // Of the 3 edges: 1 is proof-type (excluded), 1 has both endpoints in
    // scope (imported), 1 targets a dep_id outside the fixture's statement
    // set (outside pilot scope).
    assert_eq!(stats.dependencies_imported, 1, "only the sig-type, both-endpoints-in-scope edge should import");
    assert_eq!(stats.dependencies_excluded_proof_edge, 1);
    assert_eq!(stats.dependencies_outside_pilot_scope, 1);

    // Now build the manifest against the same isolated DB, with a small
    // hand-authored scope report matching this fixture's real shape.
    let scope_report = ScopeReport {
        projects: vec![
            ProjectReport {
                repo_slug: "SampleMathlib".into(),
                scope_note: "fixture project (literal + hierarchy)".into(),
                declarations_total: 3,
                declarations_literal: 1,
                declarations_typeclass_hierarchy: 2,
                declarations_excluded: 0,
                edges_imported: Some(1),
                duplicate_groups: Some(0),
                duplicate_extra_rows: Some(0),
                dependency_targets_outside_scope: Some(0),
                literal_match_method: "fixture".into(),
            },
            ProjectReport {
                repo_slug: "SampleNonMathlibProject".into(),
                scope_note: "fixture project (hierarchy only, no literal possible)".into(),
                declarations_total: 1,
                declarations_literal: 0,
                declarations_typeclass_hierarchy: 1,
                declarations_excluded: 0,
                edges_imported: Some(0),
                duplicate_groups: Some(0),
                duplicate_extra_rows: Some(0),
                dependency_targets_outside_scope: Some(1),
                literal_match_method: "NOT COMPUTABLE — fixture".into(),
            },
        ],
        totals: ScopeTotals {
            declarations: 4,
            declarations_literal: 1,
            declarations_typeclass_hierarchy: 3,
            declarations_excluded: 0,
            edges_imported: 1,
            duplicate_groups: 0,
            duplicate_extra_rows: 0,
            dependency_targets_outside_scope: 1,
        },
    };

    let manifest = pilot_artifact::build_manifest(
        &prov,
        "ci-fixture-release",
        release.0,
        "fixture-revision-0000",
        1_700_000_000,
        vec![],
        scope_report,
        vec![],
        1_700_000_100,
    )
    .unwrap();

    assert_eq!(manifest.declaration_entity_count_in_db, 4);
    assert_eq!(manifest.edge_assertion_count_in_db, 1);
    assert_eq!(
        manifest.msc_classification_counts.unavailable, 4,
        "must match declaration_entity_count_in_db, not scope_report.totals.declarations"
    );
    assert_eq!(manifest.license, "CC-BY-4.0");
    assert_eq!(manifest.dataset_revision, "fixture-revision-0000");
    assert!(manifest.scope_notes.iter().any(|n| n.contains("Isolated pilot artifact")));

    // Round-trip through JSON, matching how the real CLI writes/reads it.
    let json = serde_json::to_string_pretty(&manifest).unwrap();
    let reparsed: pilot_artifact::PilotArtifactManifest = serde_json::from_str(&json).unwrap();
    assert_eq!(reparsed.declaration_entity_count_in_db, 4);
}

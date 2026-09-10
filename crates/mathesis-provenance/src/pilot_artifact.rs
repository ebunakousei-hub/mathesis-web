//! P8.1（`docs/P8_1_STATUS.md`）: the offline TheoremGraph/Math-Graph
//! connector "artifact" — a self-describing release manifest for the
//! isolated pilot database, distinct from `ProvenanceManifest`
//! (production `import-legacy`) and `WebExportManifest` (the public
//! `dependencies.json`/etc.). This artifact is never meant to touch
//! production `scratch/provenance.db` or ship to `web/public/` — it
//! describes a standalone pilot DB (e.g. `scratch/p8_1/
//! pilot_provenance.db`) built by `import-math-graph` runs against an
//! isolated release.
//!
//! Field list follows the P8.1 spec verbatim: dataset URL/revision,
//! retrieval timestamp, file hashes, schema version, adapter/parser
//! version, license/attribution, selected project/table info, source
//! record counts, edge counts, unresolved endpoint counts, duplicate
//! counts, MSC classification-status counts, generated index hashes.

use crate::manifest::InputFileHash;
use crate::math_graph_adapter::{ADAPTER_NAME, ADAPTER_VERSION, MATH_GRAPH_ATTRIBUTION, MATH_GRAPH_LICENSE, MATH_GRAPH_SOURCE_URL};
use crate::store::ProvenanceStore;
use serde::{Deserialize, Serialize};

pub const PILOT_ARTIFACT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectReport {
    pub repo_slug: String,
    pub scope_note: String,
    pub declarations_total: i64,
    pub declarations_literal: i64,
    pub declarations_external_structural_candidate: i64,
    pub declarations_excluded: i64,
    /// P8.5（`docs/P8_5_STATUS.md`）: declarations whose `filePath` doesn't
    /// start with the project's own expected top-level directory —
    /// discovered when a full-scope directory breakdown (not just the 20
    /// spot-checked declarations) found that 92 of PrimeNumberTheoremAnd's
    /// 109 already-imported `external_structural_candidate` declarations
    /// actually live under `LeanCert/`/`PrimeCert/`/`Architect/`, and 9 of
    /// FLT's live under a bare `Mathlib/` that doesn't exist in FLT's own
    /// repo tree at all. `#[serde(default)]` so older scope reports
    /// (pre-P8.5, before this check existed) still deserialize — they
    /// simply report 0 here, which is honest (the check wasn't run, not
    /// that nothing was found).
    #[serde(default)]
    pub declarations_project_attribution_unresolved: i64,
    pub edges_imported: Option<i64>,
    pub duplicate_groups: Option<i64>,
    pub duplicate_extra_rows: Option<i64>,
    pub dependency_targets_outside_scope: Option<i64>,
    pub literal_match_method: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScopeTotals {
    pub declarations: i64,
    pub declarations_literal: i64,
    pub declarations_external_structural_candidate: i64,
    pub declarations_excluded: i64,
    #[serde(default)]
    pub declarations_project_attribution_unresolved: i64,
    pub edges_imported: i64,
    pub duplicate_groups: i64,
    pub duplicate_extra_rows: i64,
    pub dependency_targets_outside_scope: i64,
}

/// Deserialized straight from `scratch/math_graph_pilot/scope_report_p8_1.json`
/// (`build_scope_report_p8_1.py`'s output — a git-ignored scratch file,
/// same as every other Math-Graph pilot intermediate since P7).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeReport {
    pub projects: Vec<ProjectReport>,
    pub totals: ScopeTotals,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MscClassificationCounts {
    pub unavailable: i64,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PilotArtifactManifest {
    pub schema_version: u32,
    pub artifact_name: String,
    pub dataset_url: String,
    pub dataset_revision: String,
    pub retrieved_at_unix: i64,
    pub license: String,
    pub attribution: String,
    pub adapter_name: String,
    pub adapter_version: String,
    /// SHA-256 of the raw source CSVs this pilot was scoped from
    /// (`formal_dependency.csv`/`statement_formal.csv`) — verified in P7
    /// against HuggingFace's own reported LFS hashes, re-verified here.
    pub raw_source_file_hashes: Vec<InputFileHash>,
    pub projects: Vec<ProjectReport>,
    pub totals: ScopeTotals,
    /// From the isolated pilot DB itself (`ProvenanceStore::source_record_count`),
    /// not from the Python-side scope report — cross-checks that everything
    /// the report says was "safe" actually landed in the DB.
    pub source_record_count_in_db: i64,
    pub declaration_entity_count_in_db: i64,
    pub edge_assertion_count_in_db: i64,
    /// MSC alignment (`mathesis-taxonomy::alignment`) only ever runs on
    /// arXiv concept clusters — never on Lean declarations. Every record
    /// in this artifact is therefore honestly `unavailable`
    /// (`ClassificationStatus::Unavailable`, `docs/DATA_DICTIONARY.md`),
    /// not silently omitted from the manifest.
    pub msc_classification_counts: MscClassificationCounts,
    /// SHA-256 of this manifest's own sibling read-model export
    /// (`export-pilot-read-model`'s output), once written — populated by
    /// the caller after both files exist (see `run_export_pilot_manifest`).
    pub generated_index_hashes: Vec<InputFileHash>,
    pub generated_at_unix: i64,
    pub release_tag: String,
    pub release_id: i64,
    pub scope_notes: Vec<String>,
}

pub fn build_manifest(
    prov: &ProvenanceStore,
    release_tag: &str,
    release_id: i64,
    dataset_revision: &str,
    retrieved_at_unix: i64,
    raw_source_file_hashes: Vec<InputFileHash>,
    scope_report: ScopeReport,
    generated_index_hashes: Vec<InputFileHash>,
    generated_at_unix: i64,
) -> anyhow::Result<PilotArtifactManifest> {
    // MSC classification-status counts describe what's actually *in the
    // artifact* (declarations that survived scoping and were imported),
    // not `scope_report.totals.declarations` (the full scanned/scoped
    // universe, most of which — the `excluded` bucket — was deliberately
    // never imported). Using the latter here was a real bug caught while
    // verifying this manifest against real data: it silently overstated
    // the count by including declarations this artifact doesn't contain.
    let declaration_entity_count_in_db = prov.entity_count()?;
    Ok(PilotArtifactManifest {
        schema_version: PILOT_ARTIFACT_SCHEMA_VERSION,
        artifact_name: "math-graph-pilot".to_string(),
        dataset_url: MATH_GRAPH_SOURCE_URL.to_string(),
        dataset_revision: dataset_revision.to_string(),
        retrieved_at_unix,
        license: MATH_GRAPH_LICENSE.to_string(),
        attribution: MATH_GRAPH_ATTRIBUTION.to_string(),
        adapter_name: ADAPTER_NAME.to_string(),
        adapter_version: ADAPTER_VERSION.to_string(),
        raw_source_file_hashes,
        projects: scope_report.projects,
        totals: scope_report.totals,
        source_record_count_in_db: prov.source_record_count()?,
        declaration_entity_count_in_db,
        edge_assertion_count_in_db: prov.assertion_count()?,
        msc_classification_counts: MscClassificationCounts {
            unavailable: declaration_entity_count_in_db,
            note: "MSC alignment (mathesis-taxonomy::alignment) only ever runs on arXiv concept \
                   clusters, never on Lean declarations — every record actually imported into this \
                   artifact is honestly `unavailable`, not evaluated and found non-matching. This \
                   count is the artifact's own declaration_entity_count_in_db, not the larger \
                   scanned/scoped total (which includes declarations excluded before import)."
                .to_string(),
        },
        generated_index_hashes,
        generated_at_unix,
        release_tag: release_tag.to_string(),
        release_id,
        scope_notes: vec![
            "Isolated pilot artifact — never written into production scratch/provenance.db.".to_string(),
            "Graph-structure-only: no theorem/proof/context text imported, matching every prior \
             Math-Graph pilot pass (P7-P7.4)."
                .to_string(),
            "For the 4 non-Mathlib projects (PrimeNumberTheoremAnd, FLT, carleson, pfr), the `literal` \
             classification is structurally unavailable — Mathesis has never independently \
             extracted those projects, so there is nothing to cross-reference against."
                .to_string(),
            "P8.5 (docs/P8_5_STATUS.md): `external_structural_candidate` (renamed from \
             `external_typeclass_hierarchy`) means only 'zero Math-Graph-recorded proof-type \
             outgoing edges' — a real spot-check against live Lean source found declarations \
             carrying this classification with substantive tactic proofs (FLT's \
             InverseLimit.instGroup, pfr's IsMarkovKernel-deleteRight instance). It does not mean \
             'no proof exists' or 'confirmed typeclass-hierarchy position' — see \
             docs/DATA_DICTIONARY.md's four-concepts table."
                .to_string(),
            "P8.5: every declaration whose filePath falls outside its project's own expected \
             top-level directory is excluded as `project_attribution_unresolved`, not silently \
             imported — found via full per-project path auditing, not just a sample \
             (PrimeNumberTheoremAnd's dataset attribution included substantial unrelated tooling \
             content under LeanCert/PrimeCert/Architect; see docs/P8_5_STATUS.md)."
                .to_string(),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_scope_report() -> ScopeReport {
        ScopeReport {
            projects: vec![ProjectReport {
                repo_slug: "Mathlib_v429".into(),
                scope_note: "2 target namespaces".into(),
                declarations_total: 62,
                declarations_literal: 20,
                declarations_external_structural_candidate: 42,
                declarations_excluded: 1,
                declarations_project_attribution_unresolved: 0,
                edges_imported: Some(48),
                duplicate_groups: Some(0),
                duplicate_extra_rows: Some(0),
                dependency_targets_outside_scope: Some(84),
                literal_match_method: "cross-referenced".into(),
            }],
            totals: ScopeTotals {
                declarations: 62,
                declarations_literal: 20,
                declarations_external_structural_candidate: 42,
                declarations_excluded: 1,
                declarations_project_attribution_unresolved: 0,
                edges_imported: 48,
                duplicate_groups: 0,
                duplicate_extra_rows: 0,
                dependency_targets_outside_scope: 84,
            },
        }
    }

    #[test]
    fn msc_classification_count_reflects_what_is_actually_in_the_db_not_the_larger_scoped_total() {
        // Real bug caught while verifying against production data: this
        // count must be the artifact's own declaration_entity_count_in_db
        // (what actually got imported), not scope_report.totals.declarations
        // (which also includes the `excluded` bucket that was never
        // imported). Populate 3 real entities, matching a scope report
        // that claims a much larger total (62) — the two must diverge.
        let prov = ProvenanceStore::open_in_memory().unwrap();
        for i in 0..3 {
            prov.get_or_insert_entity(
                &crate::model::NewEntity {
                    kind: crate::model::EntityKind::Judgment,
                    display_label: format!("decl {i}"),
                    source_record_id: None,
                },
                &format!("judgment:mathgraph:test-{i}"),
            )
            .unwrap();
        }
        let manifest = build_manifest(
            &prov,
            "test-release",
            1,
            "abc123",
            1700000000,
            vec![],
            sample_scope_report(),
            vec![],
            1700000100,
        )
        .unwrap();
        assert_eq!(manifest.declaration_entity_count_in_db, 3);
        assert_eq!(
            manifest.msc_classification_counts.unavailable, 3,
            "must match what's actually in the DB (3), not the scope report's larger total (62)"
        );
        assert!(!manifest.msc_classification_counts.note.is_empty());
    }

    #[test]
    fn manifest_carries_real_license_and_attribution_not_placeholders() {
        let prov = ProvenanceStore::open_in_memory().unwrap();
        let manifest =
            build_manifest(&prov, "t", 1, "rev", 0, vec![], sample_scope_report(), vec![], 0).unwrap();
        assert_eq!(manifest.license, "CC-BY-4.0");
        assert!(manifest.attribution.contains("uw-math-ai"));
    }

    #[test]
    fn scope_notes_flag_the_non_mathlib_literal_limitation() {
        let prov = ProvenanceStore::open_in_memory().unwrap();
        let manifest =
            build_manifest(&prov, "t", 1, "rev", 0, vec![], sample_scope_report(), vec![], 0).unwrap();
        assert!(manifest.scope_notes.iter().any(|n| n.contains("structurally unavailable")));
    }
}

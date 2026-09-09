use crate::model::{
    EntityKind, EpistemicState, EvidenceKind, NewEntity, NewEvidence,
    NewRelationAssertion, NewSourceRecord, RelationKind, ReleaseId,
};
use crate::relation_policy::SOURCE_MAPPING_POLICY_VERSION;
use crate::source_adapter::stable_record_key;
use crate::store::ProvenanceStore;
use sha2::{Digest, Sha256};

pub const ADAPTER_NAME: &str = "mathesis-provenance-msc2020-adapter";
pub const ADAPTER_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const MSC_LICENSE: &str = "CC-BY-NC-SA-4.0";
pub const MSC_ATTRIBUTION: &str =
    "Mathematical Reviews and zbMATH, Mathematics Subject Classification 2020";
pub const MSC_URL: &str = "https://msc2020.org/";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ImportStats {
    pub concepts_imported: usize,
    pub concepts_skipped: usize,
    pub relations_imported: usize,
    pub relations_skipped: usize,
}

pub(crate) fn snapshot_hash() -> String {
    let mut hasher = Sha256::new();
    hasher.update(mathesis_msc::official_csv().as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("sha256:{hex}")
}

/// Imports the official bundled MSC2020 snapshot as a controlled vocabulary.
/// A parent/child edge is taxonomy structure, not a mathematical implication.
/// It is therefore represented as `specializes`, `extracted`, and visible-only
/// until a separate publication/traversal policy accepts it.
pub fn import(prov: &ProvenanceStore, release: ReleaseId) -> anyhow::Result<ImportStats> {
    let revision = format!("bundled-{}", snapshot_hash());
    let source_id = prov.get_or_insert_source_record(&NewSourceRecord {
        provider: "msc2020".into(),
        provider_id: "MSC_2020.csv".into(),
        provider_revision: Some(revision.clone()),
        retrieved_at_unix: None,
        content_hash: Some(snapshot_hash()),
        licence: Some(MSC_LICENSE.into()),
        attribution: Some(MSC_ATTRIBUTION.into()),
        raw_payload_uri: Some(MSC_URL.into()),
        adapter_name: ADAPTER_NAME.into(),
        adapter_version: ADAPTER_VERSION.into(),
        parser_version: Some("mathesis-msc::official-csv".into()),
        reproducibility_json: None,
    })?;

    let mut stats = ImportStats::default();
    for code in mathesis_msc::all() {
        let reference = format!("concept:msc2020:{}", code.code);
        let (entity_id, is_new) = prov.get_or_insert_entity(
            &NewEntity {
                kind: EntityKind::Concept,
                display_label: code.name.clone(),
                source_record_id: Some(source_id),
            },
            &reference,
        )?;
        prov.set_label_origin(entity_id, crate::model::LabelOrigin::SourceProvided)?;
        if is_new {
            stats.concepts_imported += 1;
        } else {
            stats.concepts_skipped += 1;
        }

        let Some(parent) = &code.parent else { continue };
        let parent_ref = format!("concept:msc2020:{parent}");
        let legacy_ref = format!("msc2020:{}", stable_record_key("msc2020", &code.code, parent));
        if prov.get_assertion_by_legacy_ref(release, &legacy_ref)?.is_some() {
            stats.relations_skipped += 1;
            continue;
        }
        let assertion_id = prov.insert_assertion(&NewRelationAssertion {
            subject_ref: reference,
            predicate: RelationKind::Specializes,
            object_ref: parent_ref,
            epistemic_state: EpistemicState::Extracted,
            score: None,
            policy_version: Some(SOURCE_MAPPING_POLICY_VERSION.into()),
            created_by_run_id: Some(ADAPTER_NAME.into()),
            supersedes_id: None,
            release_id: release,
            legacy_ref: Some(legacy_ref),
        })?;
        prov.insert_evidence(&NewEvidence {
            assertion_id,
            source_record_id: source_id,
            locator: Some(code.code.clone()),
            evidence_kind: EvidenceKind::SourceSpan,
            extractor_or_model: Some("mathesis-msc::hierarchy".into()),
            version: Some(revision.clone()),
            input_hash: Some(snapshot_hash()),
            output_hash: None,
            metric_name: None,
            metric_value: None,
            dependency_origin: None,
            external_classification: None,
        })?;
        stats.relations_imported += 1;
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::NewRelease;
    use crate::ProvenanceStore;

    #[test]
    fn imports_real_bundled_msc_snapshot_idempotently() {
        let prov = ProvenanceStore::open_in_memory().unwrap();
        let release = prov.get_or_insert_release(&NewRelease {
            tag: "msc-fixture".into(),
            git_commit: None,
            generated_at_unix: 0,
            notes: None,
        }).unwrap();
        let first = prov.transaction(|| import(&prov, release)).unwrap();
        let second = prov.transaction(|| import(&prov, release)).unwrap();
        assert_eq!(first.concepts_imported, 6603);
        assert_eq!(second.concepts_imported, 0);
        assert_eq!(first.relations_imported, 6540);
        assert_eq!(second.relations_imported, 0);
        assert_eq!(prov.entity_count().unwrap(), 6603);
    }
}

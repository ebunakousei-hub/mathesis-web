use crate::model::{EntityKind, RelationKind};

/// Stable contract every future external-source adapter must satisfy before
/// it can write into the evidence core.
pub trait SourceAdapter {
    fn source_name(&self) -> &'static str;
    fn source_version(&self) -> &str;
    fn mapping_policy_version(&self) -> &'static str;
    fn licensing(&self) -> &SourceLicense;
    fn records(&self) -> &[SourceFixtureRecord];
    fn assertions(&self) -> &[SourceFixtureAssertion];
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLicense {
    pub license: String,
    pub attribution: String,
    pub source_url: String,
    pub redistribution_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFixtureRecord {
    pub provider_id: String,
    pub revision: String,
    pub content_hash: String,
    pub entity_kind: EntityKind,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFixtureAssertion {
    pub source_record_id: String,
    pub subject_kind: EntityKind,
    pub object_kind: EntityKind,
    pub predicate: RelationKind,
}

pub fn validate_adapter<A: SourceAdapter>(adapter: &A) -> Result<(), String> {
    if adapter.mapping_policy_version() != crate::relation_policy::SOURCE_MAPPING_POLICY_VERSION {
        return Err(format!(
            "{} uses unsupported mapping policy '{}'",
            adapter.source_name(),
            adapter.mapping_policy_version()
        ));
    }
    let license = adapter.licensing();
    if license.license.trim().is_empty()
        || license.attribution.trim().is_empty()
        || license.source_url.trim().is_empty()
    {
        return Err(format!("{} has incomplete licensing metadata", adapter.source_name()));
    }
    if !license.redistribution_allowed {
        return Err(format!("{} does not permit redistribution", adapter.source_name()));
    }
    let ids: std::collections::HashSet<_> = adapter.records().iter().map(|r| &r.provider_id).collect();
    if adapter.records().iter().any(|r| {
        r.provider_id.trim().is_empty()
            || r.revision.trim().is_empty()
            || r.content_hash.trim().is_empty()
    }) {
        return Err(format!("{} contains a record without a stable identity or content hash", adapter.source_name()));
    }
    for assertion in adapter.assertions() {
        if !ids.contains(&assertion.source_record_id) {
            return Err(format!(
                "{} assertion references unknown source record '{}'",
                adapter.source_name(),
                assertion.source_record_id
            ));
        }

        if !crate::relation_policy::valid_entity_kinds(
            assertion.predicate,
            assertion.subject_kind,
            assertion.object_kind,
        ) {
            return Err(format!(
                "{} assertion has invalid {:?} -> {:?} pair for {}",
                adapter.source_name(),
                assertion.subject_kind,
                assertion.object_kind,
                assertion.predicate.as_str()
            ));
        }
    }
    Ok(())
}

/// The adapter identity key is deliberately independent of insertion order,
/// making reruns idempotent and allowing duplicate detection before import.
pub fn stable_record_key(source_name: &str, provider_id: &str, revision: &str) -> String {
    format!("{source_name}:{provider_id}:{revision}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relation_policy::SOURCE_MAPPING_POLICY_VERSION;

    struct FixtureAdapter {
        license: SourceLicense,
        records: Vec<SourceFixtureRecord>,
        assertions: Vec<SourceFixtureAssertion>,
    }

    impl SourceAdapter for FixtureAdapter {
        fn source_name(&self) -> &'static str { "synthetic" }
        fn source_version(&self) -> &str { "fixture-v1" }
        fn mapping_policy_version(&self) -> &'static str { SOURCE_MAPPING_POLICY_VERSION }
        fn licensing(&self) -> &SourceLicense { &self.license }
        fn records(&self) -> &[SourceFixtureRecord] { &self.records }
        fn assertions(&self) -> &[SourceFixtureAssertion] { &self.assertions }
    }

    fn valid() -> FixtureAdapter {
        FixtureAdapter {
            license: SourceLicense {
                license: "CC-BY-4.0".into(),
                attribution: "Synthetic source".into(),
                source_url: "https://example.invalid/source".into(),
                redistribution_allowed: true,
            },
            records: vec![SourceFixtureRecord {
                provider_id: "paper-1".into(),
                revision: "1".into(),
                content_hash: "sha256:abc".into(),
                entity_kind: EntityKind::Paper,
                label: "Paper".into(),
            }],
            assertions: vec![SourceFixtureAssertion {
                source_record_id: "paper-1".into(),
                subject_kind: EntityKind::Paper,
                object_kind: EntityKind::Paper,
                predicate: RelationKind::Cites,
            }],
        }
    }

    #[test]
    fn validates_synthetic_adapter_contract() {
        assert!(validate_adapter(&valid()).is_ok());
    }

    #[test]
    fn rejects_missing_license_and_unknown_record() {
        let mut adapter = valid();
        adapter.license.attribution.clear();
        assert!(validate_adapter(&adapter).unwrap_err().contains("licensing"));
        let mut adapter = valid();
        adapter.assertions[0].source_record_id = "missing".into();
        assert!(validate_adapter(&adapter).unwrap_err().contains("unknown source record"));
    }

    #[test]
    fn rejects_invalid_relation_pair() {
        let mut adapter = valid();
        adapter.assertions[0].subject_kind = EntityKind::Concept;
        assert!(validate_adapter(&adapter).unwrap_err().contains("invalid"));
    }

    #[test]
    fn stable_key_is_repeatable() {
        assert_eq!(
            stable_record_key("synthetic", "paper-1", "1"),
            stable_record_key("synthetic", "paper-1", "1")
        );
    }
}

use crate::model::SourceRecord;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LicensingFailure {
    pub source_record_id: i64,
    pub reason: &'static str,
}

/// A publishable external source must identify its license and attribution.
/// Legacy records are allowed to remain incomplete until a source adapter
/// supplies the missing metadata; they are reported, never silently treated
/// as redistributable.
pub fn validate_source_record(record: &SourceRecord) -> Option<LicensingFailure> {
    if record.licence.as_deref().unwrap_or("").trim().is_empty() {
        return Some(LicensingFailure { source_record_id: record.id.0, reason: "missing_license" });
    }
    if record.attribution.as_deref().unwrap_or("").trim().is_empty() {
        return Some(LicensingFailure { source_record_id: record.id.0, reason: "missing_attribution" });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{SourceRecord, SourceRecordId};

    fn record(licence: Option<&str>, attribution: Option<&str>) -> SourceRecord {
        SourceRecord {
            id: SourceRecordId(1),
            provider: "fixture".into(),
            provider_id: "1".into(),
            provider_revision: Some("1".into()),
            retrieved_at_unix: None,
            content_hash: Some("sha256:x".into()),
            licence: licence.map(str::to_string),
            attribution: attribution.map(str::to_string),
            raw_payload_uri: Some("https://example.invalid/1".into()),
            adapter_name: "fixture".into(),
            adapter_version: "1".into(),
            parser_version: None,
            reproducibility_json: None,
        }
    }

    #[test]
    fn licensing_contract_requires_license_and_attribution() {
        assert_eq!(validate_source_record(&record(None, Some("A"))).unwrap().reason, "missing_license");
        assert_eq!(validate_source_record(&record(Some("CC-BY-4.0"), None)).unwrap().reason, "missing_attribution");
        assert!(validate_source_record(&record(Some("CC-BY-4.0"), Some("A"))).is_none());
    }
}

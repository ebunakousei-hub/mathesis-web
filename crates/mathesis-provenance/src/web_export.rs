//! P2, Increment 1 (`docs/P2_STATUS.md`): the running Web app's edge-level
//! read model — judgment dependencies, morphisms (implies/specializes/
//! generalizes/equivalent_to between judgments), and typed concept
//! relations — is generated directly from this crate's `ProvenanceStore`
//! (`RelationAssertion` + `Evidence` + `ReviewDecision`), not from
//! `mathesis-graph`/`mathesis-taxonomy`'s own exporters plus a thin
//! "provenance sidecar" the client had to join by hand. Those two crates'
//! own `--export`/`export` commands still produce `judgments.json` (judgment
//! statements, papers — node data outside this crate's scope) and
//! `taxonomy.json` (concepts/clusters/search index), but no longer own the
//! *edges* the app displays.
//!
//! Enumeration reads `list_assertions_for_release` alone — nothing here opens
//! the original `mathesis-graph`/`mathesis-taxonomy` SQLite databases.
//! `subject_ref`/`object_ref`'s `"kind:id"` tag (`docs/DATA_DICTIONARY.md`
//! design decision 2) says which domain (dependency/morphism/relation) an
//! assertion belongs to. Fields the legacy exporters used to read straight
//! off `mathesis-graph`'s `morphisms` table or `mathesis-taxonomy`'s
//! `RelationStatus` (`kind`/`origin`/`status`/`rationale`/`confidence`) are
//! instead reconstructed from the assertion's own `Evidence`/
//! `ReviewDecision` rows, so the web read model can never disagree with what
//! `mathesis-provenance verify` has already checked.

use crate::assertion_export::{evidence_details_for, review_decision_details_for};
use crate::model::{EpistemicState, RelationAssertion, RelationKind};
use crate::store::ProvenanceStore;
use serde::Serialize;

fn strip_prefix_id(prefix: &str, r: &str) -> Option<i64> {
    r.strip_prefix(prefix)?.parse().ok()
}

fn strip_prefix_str<'a>(prefix: &str, r: &'a str) -> Option<&'a str> {
    r.strip_prefix(prefix)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyEdge {
    pub assertion_id: i64,
    pub from: i64,
    pub to: i64,
}

/// `mathesis-graph::export::ExportedMorphism`と同じ4フィールド
/// （`kind`/`origin`/`status`/`rationale`）を、`morphisms`テーブルからでは
/// なく`RelationAssertion`+`Evidence`+`ReviewDecision`から再構成する。
/// `id`は旧来の`morphisms.id`ではなくこのassertionのid——1射につき
/// assertionが必ず1件（`legacy_adapter::import_graph`）なので識別子として
/// 完全に代用でき、`web/src/lineage.ts`が別途持っていた
/// `morphismProvenance`という2つ目のidマップが丸ごと要らなくなる。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MorphismEdge {
    pub id: i64,
    pub src: i64,
    pub dst: i64,
    pub kind: String,
    pub origin: String,
    pub status: String,
    pub rationale: Option<String>,
}

/// `mathesis-taxonomy::export::RelationsExport`の1行相当。`confidence`は
/// Confirmedにしか存在しない実測値——Groundedは`None`（旧
/// `taxonomy.relations.json`が出していた固定1.0のプレースホルダは、ここでは
/// 出さない。`relations.rs::merge()`のGrounded確定時に代入されていた値で
/// あって測定値ではなかった、という外部レビュー2026-09-05の指摘そのもの）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationEdge {
    pub assertion_id: i64,
    pub subject: String,
    pub object: String,
    pub kind: String,
    pub status: String,
    pub confidence: Option<f64>,
    pub evidence_sentence: String,
    pub evidence_arxiv_id: String,
}

fn morphism_kind_str(k: RelationKind) -> Option<&'static str> {
    Some(match k {
        RelationKind::Implies => "implication",
        RelationKind::Specializes => "specialization",
        RelationKind::Generalizes => "generalization",
        RelationKind::EquivalentTo => "equivalence",
        _ => return None,
    })
}

fn relation_kind_str(k: RelationKind) -> Option<&'static str> {
    Some(match k {
        RelationKind::Specializes => "specialization_of",
        RelationKind::EquivalentTo => "equivalent_to",
        _ => return None,
    })
}

/// `judgment_dependencies` -> depends_on。構造的な機械的事実そのままで、
/// 状態は常に`extracted`（`legacy_adapter::import_graph`参照）——ここに
/// Evidence由来の追加解釈は要らない。
pub fn build_dependency_edges(assertions: &[RelationAssertion]) -> Vec<DependencyEdge> {
    assertions
        .iter()
        .filter(|a| a.predicate == RelationKind::DependsOn)
        .filter_map(|a| {
            let from = strip_prefix_id("judgment:", &a.subject_ref)?;
            let to = strip_prefix_id("judgment:", &a.object_ref)?;
            Some(DependencyEdge { assertion_id: a.id.0, from, to })
        })
        .collect()
}

/// 射（implies/specializes/generalizes/equivalent_to、judgment同士）。
/// `origin`は起源となったEvidence行の`evidence_kind`
/// （`reviewer_note`=manual、`model_output`=heuristic）から、`status`は
/// `epistemic_state`と`ReviewDecision`の有無から復元する
/// （`docs/DATA_DICTIONARY.md`「Resolved decisions #4」——legacy
/// `Accepted`は`epistemic_state: proposed`のまま、`ReviewDecision{accept}`
/// が別途あるかどうかで見分ける）。
pub fn build_morphism_edges(prov: &ProvenanceStore, assertions: &[RelationAssertion]) -> anyhow::Result<Vec<MorphismEdge>> {
    let mut out = Vec::new();
    for a in assertions {
        let Some(kind) = morphism_kind_str(a.predicate) else { continue };
        let (Some(src), Some(dst)) =
            (strip_prefix_id("judgment:", &a.subject_ref), strip_prefix_id("judgment:", &a.object_ref))
        else {
            continue;
        };
        let evidence = evidence_details_for(prov, a.id)?;
        let review_decisions = review_decision_details_for(prov, a.id)?;
        let origin_evidence =
            evidence.iter().find(|e| e.evidence_kind == "reviewer_note" || e.evidence_kind == "model_output");
        let origin = match origin_evidence.map(|e| e.evidence_kind.as_str()) {
            Some("reviewer_note") => "manual",
            _ => "heuristic",
        };
        let rationale = origin_evidence.and_then(|e| e.locator.clone());
        let status = if a.epistemic_state == EpistemicState::Rejected {
            "rejected"
        } else if review_decisions.iter().any(|r| r.decision == "accept") {
            "accepted"
        } else {
            "proposed"
        };
        out.push(MorphismEdge {
            id: a.id.0,
            src,
            dst,
            kind: kind.to_string(),
            origin: origin.to_string(),
            status: status.to_string(),
            rationale,
        });
    }
    Ok(out)
}

/// 型付き概念関係（specialization_of/equivalent_to）。根拠文
/// （`source_span`のEvidence）を持たないassertion（distributionalのみの
/// Proposed）は出さない——`mathesis-taxonomy::export::RelationsExport`と
/// 同じ「読者が自分の目で確かめられる根拠がある行だけを見せる」方針
/// （実測精度約50%、`relations.rs`冒頭コメント参照）。
pub fn build_relation_edges(prov: &ProvenanceStore, assertions: &[RelationAssertion]) -> anyhow::Result<Vec<RelationEdge>> {
    let mut out = Vec::new();
    for a in assertions {
        let Some(kind) = relation_kind_str(a.predicate) else { continue };
        let Some(subject) = strip_prefix_str("concept:", &a.subject_ref) else { continue };
        let Some(object) = strip_prefix_str("concept:", &a.object_ref) else { continue };
        let evidence = evidence_details_for(prov, a.id)?;
        let Some(source_span) = evidence.iter().find(|e| e.evidence_kind == "source_span") else { continue };
        let model_output = evidence.iter().find(|e| e.evidence_kind == "model_output");
        out.push(RelationEdge {
            assertion_id: a.id.0,
            subject: subject.to_string(),
            object: object.to_string(),
            kind: kind.to_string(),
            status: if model_output.is_some() { "confirmed".to_string() } else { "grounded".to_string() },
            confidence: model_output.and_then(|e| e.metric_value),
            evidence_sentence: source_span.locator.clone().unwrap_or_default(),
            evidence_arxiv_id: if source_span.source_provider == "arxiv" {
                source_span.source_provider_id.clone()
            } else {
                String::new()
            },
        });
    }
    // Confirmedを先に、同じstatus内は確信度の降順——旧`RelationsExport`と
    // 同じ表示順（利用者が最初に見るものが最も裏付けの強いものになるように）。
    out.sort_by(|x, y| {
        let rank = |s: &str| if s == "confirmed" { 0 } else { 1 };
        rank(&x.status)
            .cmp(&rank(&y.status))
            .then_with(|| y.confidence.partial_cmp(&x.confidence).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| x.subject.cmp(&y.subject))
    });
    Ok(out)
}

#[derive(Debug, Default)]
pub struct WebExport {
    pub dependencies: Vec<DependencyEdge>,
    pub morphisms: Vec<MorphismEdge>,
    pub relations: Vec<RelationEdge>,
}

/// このリリースの`ProvenanceStore`**だけ**を入口に、Web版が今表示している
/// 3種の辺すべてを組み立てる。
pub fn build_web_export(prov: &ProvenanceStore, release: crate::model::ReleaseId) -> anyhow::Result<WebExport> {
    let assertions = prov.list_assertions_for_release(release)?;
    Ok(WebExport {
        dependencies: build_dependency_edges(&assertions),
        morphisms: build_morphism_edges(prov, &assertions)?,
        relations: build_relation_edges(prov, &assertions)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        EvidenceKind, NewEvidence, NewRelationAssertion, NewRelease, NewReviewDecision, NewSourceRecord, ReviewOutcome,
    };
    use crate::store::ProvenanceStore;

    fn setup() -> (ProvenanceStore, crate::model::ReleaseId, crate::model::SourceRecordId) {
        let prov = ProvenanceStore::open_in_memory().unwrap();
        let release = prov
            .get_or_insert_release(&NewRelease {
                tag: "test".into(),
                git_commit: None,
                generated_at_unix: 0,
                notes: None,
            })
            .unwrap();
        let source = prov
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
        (prov, release, source)
    }

    #[test]
    fn dependency_edges_reconstruct_from_to_from_refs() {
        let (prov, release, source) = setup();
        let a = prov
            .insert_assertion(&NewRelationAssertion {
                subject_ref: "judgment:5".into(),
                predicate: RelationKind::DependsOn,
                object_ref: "judgment:2".into(),
                epistemic_state: EpistemicState::Extracted,
                score: None,
                policy_version: None,
                created_by_run_id: None,
                supersedes_id: None,
                release_id: release,
                legacy_ref: Some("judgment_dependency:5:2".into()),
            })
            .unwrap();
        prov.insert_evidence(&NewEvidence {
            assertion_id: a,
            source_record_id: source,
            locator: Some("foo.lean:1".into()),
            evidence_kind: EvidenceKind::SourceSpan,
            extractor_or_model: None,
            version: None,
            input_hash: None,
            output_hash: None,
            metric_name: None,
            metric_value: None,
        })
        .unwrap();

        let assertions = prov.list_assertions_for_release(release).unwrap();
        let deps = build_dependency_edges(&assertions);
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].from, 5);
        assert_eq!(deps[0].to, 2);
        assert_eq!(deps[0].assertion_id, a.0);
    }

    fn insert_morphism(
        prov: &ProvenanceStore,
        release: crate::model::ReleaseId,
        source: crate::model::SourceRecordId,
        predicate: RelationKind,
        epistemic_state: EpistemicState,
        evidence_kind: EvidenceKind,
        rationale: Option<&str>,
        accepted: bool,
    ) -> i64 {
        let a = prov
            .insert_assertion(&NewRelationAssertion {
                subject_ref: "judgment:1".into(),
                predicate,
                object_ref: "judgment:2".into(),
                epistemic_state,
                score: None,
                policy_version: None,
                created_by_run_id: None,
                supersedes_id: None,
                release_id: release,
                legacy_ref: Some(format!("morphism:{}", predicate.as_str())),
            })
            .unwrap();
        prov.insert_evidence(&NewEvidence {
            assertion_id: a,
            source_record_id: source,
            locator: rationale.map(str::to_string),
            evidence_kind,
            extractor_or_model: None,
            version: None,
            input_hash: None,
            output_hash: None,
            metric_name: None,
            metric_value: None,
        })
        .unwrap();
        if accepted {
            prov.insert_review_decision(&NewReviewDecision {
                assertion_id: a,
                decision: ReviewOutcome::Accept,
                reviewer_id: None,
                scope: None,
                rationale: rationale.map(str::to_string),
                decided_at_unix: 0,
                dataset_version: None,
            })
            .unwrap();
        }
        a.0
    }

    #[test]
    fn morphism_status_distinguishes_proposed_accepted_rejected_from_evidence_and_review() {
        let (prov, release, source) = setup();
        insert_morphism(&prov, release, source, RelationKind::Specializes, EpistemicState::Proposed, EvidenceKind::ModelOutput, None, false);
        insert_morphism(
            &prov,
            release,
            source,
            RelationKind::Implies,
            EpistemicState::Proposed,
            EvidenceKind::ReviewerNote,
            Some("human said so"),
            true,
        );
        insert_morphism(&prov, release, source, RelationKind::Generalizes, EpistemicState::Rejected, EvidenceKind::ModelOutput, None, false);

        let assertions = prov.list_assertions_for_release(release).unwrap();
        let morphisms = build_morphism_edges(&prov, &assertions).unwrap();
        assert_eq!(morphisms.len(), 3);

        let by_kind = |k: &str| morphisms.iter().find(|m| m.kind == k).unwrap();
        let heuristic_proposed = by_kind("specialization");
        assert_eq!(heuristic_proposed.status, "proposed");
        assert_eq!(heuristic_proposed.origin, "heuristic");

        let manual_accepted = by_kind("implication");
        assert_eq!(manual_accepted.status, "accepted");
        assert_eq!(manual_accepted.origin, "manual");
        assert_eq!(manual_accepted.rationale.as_deref(), Some("human said so"));

        let rejected = by_kind("generalization");
        assert_eq!(rejected.status, "rejected");
    }

    fn insert_relation(
        prov: &ProvenanceStore,
        release: crate::model::ReleaseId,
        source: crate::model::SourceRecordId,
        subject: &str,
        sentence: Option<&str>,
        metric_value: Option<f64>,
    ) -> i64 {
        let a = prov
            .insert_assertion(&NewRelationAssertion {
                subject_ref: format!("concept:{subject}"),
                predicate: RelationKind::Specializes,
                object_ref: "concept:broader".into(),
                epistemic_state: EpistemicState::Extracted,
                score: None,
                policy_version: None,
                created_by_run_id: None,
                supersedes_id: None,
                release_id: release,
                legacy_ref: Some(format!("concept_relation:{subject}")),
            })
            .unwrap();
        if let Some(sentence) = sentence {
            prov.insert_evidence(&NewEvidence {
                assertion_id: a,
                source_record_id: source,
                locator: Some(sentence.to_string()),
                evidence_kind: EvidenceKind::SourceSpan,
                extractor_or_model: None,
                version: None,
                input_hash: None,
                output_hash: None,
                metric_name: None,
                metric_value: None,
            })
            .unwrap();
        }
        if let Some(v) = metric_value {
            prov.insert_evidence(&NewEvidence {
                assertion_id: a,
                source_record_id: source,
                locator: None,
                evidence_kind: EvidenceKind::ModelOutput,
                extractor_or_model: None,
                version: None,
                input_hash: None,
                output_hash: None,
                metric_name: Some("invCL".into()),
                metric_value: Some(v),
            })
            .unwrap();
        }
        a.0
    }

    #[test]
    fn relation_edges_exclude_proposed_and_never_fabricate_grounded_confidence() {
        let (prov, release, source) = setup();
        insert_relation(&prov, release, source, "confirmed-case", Some("a sentence"), Some(0.87));
        insert_relation(&prov, release, source, "grounded-case", Some("another sentence"), None);
        insert_relation(&prov, release, source, "proposed-case", None, Some(0.5));

        let assertions = prov.list_assertions_for_release(release).unwrap();
        let relations = build_relation_edges(&prov, &assertions).unwrap();

        assert_eq!(relations.len(), 2, "根拠文の無いProposedは出さない");
        let confirmed = relations.iter().find(|r| r.subject == "confirmed-case").unwrap();
        assert_eq!(confirmed.status, "confirmed");
        assert_eq!(confirmed.confidence, Some(0.87));

        let grounded = relations.iter().find(|r| r.subject == "grounded-case").unwrap();
        assert_eq!(grounded.status, "grounded");
        assert_eq!(grounded.confidence, None, "Groundedに1.0を捏造しない");

        assert_eq!(relations[0].status, "confirmed", "Confirmedを先に出す");
    }
}

//! レガシースナップショットアダプタ（Phase 1, Increment 1）。
//!
//! `mathesis-graph`（judgment_dependencies/paper_citations/morphisms）と
//! `mathesis-taxonomy`（concept_relations）を`docs/DATA_DICTIONARY.md`の
//! マッピング表どおりに証拠層へ写す。ここで新しい判断は増やさない——
//! データディクショナリに書いていない分類はしない。
//!
//! 冪等性: 各行は`legacy_ref`（例: `"morphism:42"`）を持ち、
//! `(release_id, legacy_ref)`がユニーク制約なので、同じデータベースに対して
//! 同じリリースタグで再実行しても行は増えない。

use crate::model::{
    EpistemicState, EvidenceKind, NewEvidence, NewRelationAssertion, NewReviewDecision,
    NewSourceRecord, RelationKind, ReleaseId, ReviewOutcome, SourceRecordId,
};
use crate::store::ProvenanceStore;
use mathesis_graph::{EdgeOrigin, EdgeStatus, GraphStore, MorphismKind, PaperId};
use mathesis_taxonomy::relations::{RelationKind as TaxRelationKind, RelationStatus as TaxRelationStatus};
use mathesis_taxonomy::store::TaxonomyStore;
use std::collections::HashMap;

pub const ADAPTER_NAME: &str = "mathesis-provenance-legacy-adapter";
pub const ADAPTER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Default, Clone, Copy)]
pub struct ImportStats {
    pub dependencies_imported: usize,
    pub dependencies_skipped_existing: usize,
    pub citations_imported: usize,
    pub citations_skipped_existing: usize,
    pub morphisms_imported: usize,
    pub morphisms_skipped_existing: usize,
    pub review_decisions_created: usize,
    pub relations_imported: usize,
    pub relations_skipped_existing: usize,
}

/// レガシースナップショット全体を代表する、粗粒度の`SourceRecord`。
/// 1件の論文/ファイルに紐づかない行（分布的`Proposed`関係、由来論文が
/// 不明な射など）はここへ落とす（`docs/DATA_DICTIONARY.md`設計判断1）。
fn get_or_insert_snapshot_source(prov: &ProvenanceStore, release_tag: &str) -> anyhow::Result<SourceRecordId> {
    Ok(prov.get_or_insert_source_record(&NewSourceRecord {
        provider: "mathesis-legacy-snapshot".into(),
        provider_id: release_tag.to_string(),
        provider_revision: None,
        retrieved_at_unix: None,
        content_hash: None,
        licence: None,
        attribution: None,
        raw_payload_uri: None,
        adapter_name: ADAPTER_NAME.into(),
        adapter_version: ADAPTER_VERSION.into(),
        parser_version: None,
    })?)
}

fn get_or_insert_arxiv_source(prov: &ProvenanceStore, arxiv_id: &str) -> anyhow::Result<SourceRecordId> {
    Ok(prov.get_or_insert_source_record(&NewSourceRecord {
        provider: "arxiv".into(),
        provider_id: arxiv_id.to_string(),
        provider_revision: None,
        retrieved_at_unix: None,
        content_hash: None,
        licence: None,
        attribution: None,
        raw_payload_uri: None,
        adapter_name: ADAPTER_NAME.into(),
        adapter_version: ADAPTER_VERSION.into(),
        parser_version: None,
    })?)
}

/// `judgment_dependencies`・`paper_citations`・`morphisms`を証拠層へ写す。
pub fn import_graph(
    graph: &GraphStore,
    prov: &ProvenanceStore,
    release: ReleaseId,
    release_tag: &str,
) -> anyhow::Result<ImportStats> {
    let mut stats = ImportStats::default();
    let snapshot_source = get_or_insert_snapshot_source(prov, release_tag)?;
    let mut paper_source_cache: HashMap<i64, SourceRecordId> = HashMap::new();

    let mut source_for_paper = |graph: &GraphStore, paper: Option<PaperId>| -> anyhow::Result<SourceRecordId> {
        let Some(pid) = paper else { return Ok(snapshot_source) };
        if let Some(&sid) = paper_source_cache.get(&pid.0) {
            return Ok(sid);
        }
        let sid = match graph.get_paper(pid)? {
            Some(p) => get_or_insert_arxiv_source(prov, &p.arxiv_id)?,
            None => snapshot_source,
        };
        paper_source_cache.insert(pid.0, sid);
        Ok(sid)
    };

    // judgment_dependencies -> depends_on / extracted。elaborator検証ではなく
    // raw Lean文中の名前一致（judgment_dependency.rs参照）なので`observed`
    // ではない——`docs/DATA_DICTIONARY.md`「Resolved decisions #3」参照。
    for j in graph.list_judgments()? {
        for dep in graph.dependencies_of(j.id)? {
            let legacy_ref = format!("judgment_dependency:{}:{}", j.id.0, dep.0);
            if prov.get_assertion_by_legacy_ref(release, &legacy_ref)?.is_some() {
                stats.dependencies_skipped_existing += 1;
                continue;
            }
            let source_id = source_for_paper(graph, j.source_paper)?;
            let assertion_id = prov.insert_assertion(&NewRelationAssertion {
                subject_ref: format!("judgment:{}", j.id.0),
                predicate: RelationKind::DependsOn,
                object_ref: format!("judgment:{}", dep.0),
                epistemic_state: EpistemicState::Extracted,
                score: None,
                policy_version: None,
                created_by_run_id: Some(ADAPTER_NAME.into()),
                supersedes_id: None,
                release_id: release,
                legacy_ref: Some(legacy_ref),
            })?;
            prov.insert_evidence(&NewEvidence {
                assertion_id,
                source_record_id: source_id,
                locator: Some(format!("{}:{}", j.source_file, j.source_line)),
                evidence_kind: EvidenceKind::SourceSpan,
                extractor_or_model: Some("mathesis-importer".into()),
                version: None,
                input_hash: None,
                output_hash: None,
                metric_name: None,
                metric_value: None,
            })?;
            stats.dependencies_imported += 1;
        }
    }

    // paper_citations -> cites / observed
    let papers = graph.list_papers()?;
    let arxiv_by_paper_id: HashMap<i64, String> = papers.iter().map(|p| (p.id.0, p.arxiv_id.clone())).collect();
    for p in &papers {
        for target in graph.citations_of(p.id)? {
            let legacy_ref = format!("paper_citation:{}:{}", p.id.0, target.0);
            if prov.get_assertion_by_legacy_ref(release, &legacy_ref)?.is_some() {
                stats.citations_skipped_existing += 1;
                continue;
            }
            let Some(target_arxiv_id) = arxiv_by_paper_id.get(&target.0) else { continue };
            let citing_source = get_or_insert_arxiv_source(prov, &p.arxiv_id)?;
            let assertion_id = prov.insert_assertion(&NewRelationAssertion {
                subject_ref: format!("paper:{}", p.arxiv_id),
                predicate: RelationKind::Cites,
                object_ref: format!("paper:{}", target_arxiv_id),
                epistemic_state: EpistemicState::Observed,
                score: None,
                policy_version: None,
                created_by_run_id: Some(ADAPTER_NAME.into()),
                supersedes_id: None,
                release_id: release,
                legacy_ref: Some(legacy_ref),
            })?;
            // `locator: None` — 外部レビュー(2026-09-05)指摘の修正: 以前は
            // `"\cite in {arxiv_id}"`という、実際には何も位置特定していない
            // 説明文をlocatorに入れていた(subject_refで既に分かる情報の
            // 言い換えにすぎない)。`mathesis-fulltext::citation`はbibitemの
            // バイトオフセットを保持していない(citation.rsに位置情報は無い)
            // ので、無い精度をあるように見せるより`None`が正直。
            prov.insert_evidence(&NewEvidence {
                assertion_id,
                source_record_id: citing_source,
                locator: None,
                evidence_kind: EvidenceKind::SourceSpan,
                extractor_or_model: Some("mathesis-fulltext::citation".into()),
                version: None,
                input_hash: None,
                output_hash: None,
                metric_name: None,
                metric_value: None,
            })?;
            stats.citations_imported += 1;
        }
    }

    // morphisms -> implies/specializes/generalizes/equivalent_to。
    // epistemic_stateは`status`だけで決まる（`origin`はevidence_kindにのみ
    // 影響）——`Accepted`は`reviewed`にしない。`docs/DATA_DICTIONARY.md`
    // 「Resolved decisions #4」参照: legacy morphismにレビュアーの身元が
    // 無いため、`reviewed`(「説明責任を伴う人間の決定」)の定義を満たせない。
    // 旧`Accepted`という事実そのものは失わず、下の`ReviewDecision`として残す。
    for m in graph.list_morphisms()? {
        let legacy_ref = format!("morphism:{}", m.id.0);
        if prov.get_assertion_by_legacy_ref(release, &legacy_ref)?.is_some() {
            stats.morphisms_skipped_existing += 1;
            continue;
        }
        let predicate = match m.kind {
            MorphismKind::Implication => RelationKind::Implies,
            MorphismKind::Specialization => RelationKind::Specializes,
            MorphismKind::Generalization => RelationKind::Generalizes,
            MorphismKind::Equivalence => RelationKind::EquivalentTo,
        };
        let epistemic_state = match m.status {
            EdgeStatus::Accepted | EdgeStatus::Proposed => EpistemicState::Proposed,
            EdgeStatus::Rejected => EpistemicState::Rejected,
        };

        let src_judgment = graph.get_judgment(m.src)?;
        let source_id = source_for_paper(graph, src_judgment.source_paper)?;

        let assertion_id = prov.insert_assertion(&NewRelationAssertion {
            subject_ref: format!("judgment:{}", m.src.0),
            predicate,
            object_ref: format!("judgment:{}", m.dst.0),
            epistemic_state,
            score: None,
            policy_version: None,
            created_by_run_id: Some(ADAPTER_NAME.into()),
            supersedes_id: None,
            release_id: release,
            legacy_ref: Some(legacy_ref),
        })?;
        let evidence_kind = match m.origin {
            EdgeOrigin::Manual => EvidenceKind::ReviewerNote,
            EdgeOrigin::Heuristic => EvidenceKind::ModelOutput,
        };
        let extractor = match m.origin {
            EdgeOrigin::Manual => "human-annotator",
            EdgeOrigin::Heuristic => "mathesis-graph::heuristics",
        };
        prov.insert_evidence(&NewEvidence {
            assertion_id,
            source_record_id: source_id,
            locator: m.rationale.clone(),
            evidence_kind,
            extractor_or_model: Some(extractor.into()),
            version: None,
            input_hash: None,
            output_hash: None,
            metric_name: None,
            metric_value: None,
        })?;
        stats.morphisms_imported += 1;

        // `status == Accepted`は誰の起源(origin)でも起きうる（人間が
        // heuristic提案を`mathesis-annotate`で承認した場合を含む）。
        // reviewer_idは分からないので`None`のまま——それでもこの行自体が
        // 「かつてAcceptedだった」という事実の記録になる。
        if m.status == EdgeStatus::Accepted {
            prov.insert_review_decision(&NewReviewDecision {
                assertion_id,
                decision: ReviewOutcome::Accept,
                reviewer_id: None,
                scope: Some(release_tag.to_string()),
                rationale: m.rationale.clone(),
                decided_at_unix: m.created_at,
                dataset_version: None,
            })?;
            stats.review_decisions_created += 1;
        }
    }

    Ok(stats)
}

/// `mathesis-taxonomy`の`concept_relations`を証拠層へ写す。
pub fn import_taxonomy_relations(
    taxonomy: &TaxonomyStore,
    prov: &ProvenanceStore,
    release: ReleaseId,
    release_tag: &str,
) -> anyhow::Result<ImportStats> {
    let mut stats = ImportStats::default();
    let snapshot_source = get_or_insert_snapshot_source(prov, release_tag)?;

    for edge in taxonomy.load_relations()? {
        let kind_str = tax_kind_str(edge.kind);
        let legacy_ref = format!("concept_relation:{}|{}|{}", edge.subject, edge.object, kind_str);
        if prov.get_assertion_by_legacy_ref(release, &legacy_ref)?.is_some() {
            stats.relations_skipped_existing += 1;
            continue;
        }

        let predicate = match edge.kind {
            TaxRelationKind::SpecializationOf => RelationKind::Specializes,
            TaxRelationKind::EquivalentTo => RelationKind::EquivalentTo,
        };
        let epistemic_state = match edge.status {
            TaxRelationStatus::Proposed => EpistemicState::Proposed,
            TaxRelationStatus::Grounded => EpistemicState::Extracted,
            TaxRelationStatus::Confirmed => EpistemicState::Extracted,
        };

        // `RelationAssertion.score`は較正済み・比較可能な値専用に空けておく。
        // distributional detectorの"invCL"スコアは較正されていない
        // （`Proposed`では実測値、`Confirmed`でも実測値をそのまま保持——
        // `Grounded`だけがrelations.rsの1.0固定プレースホルダ）。実測値・
        // プレースホルダのどちらであれ、この値はscoreではなく下のEvidence
        // 行のmetric_name/metric_valueに置く（`docs/DATA_DICTIONARY.md`
        // 「Resolved decisions #1, #5」参照）。
        let assertion_id = prov.insert_assertion(&NewRelationAssertion {
            subject_ref: format!("concept:{}", edge.subject),
            predicate,
            object_ref: format!("concept:{}", edge.object),
            epistemic_state,
            score: None,
            policy_version: None,
            created_by_run_id: Some(ADAPTER_NAME.into()),
            supersedes_id: None,
            release_id: release,
            legacy_ref: Some(legacy_ref),
        })?;

        // Grounded/Confirmedは根拠文(Hearst)のEvidenceを必ず持つ。
        if let (Some(sentence), Some(arxiv_id)) = (&edge.evidence_sentence, &edge.evidence_arxiv_id) {
            let hearst_source = get_or_insert_arxiv_source(prov, arxiv_id)?;
            prov.insert_evidence(&NewEvidence {
                assertion_id,
                source_record_id: hearst_source,
                locator: Some(sentence.clone()),
                evidence_kind: EvidenceKind::SourceSpan,
                extractor_or_model: Some("hearst-pattern".into()),
                version: None,
                input_hash: None,
                output_hash: None,
                metric_name: None,
                metric_value: None,
            })?;
        }

        // Confirmed/Proposedは追加(またはもっぱら)でdistributional detectorの
        // 生スコア("invCL")をEvidenceに持つ。Proposedは根拠文を持たない
        // 代わりにこれが唯一のEvidenceになる。Groundedは対象外——1.0固定の
        // プレースホルダで、これ自体は「対応する分布的候補が無かった」ことの
        // 印にすぎず、測定値ではない。
        if matches!(edge.status, TaxRelationStatus::Confirmed | TaxRelationStatus::Proposed) {
            prov.insert_evidence(&NewEvidence {
                assertion_id,
                source_record_id: snapshot_source,
                locator: None,
                evidence_kind: EvidenceKind::ModelOutput,
                extractor_or_model: Some("distributional-containment".into()),
                version: None,
                input_hash: None,
                output_hash: None,
                metric_name: Some("invCL".into()),
                metric_value: Some(edge.confidence as f64),
            })?;
        }

        stats.relations_imported += 1;
    }

    Ok(stats)
}

fn tax_kind_str(kind: TaxRelationKind) -> &'static str {
    match kind {
        TaxRelationKind::SpecializationOf => "specialization_of",
        TaxRelationKind::EquivalentTo => "equivalent_to",
    }
}

//! P4: OpenAlexアダプタ本体（`docs/P4_PLAN.md`）。`msc_adapter.rs`と同じ
//! 直接構築方式——`SourceAdapter`トレイト(`source_adapter.rs`)は将来の
//! 未知アダプタ向けの契約であって、このクレート自身が書く2件目の実アダプタ
//! (`msc_adapter.rs`)もこれを経由していない。ライセンス/エンティティ種別
//! 検証は`licensing.rs`/`relation_policy.rs`の既存ゲートをそのまま使う。
//!
//! ネットワーク取得(`openalex_fetch.rs`)とインポート(この
//! ファイル)を分離している——`import`は`SnapshotEntry`の配列だけを
//! 受け取る純粋関数で、ネットワークに一切触れずテストできる
//! (`mathesis-fulltext`の「取得は別コマンド、テストは純粋関数だけ」と
//! 同じ設計)。
//!
//! 雪だるま式収集の境界をここで強制する: `referenced_work_ids`のうち、
//! **スナップショットに含まれる(=既にカタログ済みの)論文を指すものだけ**
//! がCitesアサーションになる。カタログ外の論文への参照は静かに無視する
//! ——OpenAlexグラフ全体へクロールを広げない(ARCHITECTURE_NEXT.md §3)。

use crate::model::{
    EntityKind, EpistemicState, EvidenceKind, NewEntity, NewEvidence, NewRelationAssertion, NewSourceRecord,
    RelationKind, ReleaseId,
};
use crate::openalex_fetch::SnapshotEntry;
use crate::relation_policy::SOURCE_MAPPING_POLICY_VERSION;
use crate::store::ProvenanceStore;
use std::collections::{HashMap, HashSet};

pub const ADAPTER_NAME: &str = "mathesis-provenance-openalex-adapter";
pub const ADAPTER_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const OPENALEX_LICENSE: &str = "CC0-1.0";
pub const OPENALEX_ATTRIBUTION: &str = "OpenAlex (https://openalex.org)";

#[derive(Debug, Default, Clone, Copy)]
pub struct ImportStats {
    /// このOpenAlex work idを初めて論文エンティティのaliasとして登録した件数
    /// (`get_or_insert_entity_with_refs`の`was_new`は「エンティティ自体が
    /// 新規か」しか見ないため、「aliasが新規か」は呼び出し前に
    /// `resolve_entity_ref`で確かめて別集計する — `build-catalog`と同じ
    /// 正直な+N(skip M)報告の規律)。
    pub papers_linked: usize,
    pub papers_already_linked: usize,
    pub citations_imported: usize,
    pub citations_skipped_existing: usize,
    /// カタログ外(スナップショットに無い)論文への参照——雪だるま式収集の
    /// 境界により意図的に無視した件数。0でも異常ではない。
    pub references_outside_catalog: usize,
}

/// `docs/DATA_DICTIONARY.md`と同じ形の由来キー。`legacy_adapter.rs`の
/// `paper_citation_legacy_ref`(内部judgment-graph paper id同士)とは
/// 名前空間が違う——同じ引用が将来両方の経路から取り込まれても、
/// 二重登録にはならないが別アサーションとして共存しうる(意図的、
/// 「複数の独立ソースが同じ事実を裏付ける」を1件に握り潰さない)。
fn cites_legacy_ref(citing_work_id: &str, cited_work_id: &str) -> String {
    format!("openalex:cites:{citing_work_id}:{cited_work_id}")
}

/// 取得済みスナップショットを証拠層へ写す。`release`は呼び出し側が
/// 事前に(`import-legacy`等で)作成済みのものを渡す——このアダプタは
/// リリースを新設しない。
pub fn import(prov: &ProvenanceStore, release: ReleaseId, snapshot: &[SnapshotEntry]) -> anyhow::Result<ImportStats> {
    for e in snapshot {
        if e.arxiv_id.trim().is_empty() || e.work_id.trim().is_empty() {
            anyhow::bail!("スナップショットに空のarxiv_id/work_idを持つ行がある — 破損したスナップショットを取り込まない");
        }
    }

    let work_to_arxiv: HashMap<&str, &str> =
        snapshot.iter().map(|e| (e.work_id.as_str(), e.arxiv_id.as_str())).collect();

    let mut stats = ImportStats::default();
    for entry in snapshot {
        let content_hash = entry.canonical_content_hash();
        let source_id = prov.get_or_insert_source_record(&NewSourceRecord {
            provider: "openalex".into(),
            provider_id: entry.work_id.clone(),
            provider_revision: Some(content_hash.clone()),
            retrieved_at_unix: Some(entry.retrieved_at_unix),
            content_hash: Some(content_hash.clone()),
            licence: Some(OPENALEX_LICENSE.into()),
            attribution: Some(OPENALEX_ATTRIBUTION.into()),
            raw_payload_uri: Some(format!("https://openalex.org/{}", entry.work_id)),
            adapter_name: ADAPTER_NAME.into(),
            adapter_version: ADAPTER_VERSION.into(),
            parser_version: Some("mathesis-provenance::openalex_fetch".into()),
            reproducibility_json: None,
        })?;

        let paper_ref = format!("paper:{}", entry.arxiv_id);
        let openalex_ref = format!("openalex:{}", entry.work_id);
        let already_linked = prov.resolve_entity_ref(&openalex_ref)?.is_some();
        let mut refs = vec![paper_ref.clone(), openalex_ref];
        if let Some(doi) = &entry.doi {
            refs.push(format!("doi:{doi}"));
        }
        let label = entry.title.clone().unwrap_or_else(|| entry.arxiv_id.clone());
        prov.get_or_insert_entity_with_refs(
            &NewEntity { kind: EntityKind::Paper, display_label: label, source_record_id: Some(source_id) },
            &refs,
        )?;
        if already_linked {
            stats.papers_already_linked += 1;
        } else {
            stats.papers_linked += 1;
        }

        for referenced in &entry.referenced_work_ids {
            let Some(&object_arxiv) = work_to_arxiv.get(referenced.as_str()) else {
                stats.references_outside_catalog += 1;
                continue;
            };
            if object_arxiv == entry.arxiv_id {
                continue; // 自己引用はノイズとして無視(実データでは起こらない想定の防御)
            }
            let legacy_ref = cites_legacy_ref(&entry.work_id, referenced);
            if prov.get_assertion_by_legacy_ref(release, &legacy_ref)?.is_some() {
                stats.citations_skipped_existing += 1;
                continue;
            }
            let assertion_id = prov.insert_assertion(&NewRelationAssertion {
                subject_ref: paper_ref.clone(),
                predicate: RelationKind::Cites,
                object_ref: format!("paper:{object_arxiv}"),
                epistemic_state: EpistemicState::Observed,
                score: None,
                policy_version: Some(SOURCE_MAPPING_POLICY_VERSION.into()),
                created_by_run_id: Some(ADAPTER_NAME.into()),
                supersedes_id: None,
                release_id: release,
                legacy_ref: Some(legacy_ref),
            })?;
            // `evidence_kind: SourceSpan`——同じ`Cites`述語を作る
            // `legacy_adapter.rs`のLaTeX`\cite`由来と揃える(`assertion_export.rs`の
            // `locator_precision`が`FormalExport`を"formal_artifact"に
            // マップする——Lean検証済み成果物のような機械検証済み証拠を
            // 指す語で、OpenAlexの引用グラフ(検証されたプルーフではなく
            // 書誌データベースの記述)をそう呼ぶのは過大な主張になる)。
            // `locator`は実在する具体的な参照先を持つので"approximate_location"
            // になる——LaTeX側の`locator: None`("source_only")より、むしろ
            // こちらの方が正確な位置情報を持っている。
            prov.insert_evidence(&NewEvidence {
                assertion_id,
                source_record_id: source_id,
                locator: Some(format!("OpenAlex work {} lists {} in referenced_works", entry.work_id, referenced)),
                evidence_kind: EvidenceKind::SourceSpan,
                extractor_or_model: Some(ADAPTER_NAME.into()),
                version: Some(ADAPTER_VERSION.into()),
                input_hash: Some(content_hash.clone()),
                output_hash: None,
                metric_name: None,
                metric_value: None,
                dependency_origin: None,
            })?;
            stats.citations_imported += 1;
        }
    }
    Ok(stats)
}

#[derive(Debug, Default)]
pub struct CitationCoverage {
    pub total_papers: usize,
    pub papers_with_any_citation_edge: usize,
}

impl CitationCoverage {
    pub fn print(&self) {
        println!(
            "citation coverage: {}/{} cataloged papers have at least one Cites edge",
            self.papers_with_any_citation_edge, self.total_papers
        );
    }
}

/// P4のstep 4「前は無引用だった論文が何件、引用リンクを獲得したか」を
/// 測る素朴な集計。`stats`から常時呼べるよう、OpenAlexを一度も実行して
/// いないDBでも(0/N として)安全に動く。
pub fn citation_coverage(prov: &ProvenanceStore) -> anyhow::Result<CitationCoverage> {
    let total_papers = prov
        .entity_count_by_kind()?
        .into_iter()
        .find(|(kind, _)| kind == "paper")
        .map(|(_, count)| count)
        .unwrap_or(0) as usize;

    let mut linked: HashSet<String> = HashSet::new();
    for a in prov.list_assertions()? {
        if a.predicate == RelationKind::Cites {
            linked.insert(a.subject_ref.clone());
            linked.insert(a.object_ref.clone());
        }
    }
    Ok(CitationCoverage { total_papers, papers_with_any_citation_edge: linked.len() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::NewRelease;
    use crate::ProvenanceStore;

    fn entry(arxiv_id: &str, work_id: &str, title: &str, refs: &[&str]) -> SnapshotEntry {
        SnapshotEntry {
            arxiv_id: arxiv_id.into(),
            work_id: work_id.into(),
            doi: Some(format!("10.48550/arXiv.{arxiv_id}")),
            title: Some(title.into()),
            referenced_work_ids: refs.iter().map(|s| s.to_string()).collect(),
            retrieved_at_unix: 0,
        }
    }

    fn store_with_release() -> (ProvenanceStore, ReleaseId) {
        let prov = ProvenanceStore::open_in_memory().unwrap();
        let release = prov
            .get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None })
            .unwrap();
        (prov, release)
    }

    #[test]
    fn imports_a_citation_between_two_cataloged_papers_idempotently() {
        let (prov, release) = store_with_release();
        let snapshot =
            vec![entry("a", "W1", "Paper A", &["W2"]), entry("b", "W2", "Paper B", &[])];

        let first = prov.transaction(|| import(&prov, release, &snapshot)).unwrap();
        assert_eq!(first.citations_imported, 1);
        assert_eq!(first.papers_linked, 2);

        let second = prov.transaction(|| import(&prov, release, &snapshot)).unwrap();
        assert_eq!(second.citations_imported, 0, "再実行で重複しない");
        assert_eq!(second.citations_skipped_existing, 1);
        assert_eq!(second.papers_linked, 0, "既存aliasは新規と数えない");
        assert_eq!(second.papers_already_linked, 2);
        assert_eq!(prov.assertion_count().unwrap(), 1);

        let a = prov.resolve_entity_ref("paper:a").unwrap().unwrap();
        let b = prov.resolve_entity_ref("paper:b").unwrap().unwrap();
        assert_ne!(a, b);
        assert_eq!(prov.resolve_entity_ref("openalex:W1").unwrap(), Some(a), "OpenAlex work idがaliasとして解決できる");
    }

    #[test]
    fn citation_to_an_uncataloged_paper_is_skipped_not_fabricated() {
        let (prov, release) = store_with_release();
        // "W2"はスナップショットに存在しない = カタログ外(雪だるま式収集の境界外)。
        let snapshot = vec![entry("a", "W1", "Paper A", &["W2", "W999"])];
        let stats = prov.transaction(|| import(&prov, release, &snapshot)).unwrap();
        assert_eq!(stats.citations_imported, 0);
        assert_eq!(stats.references_outside_catalog, 2);
        assert_eq!(prov.assertion_count().unwrap(), 0);
    }

    #[test]
    fn changed_snapshot_content_creates_a_new_source_revision() {
        let (prov, release) = store_with_release();
        let v1 = vec![entry("a", "W1", "Paper A", &[])];
        prov.transaction(|| import(&prov, release, &v1)).unwrap();
        assert_eq!(prov.source_record_count().unwrap(), 1);

        // 同じrerun: 内容が同じなら同じSourceRecordを再利用する。
        prov.transaction(|| import(&prov, release, &v1)).unwrap();
        assert_eq!(prov.source_record_count().unwrap(), 1, "内容が同じなら新しいリビジョンは作らない");

        // OpenAlex側でタイトルが訂正された想定 = 内容ハッシュが変わる。
        let v2 = vec![entry("a", "W1", "Paper A (corrected title)", &[])];
        assert_ne!(v1[0].canonical_content_hash(), v2[0].canonical_content_hash());
        prov.transaction(|| import(&prov, release, &v2)).unwrap();
        assert_eq!(prov.source_record_count().unwrap(), 2, "内容が変われば古いリビジョンを残したまま新しい行を作る");
    }

    #[test]
    fn refusing_a_malformed_snapshot_entry() {
        let (prov, release) = store_with_release();
        let bad = vec![SnapshotEntry {
            arxiv_id: "a".into(),
            work_id: "".into(),
            doi: None,
            title: None,
            referenced_work_ids: vec![],
            retrieved_at_unix: 0,
        }];
        assert!(import(&prov, release, &bad).is_err(), "空のwork_idは破損したスナップショットとして拒否すべき");
    }

    #[test]
    fn every_citation_has_exactly_one_resolvable_evidence_row() {
        let (prov, release) = store_with_release();
        let snapshot = vec![entry("a", "W1", "Paper A", &["W2"]), entry("b", "W2", "Paper B", &[])];
        prov.transaction(|| import(&prov, release, &snapshot)).unwrap();

        let assertions = prov.list_assertions().unwrap();
        let cite = assertions.iter().find(|a| a.predicate == RelationKind::Cites).unwrap();
        let evidence = crate::assertion_export::evidence_details_for(&prov, cite.id).unwrap();
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].evidence_kind, "source_span");
        assert_eq!(evidence[0].locator_precision, "approximate_location", "具体的なlocatorを持つのでformal_artifactを僭称しない");
        assert_eq!(evidence[0].source_provider, "openalex");
    }

    #[test]
    fn cites_assertions_pass_the_existing_relation_schema() {
        assert!(crate::relation_policy::valid_entity_kinds(RelationKind::Cites, EntityKind::Paper, EntityKind::Paper));
        assert!(!crate::relation_policy::valid_entity_kinds(RelationKind::Cites, EntityKind::Judgment, EntityKind::Judgment));

        // insert_assertion自身のガード(assertion.rs::validate_relation_kinds)にも
        // 同じ規約が効いていることを、この由来のrefフォーマットで直接確認する。
        let (prov, release) = store_with_release();
        let ok = prov.insert_assertion(&NewRelationAssertion {
            subject_ref: "paper:a".into(),
            predicate: RelationKind::Cites,
            object_ref: "paper:b".into(),
            epistemic_state: EpistemicState::Observed,
            score: None,
            policy_version: None,
            created_by_run_id: None,
            supersedes_id: None,
            release_id: release,
            legacy_ref: Some("test:1".into()),
        });
        assert!(ok.is_ok());

        let bad = prov.insert_assertion(&NewRelationAssertion {
            subject_ref: "judgment:1".into(),
            predicate: RelationKind::Cites,
            object_ref: "judgment:2".into(),
            epistemic_state: EpistemicState::Observed,
            score: None,
            policy_version: None,
            created_by_run_id: None,
            supersedes_id: None,
            release_id: release,
            legacy_ref: Some("test:2".into()),
        });
        assert!(bad.is_err(), "judgment同士のcitesは既存のガードで拒否されるべき");
    }

    #[test]
    fn missing_license_metadata_is_caught_by_the_existing_licensing_gate() {
        // このアダプタが実際に書き込むSourceRecordの形を、既存のlicensing.rs
        // ゲートにそのまま通す——新しい検証コードを増やさず、既存の契約を
        // このアダプタの出力に対しても再確認する。
        let (prov, release) = store_with_release();
        let snapshot = vec![entry("a", "W1", "Paper A", &[])];
        prov.transaction(|| import(&prov, release, &snapshot)).unwrap();

        let source_id = prov.resolve_entity_ref("paper:a").unwrap().unwrap();
        let entity = prov.get_entity(source_id).unwrap();
        let record = prov.get_source_record(entity.source_record_id.unwrap()).unwrap();
        assert!(crate::licensing::validate_source_record(&record).is_none(), "OpenAlexアダプタは常にlicense/attributionを埋める");

        let mut incomplete = record.clone();
        incomplete.licence = None;
        assert_eq!(
            crate::licensing::validate_source_record(&incomplete).unwrap().reason,
            "missing_license"
        );
    }
}

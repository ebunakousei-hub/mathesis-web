//! `stats`サブコマンド用の集計。`import-legacy`が書いたDBを独立に開いて
//! 検証できるよう、インポート実行時の`ImportStats`とは別に、保存済みデータ
//! そのものから数え直す。

use crate::catalog_adapter::{self, CoverageReport};
use crate::openalex_adapter::{self, CitationCoverage};
use crate::store::ProvenanceStore;

#[derive(Debug)]
pub struct Stats {
    pub release_count: i64,
    pub source_record_count: i64,
    pub assertion_count: i64,
    pub by_predicate: Vec<(String, i64)>,
    pub by_state: Vec<(String, i64)>,
    pub evidence_count: i64,
    /// (assertionあたりのevidence行数, その行数を持つassertionの件数)。
    pub evidence_histogram: Vec<(i64, i64)>,
    pub review_decision_count: i64,
    /// P3, Increment 1（`docs/P3_STATUS.md`）。`build-catalog`を一度も
    /// 実行していないDBでは両方とも0件のまま——それ自体はエラーではない。
    pub entity_count: i64,
    pub entity_count_by_kind: Vec<(String, i64)>,
    pub reference_coverage: CoverageReport,
    /// P4（`docs/P4_PLAN.md`）。`import-openalex`を一度も実行していない
    /// DBでも0/Nとして安全に表示できる——エンティティカタログの
    /// `reference_coverage`と違い「先に`build-catalog`を実行して」という
    /// 前提ゲートは要らない(cites述語がゼロ件なら単に0/total_papers)。
    pub citation_coverage: CitationCoverage,
    /// 改善点.txt項目9（`docs/PA_3_STATUS.md`）。`classify-msc`を一度も
    /// 実行していないDBでは空のまま——それ自体はエラーではない
    /// （他の`import-*`系コマンドと同じ「未実行なら0件」の扱い）。
    pub msc_classification_totals: Vec<(String, i64)>,
}

pub fn compute(prov: &ProvenanceStore) -> anyhow::Result<Stats> {
    Ok(Stats {
        release_count: prov.list_releases()?.len() as i64,
        source_record_count: prov.source_record_count()?,
        assertion_count: prov.assertion_count()?,
        by_predicate: prov.assertion_count_by_predicate()?,
        by_state: prov.assertion_count_by_state()?,
        evidence_count: prov.evidence_count()?,
        evidence_histogram: prov.evidence_count_histogram()?,
        review_decision_count: prov.review_decision_count()?,
        entity_count: prov.entity_count()?,
        entity_count_by_kind: prov.entity_count_by_kind()?,
        reference_coverage: catalog_adapter::assertion_reference_coverage(prov)?,
        citation_coverage: openalex_adapter::citation_coverage(prov)?,
        msc_classification_totals: prov.msc_classification_totals()?,
    })
}

impl Stats {
    pub fn print(&self) {
        println!("releases: {}", self.release_count);
        println!("source_records: {}", self.source_record_count);
        println!("assertions: {}", self.assertion_count);
        println!("  by predicate:");
        for (k, n) in &self.by_predicate {
            println!("    {k}: {n}");
        }
        println!("  by epistemic_state:");
        for (k, n) in &self.by_state {
            println!("    {k}: {n}");
        }
        println!("evidence: {}", self.evidence_count);
        println!("  rows-per-assertion histogram (rows, count):");
        for (rows, count) in &self.evidence_histogram {
            println!("    {rows}: {count}");
        }
        println!("review_decisions: {}", self.review_decision_count);
        println!("entities: {}", self.entity_count);
        for (k, n) in &self.entity_count_by_kind {
            println!("    {k}: {n}");
        }
        if self.entity_count > 0 {
            self.reference_coverage.print();
        } else {
            println!("  (run `build-catalog` to populate the entity catalog and see reference coverage)");
        }
        self.citation_coverage.print();
        if self.msc_classification_totals.is_empty() {
            println!("msc_classifications: 0 (run `classify-msc` to populate)");
        } else {
            let total: i64 = self.msc_classification_totals.iter().map(|(_, n)| n).sum();
            println!("msc_classifications: {total}");
            for (status, n) in &self.msc_classification_totals {
                println!("    {status}: {n}");
            }
        }
    }
}

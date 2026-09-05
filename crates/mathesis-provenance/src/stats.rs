//! `stats`サブコマンド用の集計。`import-legacy`が書いたDBを独立に開いて
//! 検証できるよう、インポート実行時の`ImportStats`とは別に、保存済みデータ
//! そのものから数え直す。

use crate::store::{ProvenanceStore, Result};

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
}

pub fn compute(prov: &ProvenanceStore) -> Result<Stats> {
    Ok(Stats {
        release_count: prov.list_releases()?.len() as i64,
        source_record_count: prov.source_record_count()?,
        assertion_count: prov.assertion_count()?,
        by_predicate: prov.assertion_count_by_predicate()?,
        by_state: prov.assertion_count_by_state()?,
        evidence_count: prov.evidence_count()?,
        evidence_histogram: prov.evidence_count_histogram()?,
        review_decision_count: prov.review_decision_count()?,
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
    }
}

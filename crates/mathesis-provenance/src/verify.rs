//! リリース単位の完全性ゲート（外部レビュー2026-09-05、提案1への対応）。
//!
//! `reconcile`が1回限りの検証で終わらせず、CI/リリースゲートとして
//! 繰り返し実行できるようにする。チェックする鎖は:
//!
//!   sidecarの各エントリ → assertion → evidence → source_record → release
//!
//! 1つでも欠けたら非ゼロ終了する。「サイドカーが指すidが存在しない」
//! 「assertionにevidenceが無い」「evidenceの参照先source_recordが無い」
//! 「releaseの食い違い」「サイドカー内の重複キーが別々のassertionを指す
//! （曖昧な解決）」「サイドカーが別のリリース/別の入力から生成された」の
//! 6種類を明示的に検査する。

use crate::manifest::{InputFileHash, ProvenanceManifest};
use crate::reconcile::{JudgmentsProvenanceExport, RelationsProvenanceExport};
use crate::store::ProvenanceStore;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct CheckFailure {
    pub check: &'static str,
    pub detail: String,
}

#[derive(Debug, Default)]
pub struct VerifyReport {
    pub checked_edges: usize,
    pub failures: Vec<CheckFailure>,
}

impl VerifyReport {
    pub fn is_ok(&self) -> bool {
        self.failures.is_empty()
    }

    fn fail(&mut self, check: &'static str, detail: impl Into<String>) {
        self.failures.push(CheckFailure { check, detail: detail.into() });
    }

    pub fn print(&self) {
        println!("verified {} sidecar entries", self.checked_edges);
        if self.failures.is_empty() {
            println!("OK — every entry resolves through sidecar -> assertion -> evidence -> source_record -> release.");
            return;
        }
        println!("FAILED — {} problem(s):", self.failures.len());
        for f in &self.failures {
            println!("  [{}] {}", f.check, f.detail);
        }
    }
}

/// `assertion_id`1件ぶんの鎖(assertion存在 → evidence≥1件 → 各evidenceの
/// source_record存在 → release一致)を検査する。複数のサイドカー種別から
/// 共通で呼ぶ。
fn verify_assertion_chain(
    prov: &ProvenanceStore,
    assertion_id: i64,
    expected_release_id: i64,
    context: &str,
    report: &mut VerifyReport,
) {
    use crate::model::AssertionId;
    let id = AssertionId(assertion_id);
    let assertion = match prov.try_get_assertion(id) {
        Ok(Some(a)) => a,
        Ok(None) => {
            report.fail("assertion_missing", format!("{context}: assertion #{assertion_id} does not exist"));
            return;
        }
        Err(e) => {
            report.fail("assertion_query_error", format!("{context}: {e}"));
            return;
        }
    };
    if assertion.release_id.0 != expected_release_id {
        report.fail(
            "release_mismatch",
            format!(
                "{context}: assertion #{assertion_id} belongs to release {}, expected {}",
                assertion.release_id.0, expected_release_id
            ),
        );
    }
    let evidence = match prov.evidence_for(id) {
        Ok(e) => e,
        Err(e) => {
            report.fail("evidence_query_error", format!("{context}: {e}"));
            return;
        }
    };
    if evidence.is_empty() {
        report.fail("assertion_without_evidence", format!("{context}: assertion #{assertion_id} has zero Evidence rows"));
    }
    for ev in &evidence {
        match prov.try_get_source_record(ev.source_record_id) {
            Ok(Some(_)) => {}
            Ok(None) => report.fail(
                "evidence_missing_source_record",
                format!("{context}: evidence #{} of assertion #{assertion_id} points to missing source_record #{}", ev.id.0, ev.source_record_id.0),
            ),
            Err(e) => report.fail("source_record_query_error", format!("{context}: {e}")),
        }
    }
}

/// キーが同じなのに別々のassertion idを指すエントリが無いか確かめる
/// （「重複した識別子キーが曖昧に解決される」の検査）。
fn check_no_ambiguous_keys<K: std::hash::Hash + Eq + std::fmt::Debug + Clone>(
    entries: impl Iterator<Item = (K, i64)>,
    label: &'static str,
    report: &mut VerifyReport,
) {
    let mut seen: HashMap<K, i64> = HashMap::new();
    for (key, assertion_id) in entries {
        match seen.get(&key) {
            Some(&existing) if existing != assertion_id => {
                report.fail(
                    "ambiguous_identity_key",
                    format!("{label}: key {key:?} resolves to both assertion #{existing} and #{assertion_id}"),
                );
            }
            _ => {
                seen.insert(key, assertion_id);
            }
        }
    }
}

pub struct VerifyInputs<'a> {
    pub manifest: &'a ProvenanceManifest,
    pub judgments_provenance: &'a JudgmentsProvenanceExport,
    pub relations_provenance: &'a RelationsProvenanceExport,
    /// 与えられていれば、マニフェストに記録済みのハッシュと突き合わせる
    /// （「サイドカーが別のリリース/入力から生成された」の検査）。
    pub live_input_files: &'a [InputFileHash],
}

pub fn verify_release(prov: &ProvenanceStore, inputs: &VerifyInputs) -> anyhow::Result<VerifyReport> {
    let mut report = VerifyReport::default();
    let m = inputs.manifest;

    // 1. release_id/tagの整合性: マニフェストの主張とDB本体が一致するか。
    match prov.get_release_by_tag(&m.release_tag)? {
        Some(release) => {
            if release.id.0 != m.release_id {
                report.fail(
                    "manifest_release_id_mismatch",
                    format!("manifest says release_id={}, but tag '{}' resolves to id={} in the DB", m.release_id, m.release_tag, release.id.0),
                );
            }
            if release.git_commit != m.release_git_commit {
                report.fail(
                    "manifest_release_commit_mismatch",
                    format!("manifest git_commit={:?}, DB release git_commit={:?}", m.release_git_commit, release.git_commit),
                );
            }
        }
        None => report.fail("manifest_release_not_found", format!("release tag '{}' not found in provenance DB", m.release_tag)),
    }

    // 2. サイドカー自身が主張するreleaseTag/GitCommitがマニフェストと一致するか。
    if inputs.judgments_provenance.release_tag != m.release_tag {
        report.fail(
            "sidecar_release_mismatch",
            format!(
                "judgments.provenance.json release_tag='{}' != manifest release_tag='{}'",
                inputs.judgments_provenance.release_tag, m.release_tag
            ),
        );
    }
    if inputs.relations_provenance.release_tag != m.release_tag {
        report.fail(
            "sidecar_release_mismatch",
            format!(
                "taxonomy.relations.provenance.json release_tag='{}' != manifest release_tag='{}'",
                inputs.relations_provenance.release_tag, m.release_tag
            ),
        );
    }

    // 3. 入力ファイルのハッシュ再検証: 生成時と今で入力が変わっていないか。
    //    パスの絶対位置は環境で変わりうるので、ファイル名の末尾一致で照合する。
    for live in inputs.live_input_files {
        let live_name = Path::new(&live.path).file_name().map(|f| f.to_string_lossy().to_string());
        let recorded = m.input_files.iter().find(|f| {
            Path::new(&f.path).file_name().map(|n| n.to_string_lossy().to_string()) == live_name
        });
        match recorded {
            Some(r) if r.sha256 != live.sha256 => report.fail(
                "input_file_hash_mismatch",
                format!("{}: manifest recorded sha256={}, current file has sha256={} — sidecar was generated from a different input", live.path, r.sha256, live.sha256),
            ),
            Some(_) => {}
            None => report.fail("input_file_not_in_manifest", format!("{} was not recorded in the manifest's input_files", live.path)),
        }
    }

    // 4. counts整合性: マニフェストの主張とサイドカーの実件数が一致するか。
    let jp = inputs.judgments_provenance;
    let rp = inputs.relations_provenance;
    if m.counts.dependencies != jp.dependencies.len() {
        report.fail("counts_mismatch", format!("manifest.counts.dependencies={} but sidecar has {}", m.counts.dependencies, jp.dependencies.len()));
    }
    if m.counts.citations != jp.citations.len() {
        report.fail("counts_mismatch", format!("manifest.counts.citations={} but sidecar has {}", m.counts.citations, jp.citations.len()));
    }
    if m.counts.morphisms != jp.morphisms.len() {
        report.fail("counts_mismatch", format!("manifest.counts.morphisms={} but sidecar has {}", m.counts.morphisms, jp.morphisms.len()));
    }
    if m.counts.relations != rp.relations.len() {
        report.fail("counts_mismatch", format!("manifest.counts.relations={} but sidecar has {}", m.counts.relations, rp.relations.len()));
    }

    // 5. 重複識別子キーの曖昧解決チェック。
    check_no_ambiguous_keys(jp.dependencies.iter().map(|d| ((d.from, d.to), d.assertion_id)), "dependencies", &mut report);
    check_no_ambiguous_keys(jp.citations.iter().map(|c| ((c.from.clone(), c.to.clone()), c.assertion_id)), "citations", &mut report);
    check_no_ambiguous_keys(jp.morphisms.iter().map(|m| (m.morphism_id, m.assertion_id)), "morphisms", &mut report);
    check_no_ambiguous_keys(
        rp.relations.iter().map(|r| ((r.subject.clone(), r.object.clone(), r.kind.clone()), r.assertion_id)),
        "relations",
        &mut report,
    );

    // 6. 鎖全体(assertion -> evidence -> source_record -> release)の検査。
    for d in &jp.dependencies {
        report.checked_edges += 1;
        verify_assertion_chain(prov, d.assertion_id, m.release_id, &format!("dependency {}->{}", d.from, d.to), &mut report);
    }
    for c in &jp.citations {
        report.checked_edges += 1;
        verify_assertion_chain(prov, c.assertion_id, m.release_id, &format!("citation {}->{}", c.from, c.to), &mut report);
    }
    for mo in &jp.morphisms {
        report.checked_edges += 1;
        verify_assertion_chain(prov, mo.assertion_id, m.release_id, &format!("morphism #{}", mo.morphism_id), &mut report);
    }
    for r in &rp.relations {
        report.checked_edges += 1;
        verify_assertion_chain(prov, r.assertion_id, m.release_id, &format!("relation {}|{}|{}", r.subject, r.object, r.kind), &mut report);
    }

    Ok(report)
}

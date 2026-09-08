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
use crate::relation_policy::valid_entity_kinds;
use crate::relation_policy::SOURCE_MAPPING_POLICY_VERSION;
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

fn verify_catalog_assertions(prov: &ProvenanceStore, release_id: i64, report: &mut VerifyReport) -> anyhow::Result<()> {
    if prov.entity_count()? == 0 {
        return Ok(());
    }
    for assertion in prov.list_assertions_for_release(crate::model::ReleaseId(release_id))? {
        let context = format!("assertion #{}", assertion.id.0);
        let subject = prov.resolve_entity_ref_with_kind(&assertion.subject_ref)?;
        let object = prov.resolve_entity_ref_with_kind(&assertion.object_ref)?;
        let (subject_kind, object_kind) = match (subject, object) {
            (Some((subject_id, subject_kind)), Some((object_id, object_kind))) => {
                if assertion.subject_entity_id != Some(subject_id) {
                    report.fail("entity_endpoint_drift", format!("{context}: subject_entity_id does not match catalog reference"));
                }
                if assertion.object_entity_id != Some(object_id) {
                    report.fail("entity_endpoint_drift", format!("{context}: object_entity_id does not match catalog reference"));
                }
                (subject_kind, object_kind)
            }
            (subject, object) => {
                if subject.is_none() {
                    report.fail("unresolved_entity_reference", format!("{context}: subject '{}' is not cataloged", assertion.subject_ref));
                }
                if object.is_none() {
                    report.fail("unresolved_entity_reference", format!("{context}: object '{}' is not cataloged", assertion.object_ref));
                }
                continue;
            }
        };
        if !valid_entity_kinds(assertion.predicate, subject_kind, object_kind) {
            report.fail(
                "relation_schema_mismatch",
                format!("{context}: {} does not allow {:?} -> {:?}", assertion.predicate.as_str(), subject_kind, object_kind),
            );
        }
    }
    Ok(())
}

/// Priority 2, step 2（ユーザー指示 2026-09-08）: 「`default_traversal`の
/// assertionは、正式な証拠(FormalExport evidence)か本人確認済みレビュー
/// (reviewer_idを持つReviewDecision{accept})のどちらかを持たない限り
/// 拒否する」というリリースゲート。`relation_policy::traversal_policy`が
/// 既定トラバース対象と判定した時点で、それを裏付ける証拠が本当に
/// あるかまでは検証していなかった——今後
/// (a) 誰かが手違いで`epistemic_state`だけ`observed`/`verified`に
///     書き換えたが証拠行はテキスト由来のまま、あるいは
/// (b) レビューは記録されているがreviewer_idが無い(誰が承認したか
///     分からない)まま`reviewed`に上げてしまった
/// といった事態が起きても、リリースを公開する前に機械的に検出する。
fn verify_trusted_assertions_have_qualifying_evidence(prov: &ProvenanceStore, release_id: i64, report: &mut VerifyReport) -> anyhow::Result<()> {
    use crate::model::{EvidenceKind, ReviewOutcome};
    use crate::relation_policy::{traversal_policy, TraversalPolicy};

    for assertion in prov.list_assertions_for_release(crate::model::ReleaseId(release_id))? {
        if traversal_policy(assertion.predicate, assertion.epistemic_state) != TraversalPolicy::DefaultTraversal {
            continue;
        }
        let has_formal_evidence = prov.evidence_for(assertion.id)?.iter().any(|e| e.evidence_kind == EvidenceKind::FormalExport);
        let has_authenticated_review = prov
            .review_decisions_for(assertion.id)?
            .iter()
            .any(|r| r.decision == ReviewOutcome::Accept && r.reviewer_id.is_some());
        if !has_formal_evidence && !has_authenticated_review {
            report.fail(
                "trusted_assertion_missing_qualifying_evidence",
                format!(
                    "assertion #{}: traversal_policy=default_traversal but has neither formal_export evidence nor an authenticated (reviewer_id-bearing) accept decision",
                    assertion.id.0
                ),
            );
        }
    }
    Ok(())
}

/// P6.1（ユーザー指示 2026-09-08、`docs/LEAN_DEPENDENCY_POLICY.md`
/// 「Reproducibility metadata」）: `default_traversal`まで届いた
/// `formal_export`証拠は、Lean/mathlibのバージョン・抽出器版・フィルタ
/// ポリシー版・raw/normalizedマニフェストハッシュを再現性メタデータ
/// として持たなければならない——`verify_trusted_assertions_have_
/// qualifying_evidence`は「evidence_kindがformal_exportかどうか」しか
/// 見ないので、evidence自体は存在するのに、それを生んだmanifestが
/// 実際どのLean版・どのフィルタルールで作られたのか分からない（=検証者が
/// 追試できない）事故を捕まえない。これはその一段深いチェック。
/// `filteringPolicyVersion`がこのビルドの
/// `lean_manifest_adapter::FILTERING_POLICY_VERSION`と食い違う場合も
/// 拒否する——`SOURCE_MAPPING_POLICY_VERSION`と同じ「ポリシーが古い証拠を
/// 黙って信頼しない」という扱い。
fn verify_formal_evidence_has_reproducibility_metadata(prov: &ProvenanceStore, release_id: i64, report: &mut VerifyReport) -> anyhow::Result<()> {
    use crate::lean_manifest_adapter::FILTERING_POLICY_VERSION;
    use crate::model::EvidenceKind;
    use crate::relation_policy::{traversal_policy, TraversalPolicy};

    const REQUIRED_KEYS: [&str; 6] =
        ["leanToolchain", "mathlibRev", "extractorVersion", "filteringPolicyVersion", "rawManifestHash", "normalizedManifestHash"];

    for assertion in prov.list_assertions_for_release(crate::model::ReleaseId(release_id))? {
        if traversal_policy(assertion.predicate, assertion.epistemic_state) != TraversalPolicy::DefaultTraversal {
            continue;
        }
        for ev in prov.evidence_for(assertion.id)? {
            if ev.evidence_kind != EvidenceKind::FormalExport {
                continue;
            }
            let context = format!("assertion #{} evidence #{} (formal_export)", assertion.id.0, ev.id.0);
            let Some(source) = prov.try_get_source_record(ev.source_record_id)? else { continue }; // 別チェック(evidence_missing_source_record)が既に検出する
            let Some(json_str) = &source.reproducibility_json else {
                report.fail("formal_evidence_missing_reproducibility_metadata", format!("{context}: source_record #{} has no reproducibility_json", source.id.0));
                continue;
            };
            let parsed: serde_json::Value = match serde_json::from_str(json_str) {
                Ok(v) => v,
                Err(e) => {
                    report.fail("formal_evidence_missing_reproducibility_metadata", format!("{context}: reproducibility_json is not valid JSON: {e}"));
                    continue;
                }
            };
            for key in REQUIRED_KEYS {
                let present_and_nonempty = parsed.get(key).and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty());
                if !present_and_nonempty {
                    report.fail("formal_evidence_missing_reproducibility_metadata", format!("{context}: reproducibility_json is missing or empty '{key}'"));
                }
            }
            if let Some(policy) = parsed.get("filteringPolicyVersion").and_then(|v| v.as_str()) {
                if policy != FILTERING_POLICY_VERSION {
                    report.fail(
                        "formal_evidence_filtering_policy_mismatch",
                        format!("{context}: reproducibility_json filteringPolicyVersion='{policy}', this build uses '{FILTERING_POLICY_VERSION}'"),
                    );
                }
            }
        }
    }
    Ok(())
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

    if !m.source_mapping_policy_version.is_empty()
        && m.source_mapping_policy_version != SOURCE_MAPPING_POLICY_VERSION
    {
        report.fail(
            "source_mapping_policy_mismatch",
            format!(
                "manifest uses '{}', this build uses '{}'",
                m.source_mapping_policy_version, SOURCE_MAPPING_POLICY_VERSION
            ),
        );
    }

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

    if let Some(catalog) = &m.catalog {
        match prov.catalog_metadata()? {
            Some(live) => {
                if catalog.schema_version != live.schema_version
                    || catalog.build_version != live.build_version
                    || catalog.entity_resolution_version != live.entity_resolution_version
                    || catalog.graph_input_sha256 != live.graph_input_sha256
                    || catalog.taxonomy_input_sha256 != live.taxonomy_input_sha256
                    || catalog.entity_count != live.entity_count
                    || catalog.alias_count != live.alias_count
                {
                    report.fail(
                        "catalog_metadata_mismatch",
                        "provenance manifest catalog metadata does not match the live catalog",
                    );
                }
            }
            None => report.fail("catalog_metadata_missing", "manifest records a catalog, but the provenance DB has none"),
        }
    }

    if let Err(e) = verify_catalog_assertions(prov, m.release_id, &mut report) {
        report.fail("catalog_query_error", e.to_string());
    }

    // 4.5. Priority 2, step 2: 既定トラバース対象を裏付ける証拠の有無
    // （カタログの有無に関わらず常に検査する——正式なEvidence/認証済み
    // レビューの有無はentity catalogと独立の話のため）。
    if let Err(e) = verify_trusted_assertions_have_qualifying_evidence(prov, m.release_id, &mut report) {
        report.fail("trusted_evidence_query_error", e.to_string());
    }

    // 4.6. P6.1: formal_export証拠自体の再現性メタデータ(Lean/mathlib版・
    // 抽出器版・フィルタポリシー版・raw/normalizedハッシュ)の有無と整合性。
    if let Err(e) = verify_formal_evidence_has_reproducibility_metadata(prov, m.release_id, &mut report) {
        report.fail("reproducibility_metadata_query_error", e.to_string());
    }

    // 5. 重複識別子キーの曖昧解決チェック。
    //
    // `dependencies`だけは対象外——P6.1でreconcileがcheckerーderived
    // (`lean-manifest:...`)分もsidecarへ含めるよう直した結果(実データで
    // 確認済み)、同じ(from,to)組がtext-extracted由来とchecker-derived由来の
    // *2つの正当な別assertion*を持つケースが実在する(比較の「agree」件、
    // `docs/P6_STATUS.md`)——これは事故による曖昧解決ではなく、2つの独立
    // ソースが同じ辺を裏付けているという意図した状態そのもの。
    // `DependencyProvenance`はソース種別を持たないので、ここでは
    // 「ソースをまたいだ重複は正当、同一ソース内の重複だけが事故」という
    // 区別を付けられない——各アダプタ自身の冪等性は
    // `relation_assertions`テーブルの`UNIQUE(release_id, legacy_ref)`制約が
    // 既に保証している(このsidecarとは独立の、より強い保証)ので、ここでの
    // チェック省略は実害が無い。citations/morphisms/relationsは今も
    // 1ソース1辺のままなので従来通り検査する。
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

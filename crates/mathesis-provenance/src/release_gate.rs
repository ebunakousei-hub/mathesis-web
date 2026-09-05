//! P2 stabilization: a single release gate that additionally covers what
//! `verify.rs` does not — the `mathesis-provenance web-export` outputs
//! themselves (`dependencies.json`/`morphisms.json`/`relations.json`).
//!
//! `verify.rs` proves the chain `sidecar -> assertion -> evidence ->
//! source_record -> release` for the legacy edges. It says nothing about
//! whether the *shipped* `dependencies.json`/`morphisms.json`/
//! `relations.json` still match what the `ProvenanceStore` would produce
//! right now — a release could ship a `web-export` output that was
//! generated before a later `import-legacy` run, and `verify` alone would
//! not catch it (the sidecars and the store's chain are still internally
//! consistent; only the *served* file is stale).
//!
//! This module closes that gap by re-deriving the expected output with
//! `web_export::build_web_export` and comparing it — structurally, not by
//! trusting a hash alone — against what is actually on disk. A byte-hash
//! comparison against the manifest catches accidental hand-edits or
//! transfer corruption; the structural regeneration comparison catches the
//! "generated from a different commit/release" case even if someone
//! recomputed the hash to match a stale file.
//!
//! `mathesis-provenance verify-release` runs both this and `verify::
//! verify_release` under one command and one exit code — see `main.rs`.

use crate::manifest::WebExportManifest;
use crate::model::ReleaseId;
use crate::store::ProvenanceStore;
use crate::verify::CheckFailure;
use crate::web_export::build_web_export;
use serde::Serialize;
use std::path::Path;

fn fail(failures: &mut Vec<CheckFailure>, check: &'static str, detail: impl Into<String>) {
    failures.push(CheckFailure { check, detail: detail.into() });
}

/// 1ファイルぶんの検査: 存在するか、マニフェストに記録されたハッシュと
/// 一致するか、そして**今のProvenanceStoreから再生成した内容**と構造的に
/// 一致するか。最後の比較がこのゲートの本体——ハッシュ一致だけでは
/// 「別のコミット/リリースから生成されたファイルのハッシュを、マニフェスト
/// 側もそのファイルに合わせて計算し直してしまった」ケースを検出できない。
fn check_output_file<T: Serialize>(
    failures: &mut Vec<CheckFailure>,
    dir: &Path,
    filename: &'static str,
    manifest: &WebExportManifest,
    live: &[T],
) {
    let path = dir.join(filename);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            fail(failures, "web_export_file_missing", format!("{filename}: {e} (partially copied export directory?)"));
            return;
        }
    };

    let recorded = manifest.output_files.iter().find(|f| Path::new(&f.path).file_name().map(|n| n.to_string_lossy().to_string()) == Some(filename.to_string()));
    match recorded {
        None => {
            fail(failures, "output_file_not_in_manifest", format!("{filename} was not recorded in the web-export manifest's output_files"));
        }
        Some(r) => {
            let actual_hash = match crate::manifest::sha256_file(&path) {
                Ok(h) => h,
                Err(e) => {
                    fail(failures, "output_hash_compute_error", format!("{filename}: {e}"));
                    return;
                }
            };
            if actual_hash != r.sha256 {
                fail(
                    failures,
                    "output_hash_mismatch",
                    format!("{filename}: manifest recorded sha256={}, file on disk has sha256={}", r.sha256, actual_hash),
                );
            }
        }
    }

    let on_disk: Vec<serde_json::Value> = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => {
            fail(failures, "malformed_json", format!("{filename}: {e}"));
            return;
        }
    };
    let live_json: Vec<serde_json::Value> = live
        .iter()
        .map(|v| serde_json::to_value(v).expect("web_export structs always serialize"))
        .collect();
    if on_disk.len() != live_json.len() {
        fail(
            failures,
            "web_export_stale",
            format!(
                "{filename}: on disk has {} entries, but the current ProvenanceStore would generate {} — regenerate with `web-export`",
                on_disk.len(),
                live_json.len()
            ),
        );
    } else if on_disk != live_json {
        fail(
            failures,
            "web_export_stale",
            format!("{filename}: content on disk does not match what the current ProvenanceStore would generate — regenerate with `web-export`"),
        );
    }
}

/// `manifest`が自己申告するschema versionとrelease素性を確かめ、3ファイル
/// それぞれを`check_output_file`で検査する。`expected_release_tag`は
/// 呼び出し側（`verify-release`のCLI引数）が期待するリリースタグ——
/// マニフェスト自身の記述と食い違えば「別のリリースから生成された」ことの
/// 合図になる。
pub fn verify_web_export(
    prov: &ProvenanceStore,
    expected_release_tag: &str,
    manifest: &WebExportManifest,
    web_export_dir: &Path,
) -> anyhow::Result<Vec<CheckFailure>> {
    let mut failures = Vec::new();

    if manifest.schema_version != crate::manifest::WEB_EXPORT_SCHEMA_VERSION {
        fail(
            &mut failures,
            "unknown_web_export_schema_version",
            format!(
                "web-export manifest declares schema_version={}, this build of mathesis-provenance only understands {}",
                manifest.schema_version,
                crate::manifest::WEB_EXPORT_SCHEMA_VERSION
            ),
        );
    }
    if manifest.release_tag != expected_release_tag {
        fail(
            &mut failures,
            "web_export_release_mismatch",
            format!("web-export manifest release_tag='{}', expected '{expected_release_tag}'", manifest.release_tag),
        );
    }

    let release = match prov.get_release_by_tag(&manifest.release_tag)? {
        Some(r) => r,
        None => {
            fail(&mut failures, "manifest_release_not_found", format!("release tag '{}' not found in provenance DB", manifest.release_tag));
            return Ok(failures);
        }
    };
    if release.id.0 != manifest.release_id {
        fail(
            &mut failures,
            "manifest_release_id_mismatch",
            format!("web-export manifest says release_id={}, but tag '{}' resolves to id={} in the DB", manifest.release_id, manifest.release_tag, release.id.0),
        );
    }

    let live = build_web_export(prov, ReleaseId(manifest.release_id))?;

    if manifest.counts.dependencies != live.dependencies.len() {
        fail(&mut failures, "counts_mismatch", format!("manifest.counts.dependencies={} but the live store has {}", manifest.counts.dependencies, live.dependencies.len()));
    }
    if manifest.counts.morphisms != live.morphisms.len() {
        fail(&mut failures, "counts_mismatch", format!("manifest.counts.morphisms={} but the live store has {}", manifest.counts.morphisms, live.morphisms.len()));
    }
    if manifest.counts.relations != live.relations.len() {
        fail(&mut failures, "counts_mismatch", format!("manifest.counts.relations={} but the live store has {}", manifest.counts.relations, live.relations.len()));
    }

    check_output_file(&mut failures, web_export_dir, "dependencies.json", manifest, &live.dependencies);
    check_output_file(&mut failures, web_export_dir, "morphisms.json", manifest, &live.morphisms);
    check_output_file(&mut failures, web_export_dir, "relations.json", manifest, &live.relations);

    Ok(failures)
}

pub fn print_web_export_failures(failures: &[CheckFailure]) {
    if failures.is_empty() {
        println!("OK — dependencies.json/morphisms.json/relations.json match the live ProvenanceStore and the web-export manifest.");
        return;
    }
    println!("FAILED — {} web-export problem(s):", failures.len());
    for f in failures {
        println!("  [{}] {}", f.check, f.detail);
    }
}

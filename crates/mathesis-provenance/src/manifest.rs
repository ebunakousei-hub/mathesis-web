//! 機械可読な出所マニフェスト（外部レビュー2026-09-05、提案2への対応）。
//!
//! サイドカー(`judgments.provenance.json`/`taxonomy.relations.provenance.json`)
//! が「どの入力から・どのアダプタで・いつ作られたか」を自己申告する。
//! これが無いと、古いサイドカーが新しい静的JSON/DBと黙って組み合わさっても
//! 検出できない——`verify.rs`はこのマニフェストと実際のDB/ファイルを
//! 突き合わせて、そのズレを検出する。

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

/// `store.rs`のSCHEMA定数を意味のある形で変えたら上げる。現状は
/// マイグレーションを持たない設計（`mathesis-graph`と同じ方針、
/// `CREATE TABLE IF NOT EXISTS`の追記のみ）なので、破壊的変更をしたときの
/// 目印として使う。
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputFileHash {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ManifestCounts {
    pub dependencies: usize,
    pub citations: usize,
    pub morphisms: usize,
    pub relations: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvenanceManifest {
    pub release_tag: String,
    pub release_id: i64,
    pub release_git_commit: Option<String>,
    pub source_database_schema: u32,
    pub adapter_name: String,
    pub adapter_version: String,
    pub input_files: Vec<InputFileHash>,
    pub generated_at_unix: i64,
    pub counts: ManifestCounts,
}

pub fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let bytes = std::fs::read(path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    // sha2 0.11の出力型(`hybrid_array::Array`)は`LowerHex`を実装しないため、
    // バイト列から自前で16進文字列を組み立てる。
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

pub fn input_file_hash(path: &Path) -> anyhow::Result<InputFileHash> {
    Ok(InputFileHash { path: path.display().to_string(), sha256: sha256_file(path)? })
}

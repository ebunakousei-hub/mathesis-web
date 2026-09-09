//! 改善点.txt項目9（`docs/PA_3_STATUS.md`）: MSC分類の明示的なステータスモデル。
//!
//! `mathesis-taxonomy`自身の`cluster_alignments`テーブル（クラスタごとの
//! `dominant_code`/`confidence`を1行だけ持つ、履歴もリリース紐付けも無い
//! 私的なテーブル）とは意図的に別物として作る。理由:
//!
//! 1. `classified`/`pending`（=ambiguous）はクラスタに対する「分類の主張」
//!    があるので、本来は`RelationAssertion`（subject/predicate/object）に
//!    近い——だが`unclassified`/`unavailable`/`outside_scope`は分類の
//!    **不在**を表すステータスで、主張する対象（object）が無い。
//!    `subject_ref`/`predicate`/`object_ref`の3つ組では「存在しない」を
//!    表現できない——`RelationAssertion`を無理に使うと、行が無いことと
//!    「明示的にunclassifiedと判定した」ことが区別できなくなる
//!    （このプロジェクトが繰り返し警告している「暗黙の欠落」そのもの）。
//! 2. クラスタは統計的な導出物で、パイプラインを再実行するたびに構成が
//!    変わりうる——`entities`テーブル（judgment/concept/paperという実世界の
//!    対象1件=1行、`docs/P3_STATUS.md`）が前提とする永続的な同一性とは
//!    性質が違う。新しい`EntityKind::ConceptCluster`を増やすことも検討したが、
//!    `subject_ref`/`object_ref`が使う`"kind:id"`タグの実装全体に波及する
//!    変更になるため、このリリースでは見送り、`cluster_id`（整数）を
//!    そのまま参照する独立したテーブルにした。
//!
//! `review_status`は列として保持する（項目9の要求どおり）が、このテーブル
//! 専用のレビューワークフロー（`review`/`promote-review`のような別コマンド）は
//! まだ実装していない——本プロジェクトの他の軸（`review_decisions`が本番で
//! 0件）と同じく、「まだ誰もレビューしていない」を正直に`unreviewed`固定値で
//! 表す。

use crate::model::ReleaseId;
use crate::msc_adapter::snapshot_hash;
use crate::store::{ProvenanceStore, Result};
use mathesis_taxonomy::alignment::ClusterAlignment;
use mathesis_taxonomy::store::TaxonomyStore;
use rusqlite::params;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub const CLASSIFIER_VERSION: &str = "mathesis-taxonomy::alignment-v1";
pub const CLASSIFICATION_SOURCE: &str = "mathesis-taxonomy::cluster_alignments";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ClassificationStatus {
    /// `grounded_count >= 2 && confidence > 0.5`——`ClusterAlignment::is_confident()`。
    Classified,
    /// grounded memberはいるが多数派に届かない（旧"ambiguous"、
    /// `export.rs`のexportからこれまで漏れていた層。項目6/9で初めて可視化）。
    Pending,
    /// `grounded_count == 0`——`ClusterAlignment::is_novel()`。
    Unclassified,
    /// このアダプタは分類を試みてすらいない対象（Lean判断・Math-Graph
    /// 由来ノード等、taxonomyパイプラインの対象外）向けに予約——
    /// このリリースではクラスタ以外への分類実行自体をまだ行っていないため0件。
    Unavailable,
    /// MSCの主題範囲そのものに該当しないという明示的な判定向けに予約——
    /// 現在のパイプラインにこれを判定する根拠信号が無いため0件
    /// （「信頼度が低い」＝`Pending`と、「範囲外」は別の主張であり、
    /// 前者から後者を捏造しない）。
    OutsideScope,
    /// 人間のレビューが分類を明示的に却下した場合向けに予約——
    /// このテーブル専用のレビューワークフローが未実装のため0件。
    Rejected,
}

impl ClassificationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Classified => "classified",
            Self::Pending => "pending",
            Self::Unclassified => "unclassified",
            Self::Unavailable => "unavailable",
            Self::OutsideScope => "outside_scope",
            Self::Rejected => "rejected",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "classified" => Self::Classified,
            "pending" => Self::Pending,
            "unclassified" => Self::Unclassified,
            "unavailable" => Self::Unavailable,
            "outside_scope" => Self::OutsideScope,
            "rejected" => Self::Rejected,
            _ => return None,
        })
    }

    /// `ClusterAlignment`は`classified`/`pending`/`unclassified`の3状態しか
    /// 自然には生まれない——他の3状態はこの関数では絶対に返らない
    /// （フィールドのコメント参照）。
    fn from_alignment(a: &ClusterAlignment) -> Self {
        if a.is_novel() {
            Self::Unclassified
        } else if a.is_confident() {
            Self::Classified
        } else {
            Self::Pending
        }
    }
}

#[derive(Debug, Clone)]
pub struct MscClassification {
    pub id: i64,
    pub cluster_id: i64,
    pub representative_label: String,
    pub status: ClassificationStatus,
    pub msc_code: Option<String>,
    pub msc_code_name: Option<String>,
    /// `Some(true)`: 実際に投票で選ばれたコードそのもの。`Some(false)`:
    /// その祖先への繰り上げ（現行の`align_cluster`は常に投票結果を
    /// そのまま返すため、このリリースでは常に`Some(true)`かつ`None`
    /// （分類が無い行）——将来、直接分類から祖先コードへの分離した
    /// 繰り上げ行を持たせる余地として列だけ用意する。
    pub is_direct: Option<bool>,
    pub msc_revision: String,
    pub source: String,
    pub classifier_version: String,
    pub confidence: Option<f64>,
    pub grounded_count: i64,
    pub cluster_size: i64,
    pub evidence_locator: Option<String>,
    pub review_status: String,
    pub release_id: ReleaseId,
    pub created_at_unix: i64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ClassificationStats {
    pub classified: usize,
    pub pending: usize,
    pub unclassified: usize,
    pub total_clusters: usize,
}

impl ProvenanceStore {
    pub fn upsert_msc_classification(&self, row: &MscClassificationInsert) -> Result<()> {
        self.conn
            .prepare_cached(
                "INSERT INTO msc_classifications
                    (cluster_id, representative_label, status, msc_code, msc_code_name, is_direct,
                     msc_revision, source, classifier_version, confidence, grounded_count,
                     cluster_size, evidence_locator, review_status, release_id, created_at_unix)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                 ON CONFLICT(cluster_id, release_id) DO UPDATE SET
                    representative_label = excluded.representative_label,
                    status = excluded.status,
                    msc_code = excluded.msc_code,
                    msc_code_name = excluded.msc_code_name,
                    is_direct = excluded.is_direct,
                    msc_revision = excluded.msc_revision,
                    source = excluded.source,
                    classifier_version = excluded.classifier_version,
                    confidence = excluded.confidence,
                    grounded_count = excluded.grounded_count,
                    cluster_size = excluded.cluster_size,
                    evidence_locator = excluded.evidence_locator,
                    created_at_unix = excluded.created_at_unix",
            )?
            .execute(params![
                row.cluster_id,
                row.representative_label,
                row.status.as_str(),
                row.msc_code,
                row.msc_code_name,
                row.is_direct,
                row.msc_revision,
                row.source,
                row.classifier_version,
                row.confidence,
                row.grounded_count,
                row.cluster_size,
                row.evidence_locator,
                row.review_status,
                row.release_id.0,
                row.created_at_unix,
            ])?;
        Ok(())
    }

    pub fn msc_classifications_for_release(&self, release: ReleaseId) -> Result<Vec<MscClassification>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, cluster_id, representative_label, status, msc_code, msc_code_name, is_direct,
                    msc_revision, source, classifier_version, confidence, grounded_count, cluster_size,
                    evidence_locator, review_status, release_id, created_at_unix
             FROM msc_classifications WHERE release_id = ?1 ORDER BY cluster_id",
        )?;
        let rows = stmt
            .query_map(params![release.0], Self::msc_classification_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// `stats`コマンド用、リリースを跨いだDB全体の内訳
    /// （改善点.txt項目9の「coverage metrics」要求の最小版——項目10の
    /// ソース別・レコード種別ごとの詳しい内訳はまだ別の作業）。
    pub fn msc_classification_totals(&self) -> Result<Vec<(String, i64)>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT status, COUNT(*) FROM msc_classifications GROUP BY status ORDER BY status")?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn msc_classification_stats(&self, release: ReleaseId) -> Result<ClassificationStats> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT status, COUNT(*) FROM msc_classifications WHERE release_id = ?1 GROUP BY status",
        )?;
        let mut stats = ClassificationStats::default();
        let rows = stmt.query_map(params![release.0], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
        })?;
        for row in rows {
            let (status, count) = row?;
            match ClassificationStatus::from_str(&status) {
                Some(ClassificationStatus::Classified) => stats.classified = count,
                Some(ClassificationStatus::Pending) => stats.pending = count,
                Some(ClassificationStatus::Unclassified) => stats.unclassified = count,
                _ => {}
            }
            stats.total_clusters += count;
        }
        Ok(stats)
    }

    fn msc_classification_row(row: &rusqlite::Row) -> rusqlite::Result<MscClassification> {
        let status_str: String = row.get(3)?;
        Ok(MscClassification {
            id: row.get(0)?,
            cluster_id: row.get(1)?,
            representative_label: row.get(2)?,
            status: ClassificationStatus::from_str(&status_str).unwrap_or(ClassificationStatus::Unclassified),
            msc_code: row.get(4)?,
            msc_code_name: row.get(5)?,
            is_direct: row.get(6)?,
            msc_revision: row.get(7)?,
            source: row.get(8)?,
            classifier_version: row.get(9)?,
            confidence: row.get(10)?,
            grounded_count: row.get(11)?,
            cluster_size: row.get(12)?,
            evidence_locator: row.get(13)?,
            review_status: row.get(14)?,
            release_id: ReleaseId(row.get(15)?),
            created_at_unix: row.get(16)?,
        })
    }
}

pub struct MscClassificationInsert {
    pub cluster_id: i64,
    pub representative_label: String,
    pub status: ClassificationStatus,
    pub msc_code: Option<String>,
    pub msc_code_name: Option<String>,
    pub is_direct: Option<bool>,
    pub msc_revision: String,
    pub source: String,
    pub classifier_version: String,
    pub confidence: Option<f64>,
    pub grounded_count: i64,
    pub cluster_size: i64,
    pub evidence_locator: Option<String>,
    pub review_status: String,
    pub release_id: ReleaseId,
    pub created_at_unix: i64,
}

/// `taxonomy_db`（`mathesis-taxonomy`自身のSQLite、例:
/// `scratch/papers_100k_fc.db`）を直接開いて`cluster_alignments`を読み、
/// 6状態モデルへ分類してから`msc_classifications`へ書く——`reconcile`が
/// `GraphStore`/`TaxonomyStore`を直接開くのと同じ、既存のクロスクレート
/// 読み取りの前例に倣う。`Unavailable`/`OutsideScope`/`Rejected`は
/// このアダプタでは1件も生成しない（型のコメント参照）。
pub fn classify_from_taxonomy(
    prov: &ProvenanceStore,
    taxonomy_db: &Path,
    release: ReleaseId,
) -> anyhow::Result<ClassificationStats> {
    let taxonomy = TaxonomyStore::open(taxonomy_db)?;
    let alignments = taxonomy.load_cluster_alignments()?;

    // 代表ラベル: `export.rs`のdoc_freq順ソートとは違い、監査目的の表示
    // なので決定的であれば十分——クラスタに属する語をアルファベット順で
    // 最小のものを選ぶ（フロントエンドの表示順と一致させる必要は無い）。
    let clusters = taxonomy.load_clusters()?;
    let mut members_by_cluster: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for (phrase, cluster_id) in clusters {
        members_by_cluster.entry(cluster_id).or_default().push(phrase);
    }

    let msc_revision = format!("bundled-{}", snapshot_hash());
    let now_unix = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);

    let mut stats = ClassificationStats::default();
    for alignment in &alignments {
        let status = ClassificationStatus::from_alignment(alignment);
        let representative_label = members_by_cluster
            .get(&alignment.cluster_id)
            .and_then(|members| members.iter().min().cloned())
            .unwrap_or_else(|| format!("cluster #{}", alignment.cluster_id));

        prov.upsert_msc_classification(&MscClassificationInsert {
            cluster_id: alignment.cluster_id as i64,
            representative_label,
            status,
            msc_code: alignment.dominant_code.clone(),
            msc_code_name: alignment.dominant_name.clone(),
            is_direct: alignment.dominant_code.as_ref().map(|_| true),
            msc_revision: msc_revision.clone(),
            source: CLASSIFICATION_SOURCE.to_string(),
            classifier_version: CLASSIFIER_VERSION.to_string(),
            confidence: if alignment.grounded_count > 0 { Some(alignment.confidence as f64) } else { None },
            grounded_count: alignment.grounded_count as i64,
            cluster_size: alignment.size as i64,
            evidence_locator: Some(format!("cluster_id={}", alignment.cluster_id)),
            review_status: "unreviewed".to_string(),
            release_id: release,
            created_at_unix: now_unix,
        })?;

        match status {
            ClassificationStatus::Classified => stats.classified += 1,
            ClassificationStatus::Pending => stats.pending += 1,
            ClassificationStatus::Unclassified => stats.unclassified += 1,
            _ => unreachable!("from_alignment only ever returns Classified/Pending/Unclassified"),
        }
        stats.total_clusters += 1;
    }

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alignment(grounded_count: usize, confidence: f32, code: Option<&str>) -> ClusterAlignment {
        ClusterAlignment {
            cluster_id: 1,
            size: 10,
            grounded_count,
            dominant_code: code.map(str::to_string),
            dominant_name: code.map(|_| "Some field".to_string()),
            confidence,
        }
    }

    #[test]
    fn zero_grounded_members_is_unclassified() {
        let a = alignment(0, 0.0, None);
        assert_eq!(ClassificationStatus::from_alignment(&a), ClassificationStatus::Unclassified);
    }

    #[test]
    fn confident_majority_is_classified() {
        let a = alignment(3, 0.8, Some("18A05"));
        assert_eq!(ClassificationStatus::from_alignment(&a), ClassificationStatus::Classified);
    }

    #[test]
    fn low_confidence_with_some_grounding_is_pending_not_unclassified() {
        // grounded_count=1 は is_confident() の条件(>=2)を満たさないため
        // Pending になる——これが項目6/9で新しく可視化した"ambiguous"層。
        let a = alignment(1, 1.0, Some("18A05"));
        assert_eq!(ClassificationStatus::from_alignment(&a), ClassificationStatus::Pending);
    }

    #[test]
    fn tied_confidence_is_pending() {
        let a = alignment(4, 0.5, Some("18A05"));
        assert_eq!(ClassificationStatus::from_alignment(&a), ClassificationStatus::Pending);
    }

    #[test]
    fn status_round_trips_through_as_str() {
        for status in [
            ClassificationStatus::Classified,
            ClassificationStatus::Pending,
            ClassificationStatus::Unclassified,
            ClassificationStatus::Unavailable,
            ClassificationStatus::OutsideScope,
            ClassificationStatus::Rejected,
        ] {
            assert_eq!(ClassificationStatus::from_str(status.as_str()), Some(status));
        }
    }

    #[test]
    fn classify_from_taxonomy_against_a_file_backed_taxonomy_store_produces_the_expected_split() {
        // classify_from_taxonomy opens the taxonomy DB by path (matching
        // reconcile's own cross-crate pattern), so this test needs a real
        // file-backed TaxonomyStore, not an in-memory one.
        let dir = tempfile_dir();
        let path = dir.join("taxonomy.db");
        let mut taxonomy = mathesis_taxonomy::store::TaxonomyStore::open(&path).unwrap();
        taxonomy.save_clusters(&[("alpha".into(), 0), ("beta".into(), 1), ("gamma".into(), 2)]).unwrap();
        taxonomy
            .save_alignment(
                &[
                    alignment_with_id(0, 2, 0.8, Some("18A05")), // classified
                    alignment_with_id(1, 1, 1.0, Some("18A05")), // pending (grounded_count<2)
                    alignment_with_id(2, 0, 0.0, None),          // unclassified
                ],
                &[],
            )
            .unwrap();
        drop(taxonomy);

        let prov = ProvenanceStore::open_in_memory().unwrap();
        let release = prov
            .get_or_insert_release(&crate::model::NewRelease {
                tag: "test-release".into(),
                git_commit: None,
                generated_at_unix: 0,
                notes: None,
            })
            .unwrap();

        let stats = classify_from_taxonomy(&prov, &path, release).unwrap();
        assert_eq!(stats, ClassificationStats { classified: 1, pending: 1, unclassified: 1, total_clusters: 3 });

        let rows = prov.msc_classifications_for_release(release).unwrap();
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().any(|r| r.status == ClassificationStatus::Classified && r.msc_code.as_deref() == Some("18A05")));
        assert!(rows.iter().any(|r| r.status == ClassificationStatus::Pending));
        assert!(rows.iter().any(|r| r.status == ClassificationStatus::Unclassified && r.msc_code.is_none()));
    }

    fn alignment_with_id(cluster_id: usize, grounded_count: usize, confidence: f32, code: Option<&str>) -> ClusterAlignment {
        ClusterAlignment {
            cluster_id,
            size: 5,
            grounded_count,
            dominant_code: code.map(str::to_string),
            dominant_name: code.map(|_| "Some field".to_string()),
            confidence,
        }
    }

    fn tempfile_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mathesis-msc-classification-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}

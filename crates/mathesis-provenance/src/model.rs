//! `docs/DATA_DICTIONARY.md`が定義する証拠層（evidence core）のデータモデル。
//!
//! `mathesis-graph::model`の慣習（`i64`のタプルニュータイプID、
//! `New*`(未採番)/`*Record`(採番済み)の対、enumは`as_str()`/`from_str()`で
//! TEXTに往復させる）をそのまま踏襲する。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ReleaseId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SourceRecordId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AssertionId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EvidenceId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ReviewId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EntityId(pub i64);

/// P3, Increment 1（ARCHITECTURE_NEXT.md §5.2の`Paper`/`Statement`/`Concept`の
/// 最小版、`docs/P3_STATUS.md`）。`subject_ref`/`object_ref`が使う`"kind:id"`
/// タグの`kind`側3種と1対1で対応する——新しい種類を増やすときはそちらの
/// プレフィックス一覧も合わせて確認すること。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntityKind {
    Judgment,
    Concept,
    Paper,
}

impl EntityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EntityKind::Judgment => "judgment",
            EntityKind::Concept => "concept",
            EntityKind::Paper => "paper",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "judgment" => EntityKind::Judgment,
            "concept" => EntityKind::Concept,
            "paper" => EntityKind::Paper,
            _ => return None,
        })
    }
}

/// `docs/P3_STATUS.md`: レガシーデータが既に持っている表示名をそのまま
/// 写すだけで、新しい判断は増やさない（`display_label`はjudgmentなら
/// `name`、無ければ種別のみ；paperなら`title`、無ければarXiv id；
/// conceptなら代表表記——いずれも捏造ではなく既存フィールドの転記）。
/// `source_record_id`はこの増分では常に`None`——エンティティ自体の由来
/// 追跡は将来の増分に残し、今回は「参照が実在するか」の解決表に絞る。
#[derive(Debug, Clone)]
pub struct NewEntity {
    pub kind: EntityKind,
    pub display_label: String,
    pub source_record_id: Option<SourceRecordId>,
}

#[derive(Debug, Clone)]
pub struct Entity {
    pub id: EntityId,
    pub kind: EntityKind,
    pub display_label: String,
    pub source_record_id: Option<SourceRecordId>,
}

/// `docs/DATA_DICTIONARY.md`の関係カインド。ARCHITECTURE_NEXT.md §4.2の8種に
/// `Implies`を加えた9種（2026-09-05、ユーザーの決定でMorphismKind::Implicationを
/// `depends_on`から分離したときに追加——`depends_on`は`judgment_dependencies`
/// 由来の機械的な事実専用に残す）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RelationKind {
    DependsOn,
    Imports,
    Cites,
    Specializes,
    EquivalentTo,
    Generalizes,
    RelatedTo,
    UsesConcept,
    Implies,
}

impl RelationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RelationKind::DependsOn => "depends_on",
            RelationKind::Imports => "imports",
            RelationKind::Cites => "cites",
            RelationKind::Specializes => "specializes",
            RelationKind::EquivalentTo => "equivalent_to",
            RelationKind::Generalizes => "generalizes",
            RelationKind::RelatedTo => "related_to",
            RelationKind::UsesConcept => "uses_concept",
            RelationKind::Implies => "implies",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "depends_on" => RelationKind::DependsOn,
            "imports" => RelationKind::Imports,
            "cites" => RelationKind::Cites,
            "specializes" => RelationKind::Specializes,
            "equivalent_to" => RelationKind::EquivalentTo,
            "generalizes" => RelationKind::Generalizes,
            "related_to" => RelationKind::RelatedTo,
            "uses_concept" => RelationKind::UsesConcept,
            "implies" => RelationKind::Implies,
            _ => return None,
        })
    }
}

/// `docs/DATA_DICTIONARY.md`の6状態（ARCHITECTURE_NEXT.md §4.2 verbatim）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EpistemicState {
    Observed,
    Extracted,
    Proposed,
    Reviewed,
    Verified,
    Rejected,
}

impl EpistemicState {
    pub fn as_str(self) -> &'static str {
        match self {
            EpistemicState::Observed => "observed",
            EpistemicState::Extracted => "extracted",
            EpistemicState::Proposed => "proposed",
            EpistemicState::Reviewed => "reviewed",
            EpistemicState::Verified => "verified",
            EpistemicState::Rejected => "rejected",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "observed" => EpistemicState::Observed,
            "extracted" => EpistemicState::Extracted,
            "proposed" => EpistemicState::Proposed,
            "reviewed" => EpistemicState::Reviewed,
            "verified" => EpistemicState::Verified,
            "rejected" => EpistemicState::Rejected,
            _ => return None,
        })
    }
}

/// ARCHITECTURE_NEXT.md §5.3の`Evidence.evidence_kind`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EvidenceKind {
    SourceSpan,
    FormalExport,
    ModelOutput,
    ReviewerNote,
}

impl EvidenceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EvidenceKind::SourceSpan => "source_span",
            EvidenceKind::FormalExport => "formal_export",
            EvidenceKind::ModelOutput => "model_output",
            EvidenceKind::ReviewerNote => "reviewer_note",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "source_span" => EvidenceKind::SourceSpan,
            "formal_export" => EvidenceKind::FormalExport,
            "model_output" => EvidenceKind::ModelOutput,
            "reviewer_note" => EvidenceKind::ReviewerNote,
            _ => return None,
        })
    }
}

/// ARCHITECTURE_NEXT.md §10の決定outcome（accept/reject/split/merge/needs expert）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReviewOutcome {
    Accept,
    Reject,
    Split,
    Merge,
    NeedsExpert,
}

impl ReviewOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            ReviewOutcome::Accept => "accept",
            ReviewOutcome::Reject => "reject",
            ReviewOutcome::Split => "split",
            ReviewOutcome::Merge => "merge",
            ReviewOutcome::NeedsExpert => "needs_expert",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "accept" => ReviewOutcome::Accept,
            "reject" => ReviewOutcome::Reject,
            "split" => ReviewOutcome::Split,
            "merge" => ReviewOutcome::Merge,
            "needs_expert" => ReviewOutcome::NeedsExpert,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct NewRelease {
    pub tag: String,
    pub git_commit: Option<String>,
    pub generated_at_unix: i64,
    pub notes: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Release {
    pub id: ReleaseId,
    pub tag: String,
    pub git_commit: Option<String>,
    pub generated_at_unix: i64,
    pub notes: Option<String>,
}

/// ARCHITECTURE_NEXT.md §5.1。レガシーアダプタが作るものは、実データの大半で
/// `content_hash`/`licence`/`raw_payload_uri`が`None`のまま——旧パイプラインが
/// そもそも記録していなかった項目を後から捏造しない（[[never-fabricate-math-notation]]
/// と同じ精神）。
#[derive(Debug, Clone)]
pub struct NewSourceRecord {
    pub provider: String,
    pub provider_id: String,
    pub provider_revision: Option<String>,
    pub retrieved_at_unix: Option<i64>,
    pub content_hash: Option<String>,
    pub licence: Option<String>,
    pub attribution: Option<String>,
    pub raw_payload_uri: Option<String>,
    pub adapter_name: String,
    pub adapter_version: String,
    pub parser_version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SourceRecord {
    pub id: SourceRecordId,
    pub provider: String,
    pub provider_id: String,
    pub provider_revision: Option<String>,
    pub retrieved_at_unix: Option<i64>,
    pub content_hash: Option<String>,
    pub licence: Option<String>,
    pub attribution: Option<String>,
    pub raw_payload_uri: Option<String>,
    pub adapter_name: String,
    pub adapter_version: String,
    pub parser_version: Option<String>,
}

/// ARCHITECTURE_NEXT.md §5.3。`subject_ref`/`object_ref`は本来`subject_id`/
/// `object_id`（§5.2のStatement/Concept/Paperカタログを指す）だが、そのカタログ
/// スキーマはPhase 1のこの増分にはまだ無い（`docs/DATA_DICTIONARY.md`の
/// 設計判断2）。`"judgment:<id>"`/`"concept:<phrase>"`/`"paper:<arxiv_id>"`の
/// ようなタグ付き文字列で代用し、カタログができたら差し替える。
#[derive(Debug, Clone)]
pub struct NewRelationAssertion {
    pub subject_ref: String,
    pub predicate: RelationKind,
    pub object_ref: String,
    pub epistemic_state: EpistemicState,
    /// 較正済みで比較可能な値がある場合のみ埋める。無ければ`None`のままにする
    /// ——証拠の本数（corroboration）を数値化した代用スコアにはしない
    /// （`docs/DATA_DICTIONARY.md`「Evidence multiplicity, not epistemic-state
    /// inflation」の決定）。
    pub score: Option<f64>,
    pub policy_version: Option<String>,
    pub created_by_run_id: Option<String>,
    pub supersedes_id: Option<AssertionId>,
    pub release_id: ReleaseId,
    /// レガシーアダプタが冪等に再実行できるように残す由来キー
    /// （例: `"morphism:42"`, `"concept_relation:a|b|specialization_of"`）。
    /// 同じ(release_id, legacy_ref)の組は`relation_assertions`テーブルの
    /// UNIQUE制約で二重登録されない。
    pub legacy_ref: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RelationAssertion {
    pub id: AssertionId,
    pub subject_ref: String,
    pub predicate: RelationKind,
    pub object_ref: String,
    pub epistemic_state: EpistemicState,
    pub score: Option<f64>,
    pub policy_version: Option<String>,
    pub created_by_run_id: Option<String>,
    pub supersedes_id: Option<AssertionId>,
    pub release_id: ReleaseId,
    pub legacy_ref: Option<String>,
}

/// `metric_name`/`metric_value`はARCHITECTURE_NEXT.md §5.3の原型には無い
/// 追加項目（2026-09-05、ユーザーの決定）。`RelationEdge.confidence`
/// （distributional detectorの非較正スコア、例: "invCL"）は`assertion.score`
/// （較正済みの比較可能な値専用、`docs/DATA_DICTIONARY.md`参照）に入れると
/// 較正済み確率であるかのように読めてしまうため、`model_output`種別の
/// Evidence行にだけ載せる。
#[derive(Debug, Clone)]
pub struct NewEvidence {
    pub assertion_id: AssertionId,
    pub source_record_id: SourceRecordId,
    pub locator: Option<String>,
    pub evidence_kind: EvidenceKind,
    pub extractor_or_model: Option<String>,
    pub version: Option<String>,
    pub input_hash: Option<String>,
    pub output_hash: Option<String>,
    pub metric_name: Option<String>,
    pub metric_value: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct Evidence {
    pub id: EvidenceId,
    pub assertion_id: AssertionId,
    pub source_record_id: SourceRecordId,
    pub locator: Option<String>,
    pub evidence_kind: EvidenceKind,
    pub extractor_or_model: Option<String>,
    pub version: Option<String>,
    pub input_hash: Option<String>,
    pub output_hash: Option<String>,
    pub metric_name: Option<String>,
    pub metric_value: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct NewReviewDecision {
    pub assertion_id: AssertionId,
    pub decision: ReviewOutcome,
    pub reviewer_id: Option<String>,
    pub scope: Option<String>,
    pub rationale: Option<String>,
    pub decided_at_unix: i64,
    pub dataset_version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ReviewDecision {
    pub id: ReviewId,
    pub assertion_id: AssertionId,
    pub decision: ReviewOutcome,
    pub reviewer_id: Option<String>,
    pub scope: Option<String>,
    pub rationale: Option<String>,
    pub decided_at_unix: i64,
    pub dataset_version: Option<String>,
}

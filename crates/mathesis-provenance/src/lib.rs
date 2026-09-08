//! Phase 1, Increment 1（`docs/DATA_DICTIONARY.md`、ARCHITECTURE_NEXT.md §12）:
//! 証拠層（evidence core）のスキーマと、レガシースナップショットアダプタ。
//!
//! `SourceRecord`/`Evidence`/`RelationAssertion`/`ReviewDecision`/`Release`を
//! `mathesis-graph`と同じ「1本のSCHEMA定数 + 機能ごとの実装ファイル」方式で
//! 実装する。`legacy_adapter`が`mathesis-graph`（判断・射・引用）と
//! `mathesis-taxonomy`（概念間関係）の既存データを、このスキーマへ
//! `docs/DATA_DICTIONARY.md`のマッピング表どおりに写す——ここでは`web/`にも
//! 既存のexportパイプラインにも触れない（それは次の増分の仕事）。

pub mod assertion;
pub mod assertion_export;
pub mod catalog_adapter;
pub mod catalog_metadata;
pub mod entity;
pub mod error;
pub mod evidence;
pub mod legacy_adapter;
pub mod manifest;
pub mod model;
pub mod reconcile;
pub mod release;
pub mod release_gate;
pub mod relation_policy;
pub mod source_adapter;
pub mod licensing;
pub mod lean_manifest_adapter;
pub mod msc_adapter;
pub mod openalex_adapter;
pub mod openalex_fetch;
pub mod review;
pub mod source_record;
pub mod stats;
pub mod store;
pub mod verify;
pub mod web_export;

pub use error::{ProvenanceError, ProvenanceResult, ValidationError};
pub use model::{
    AssertionId, CatalogMetadata, EntityId, EntityKind, Entity, EpistemicState, Evidence, EvidenceId, EvidenceKind, LabelOrigin, NewEntity,
    NewEvidence, NewRelationAssertion, NewRelease, NewReviewDecision, NewSourceRecord, RelationAssertion,
    RelationKind, Release, ReleaseId, ReviewDecision, ReviewId, ReviewOutcome, SourceRecord, SourceRecordId,
};
pub use store::ProvenanceStore;

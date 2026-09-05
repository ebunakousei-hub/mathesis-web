//! `mathesis-graph::error`と同じ方針: 既存APIは`rusqlite::Result`のまま残し、
//! 検証が要る操作（`insert_assertion`のkind整合性チェック）だけこちらを使う。

use crate::model::RelationKind;
use std::fmt;

#[derive(Debug)]
pub enum ProvenanceError {
    Sqlite(rusqlite::Error),
    Validation(ValidationError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// 外部レビュー（2026-09-05）指摘: `subject_ref`/`object_ref`は
    /// `"kind:id"`形式のタグ付き文字列にすぎず、DBは`paper:X specializes
    /// paper:Y`のような無意味な組み合わせを拒否できない。完全な型付き
    /// カタログ（ARCHITECTURE_NEXT.md §5.2）はこの増分の範囲外だが、
    /// このアダプタが実際に生成する組み合わせだけは検証する
    /// （`legacy_adapter.rs`が作る行の範囲でのガードレール）。
    RelationKindMismatch {
        predicate: RelationKind,
        subject_ref: String,
        object_ref: String,
    },
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidationError::RelationKindMismatch { predicate, subject_ref, object_ref } => write!(
                f,
                "{} is not a valid subject/object kind pair for predicate {}",
                format_args!("({subject_ref}, {object_ref})"),
                predicate.as_str()
            ),
        }
    }
}

impl fmt::Display for ProvenanceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProvenanceError::Sqlite(e) => write!(f, "{e}"),
            ProvenanceError::Validation(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ProvenanceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ProvenanceError::Sqlite(e) => Some(e),
            ProvenanceError::Validation(_) => None,
        }
    }
}

impl From<rusqlite::Error> for ProvenanceError {
    fn from(e: rusqlite::Error) -> Self {
        ProvenanceError::Sqlite(e)
    }
}

impl From<ValidationError> for ProvenanceError {
    fn from(e: ValidationError) -> Self {
        ProvenanceError::Validation(e)
    }
}

pub type ProvenanceResult<T> = std::result::Result<T, ProvenanceError>;

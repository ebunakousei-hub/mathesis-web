//! 層3の検証エラー。フェーズ1の rusqlite::Result は既存 API に残し、
//! 射の挿入・承認だけこちらを使う。

use crate::failed_attempt::FailedAttemptId;
use crate::model::JudgmentId;
use crate::morphism::{EdgeStatus, MorphismId, MorphismKind};
use crate::strategy::StrategyId;
use std::fmt;

#[derive(Debug)]
pub enum GraphError {
    Sqlite(rusqlite::Error),
    Validation(ValidationError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    MissingEndpoint {
        which: &'static str,
        id: JudgmentId,
    },
    SelfLoop,
    ExclusiveKindConflict {
        existing: MorphismKind,
        requested: MorphismKind,
        existing_id: MorphismId,
    },
    NotFound(MorphismId),
    InvalidStatusTransition {
        from: EdgeStatus,
        to: EdgeStatus,
    },
    /// 層4: タグ付け対象の射が存在しない
    MissingMorphism(MorphismId),
    /// 層4: 参照した戦略ノードが存在しない
    StrategyNotFound(StrategyId),
    /// 層4: 参照した失敗試行ノードが存在しない
    FailedAttemptNotFound(FailedAttemptId),
    /// 層4: 失敗試行の対象として指定した判断ノードが存在しない
    MissingTargetJudgment(JudgmentId),
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidationError::MissingEndpoint { which, id } => {
                write!(f, "missing {which} judgment {}", id.0)
            }
            ValidationError::SelfLoop => write!(f, "morphism cannot be a self-loop"),
            ValidationError::ExclusiveKindConflict {
                existing,
                requested,
                existing_id,
            } => write!(
                f,
                "accepted {} edge {} already occupies this pair; cannot add {}",
                existing.as_str(),
                existing_id.0,
                requested.as_str()
            ),
            ValidationError::NotFound(id) => write!(f, "morphism {} not found", id.0),
            ValidationError::InvalidStatusTransition { from, to } => {
                write!(
                    f,
                    "cannot change status from {} to {}",
                    from.as_str(),
                    to.as_str()
                )
            }
            ValidationError::MissingMorphism(id) => {
                write!(f, "morphism {} not found", id.0)
            }
            ValidationError::StrategyNotFound(id) => {
                write!(f, "strategy {} not found", id.0)
            }
            ValidationError::FailedAttemptNotFound(id) => {
                write!(f, "failed attempt {} not found", id.0)
            }
            ValidationError::MissingTargetJudgment(id) => {
                write!(f, "target judgment {} not found", id.0)
            }
        }
    }
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GraphError::Sqlite(e) => write!(f, "{e}"),
            GraphError::Validation(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for GraphError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            GraphError::Sqlite(e) => Some(e),
            GraphError::Validation(_) => None,
        }
    }
}

impl From<rusqlite::Error> for GraphError {
    fn from(e: rusqlite::Error) -> Self {
        GraphError::Sqlite(e)
    }
}

impl From<ValidationError> for GraphError {
    fn from(e: ValidationError) -> Self {
        GraphError::Validation(e)
    }
}

pub type GraphResult<T> = std::result::Result<T, GraphError>;

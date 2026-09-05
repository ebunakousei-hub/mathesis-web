//! 層3: 射・依存関係層（Morphism/Dependency Layer）のデータモデル。
//!
//! エッジは次の4種に排他分類する。証明項そのものの埋め込みはフェーズ3だが、
//! `proof_term_hash` / `dependency_signature` の列は先に用意しておく。
//!
//! フェーズ2では自動検出を承認に直結させない。ヒューリスティックは
//! `Proposed` として蓄積し、人間（または後段の学習）が `Accepted` にする。

use crate::model::JudgmentId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MorphismId(pub i64);

/// 層3の4種の射。同一の有向対 (src, dst) に対して Accepted な射は高々1種。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum MorphismKind {
    /// A → B。証明項は（フェーズ3で）関数 `λx. …`。
    Implication,
    /// A ⇒ B。A の条件を強めたものが B（コンテキスト拡張）。
    /// 例: 群 → アーベル群。
    Specialization,
    /// A ⇐ B。特殊化の逆方向。
    Generalization,
    /// A ↔ B。両端は同一の同値類に属する。
    Equivalence,
}

impl MorphismKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MorphismKind::Implication => "implication",
            MorphismKind::Specialization => "specialization",
            MorphismKind::Generalization => "generalization",
            MorphismKind::Equivalence => "equivalence",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "implication" => MorphismKind::Implication,
            "specialization" => MorphismKind::Specialization,
            "generalization" => MorphismKind::Generalization,
            "equivalence" => MorphismKind::Equivalence,
            _ => return None,
        })
    }

    /// 特殊化 ↔ 一般化。含意は逆を持たない。同値は自己逆。
    pub fn inverse(self) -> Option<Self> {
        match self {
            MorphismKind::Specialization => Some(MorphismKind::Generalization),
            MorphismKind::Generalization => Some(MorphismKind::Specialization),
            MorphismKind::Equivalence => Some(MorphismKind::Equivalence),
            MorphismKind::Implication => None,
        }
    }
}

/// 誰がこのエッジを置いたか。ヒューリスティックは承認されるまでクエリの商グラフに入らない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeOrigin {
    Manual,
    Heuristic,
}

impl EdgeOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            EdgeOrigin::Manual => "manual",
            EdgeOrigin::Heuristic => "heuristic",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "manual" => EdgeOrigin::Manual,
            "heuristic" => EdgeOrigin::Heuristic,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeStatus {
    Proposed,
    Accepted,
    Rejected,
}

impl EdgeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            EdgeStatus::Proposed => "proposed",
            EdgeStatus::Accepted => "accepted",
            EdgeStatus::Rejected => "rejected",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "proposed" => EdgeStatus::Proposed,
            "accepted" => EdgeStatus::Accepted,
            "rejected" => EdgeStatus::Rejected,
            _ => return None,
        })
    }
}

/// これから保存する射（ID 未採番）。
#[derive(Debug, Clone)]
pub struct NewMorphism {
    pub src: JudgmentId,
    pub dst: JudgmentId,
    pub kind: MorphismKind,
    pub origin: EdgeOrigin,
    pub status: EdgeStatus,
    pub rationale: Option<String>,
    /// フェーズ3で証明項 AST のハッシュを入れる。フェーズ2では常に None。
    pub proof_term_hash: Option<String>,
    /// フェーズ3で依存公理集合のハッシュを入れる。フェーズ2では常に None。
    pub dependency_signature: Option<String>,
}

impl NewMorphism {
    pub fn manual_accepted(
        src: JudgmentId,
        dst: JudgmentId,
        kind: MorphismKind,
        rationale: Option<String>,
    ) -> Self {
        Self {
            src,
            dst,
            kind,
            origin: EdgeOrigin::Manual,
            status: EdgeStatus::Accepted,
            rationale,
            proof_term_hash: None,
            dependency_signature: None,
        }
    }

    pub fn heuristic_proposed(
        src: JudgmentId,
        dst: JudgmentId,
        kind: MorphismKind,
        rationale: String,
    ) -> Self {
        Self {
            src,
            dst,
            kind,
            origin: EdgeOrigin::Heuristic,
            status: EdgeStatus::Proposed,
            rationale: Some(rationale),
            proof_term_hash: None,
            dependency_signature: None,
        }
    }

    /// 同値は無向なので、保存時に id の小さい方を src にする。
    pub fn normalized(mut self) -> Self {
        if self.kind == MorphismKind::Equivalence && self.src.0 > self.dst.0 {
            std::mem::swap(&mut self.src, &mut self.dst);
        }
        self
    }
}

#[derive(Debug, Clone)]
pub struct MorphismRecord {
    pub id: MorphismId,
    pub src: JudgmentId,
    pub dst: JudgmentId,
    pub kind: MorphismKind,
    pub origin: EdgeOrigin,
    pub status: EdgeStatus,
    pub rationale: Option<String>,
    pub proof_term_hash: Option<String>,
    pub dependency_signature: Option<String>,
    pub created_at: i64,
}

/// ヒューリスティックが返す、まだ永続化していない候補。
#[derive(Debug, Clone)]
pub struct MorphismProposal {
    pub src: JudgmentId,
    pub dst: JudgmentId,
    pub kind: MorphismKind,
    pub rationale: String,
    pub confidence: f32,
}

impl MorphismProposal {
    pub fn into_new(self) -> NewMorphism {
        NewMorphism::heuristic_proposed(self.src, self.dst, self.kind, self.rationale).normalized()
    }
}

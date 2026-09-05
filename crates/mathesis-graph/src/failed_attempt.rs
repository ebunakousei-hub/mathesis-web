//! 層4: 戦略・メタ層（Strategy/Meta Layer）のうち、失敗した証明試行の記録。
//!
//! 「成功した証明」だけでなく「どう失敗したか」を構造化して蓄積することで、
//! 将来の証明探索が同じ手詰まりを繰り返さないよう刈り込めるようにする
//! （アーキテクチャ文書: 失敗試行を FailedPath ノードとして保存する。反例が
//! 見つかって命題自体が偽と判明した場合＝FalseTheorem 相当の負の知識も、
//! 独立したノード種別を新設せず `FailurePattern::Counterexample` として同じ
//! 枠組みで扱う——「証明にまだ失敗している」と「反証済み」は同じ
//! 「この目標に手を出すな」という負の知識の一種であり、クエリ面でも
//! `failed_attempts_for_target` 一本で両方拾えたほうが刈り込みに使いやすい）。
//!
//! 既存の判断・成功証明を書き換えることはなく追記のみ。同じ手詰まりが複数回
//! 記録されること自体が「よくある失敗パターン」のシグナルになる。

use crate::model::JudgmentId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct FailedAttemptId(pub i64);

/// 失敗のしかたを構造化するための分類。`Other` 以外は将来の刈り込み・分析で
/// 直接クエリできることを意図している。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailurePattern {
    /// 帰納法の基底段階で崩れた
    InductionBaseCase,
    /// 帰納法の帰納段階で崩れた（何段階目かは `detail` に記す）
    InductionStep,
    /// 反例が見つかり、命題そのものが偽と判明した（FalseTheorem 相当の負の知識）
    Counterexample,
    /// 適用可能な戦略を使い果たした（手詰まり）
    StrategyExhausted,
    /// 計算資源・時間の限界に達した
    ResourceExhausted,
    /// 前提コンテキスト Γ が不十分、または過剰で矛盾していた
    ContextMismatch,
    /// 上記に当てはまらないその他のパターン（`detail` に自由記述する）
    Other,
}

impl FailurePattern {
    pub fn as_str(self) -> &'static str {
        match self {
            FailurePattern::InductionBaseCase => "induction_base_case",
            FailurePattern::InductionStep => "induction_step",
            FailurePattern::Counterexample => "counterexample",
            FailurePattern::StrategyExhausted => "strategy_exhausted",
            FailurePattern::ResourceExhausted => "resource_exhausted",
            FailurePattern::ContextMismatch => "context_mismatch",
            FailurePattern::Other => "other",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "induction_base_case" => FailurePattern::InductionBaseCase,
            "induction_step" => FailurePattern::InductionStep,
            "counterexample" => FailurePattern::Counterexample,
            "strategy_exhausted" => FailurePattern::StrategyExhausted,
            "resource_exhausted" => FailurePattern::ResourceExhausted,
            "context_mismatch" => FailurePattern::ContextMismatch,
            "other" => FailurePattern::Other,
            _ => return None,
        })
    }

    /// 命題そのものが偽と判明した（FalseTheorem 相当）かどうか。
    pub fn refutes_target(self) -> bool {
        matches!(self, FailurePattern::Counterexample)
    }
}

/// これから保存する失敗試行（ID 未採番）。
#[derive(Debug, Clone)]
pub struct NewFailedAttempt {
    /// 何を証明しようとしていたか。既存の（`Conjecture` 等の）判断ノードを
    /// 指す場合はここに入れる。まだ形式化されていない探索的な試みの場合は
    /// `None` のままでよい（`goal_text` に頼る）。
    pub target: Option<JudgmentId>,
    /// 目標の自由記述（`target` があってもトレーサビリティのため常に残す）。
    pub goal_text: String,
    pub pattern: FailurePattern,
    /// パターンの補足説明（`Other` の中身や、帰納法が何段階目で詰まったか等）。
    pub detail: Option<String>,
    /// 失敗した試行の証明項・タクティクスクリプトを層3の `ProofTerm` として
    /// 事前にインターンしたハッシュ（あれば）。同じ詰まり方の再検出に使う。
    pub proof_term_hash: Option<String>,
    pub source_file: Option<String>,
    pub source_line: Option<u32>,
}

impl NewFailedAttempt {
    pub fn new(goal_text: impl Into<String>, pattern: FailurePattern) -> Self {
        Self {
            target: None,
            goal_text: goal_text.into(),
            pattern,
            detail: None,
            proof_term_hash: None,
            source_file: None,
            source_line: None,
        }
    }

    pub fn with_target(mut self, target: JudgmentId) -> Self {
        self.target = Some(target);
        self
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

#[derive(Debug, Clone)]
pub struct FailedAttemptRecord {
    pub id: FailedAttemptId,
    pub target: Option<JudgmentId>,
    pub goal_text: String,
    pub pattern: FailurePattern,
    pub detail: Option<String>,
    pub proof_term_hash: Option<String>,
    pub source_file: Option<String>,
    pub source_line: Option<u32>,
    pub created_at: i64,
}

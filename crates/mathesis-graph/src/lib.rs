//! mathesis-graph — 層2: 判断・命題層、層3: 射・依存関係層、層4: 戦略・メタ層
//!
//! 層1（mathesis-ast）が作る式ノードを指し示す判断ノード `Judgment(Γ ⊢ P)` を
//! 定義し、SQLite 上に永続化する。フェーズ2で判断間の射（含意・特殊化・一般化・同値）を
//! 型付きエッジとして載せ、フェーズ3で証明項の埋め込みと依存関係静的解析
//! （`dependency_signature`）を実装した。フェーズ4では層3の射に「どの戦略
//! （帰納法・背理法・対角線論法など）で導かれたか」をタグ付けし（`StrategyRecord`）、
//! 失敗した証明試行を `FailedAttemptRecord` として構造化して蓄積する
//! （反例が見つかり命題が偽と判明した場合＝FalseTheorem 相当の負の知識も
//! `FailurePattern::Counterexample` として同じ枠組みで扱う）。フェーズ5では
//! 層3のエッジ合成ルール（[`inference::compose`]）と、それを使った最短の
//! 導出パス探索（[`inference::shortest_derivation`]）を実装する。同値エッジは
//! 層2/3の `quotient_graph()` が既に代表元へ縮約しているため、層5はそれを
//! そのまま使うだけで同値類のクエリ時マージを満たす。
//!
//! （上記の「フェーズ2〜5」は本クレートの層1〜5実装順を指す、最初期の設計
//! 由来の番号——アーキテクチャ.txt 5.8のarXiv概念タクソノミー・ロードマップ
//! の「Phase 9/10」とは別の数え方なので混同しないこと。）Phase 9で
//! `judgment_dependencies`（`morphisms`とは別の、証明本体が参照する判断への
//! 依存）と`papers`（arXiv論文への由来リンク）を追加し、taxonomyエンジン側
//! （`mathesis-taxonomy`/`mathesis-fulltext`）と合流させた。Phase 10では
//! `export::build_export`でこのグラフ全体をWeb版Explorer用の静的JSONへ
//! 書き出す。

pub mod edges;
pub mod error;
pub mod export;
pub mod failed_attempt;
pub mod heuristics;
pub mod inference;
pub mod judgment_dependency;
pub mod layer4;
pub mod model;
pub mod morphism;
pub mod paper;
pub mod paper_citation;
pub mod proof;
pub mod quotient;
pub mod store;
pub mod strategy;

pub use error::{GraphError, GraphResult, ValidationError};
pub use export::{build_export, ExportedDependency, ExportedJudgment, ExportedPaper, GraphExport};
pub use failed_attempt::{
    FailedAttemptId, FailedAttemptRecord, FailurePattern, NewFailedAttempt,
};
pub use heuristics::{propose as propose_heuristic_morphisms, JudgmentLite, JudgmentLiteCache};
pub use inference::{compose, is_reachable, shortest_derivation, InferredPath, PathHop};
pub use model::{
    ExprId, Hypothesis, JudgmentId, JudgmentKind, JudgmentRecord, NewJudgment, ParseStatus,
    SourceRef,
};
pub use morphism::{
    EdgeOrigin, EdgeStatus, MorphismId, MorphismKind, MorphismProposal, MorphismRecord, NewMorphism,
};
pub use paper::{NewPaper, PaperId, PaperRecord};
pub use proof::{analyze_dependencies, DependencyAnalysis, ProofTerm};
pub use quotient::{CollapsedMorphism, QuotientGraph};
pub use store::{intern_hypotheses, GraphStore, ValidationReport};
pub use strategy::{well_known as strategy_names, StrategyId, StrategyRecord};

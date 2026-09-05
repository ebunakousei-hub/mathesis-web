//! title+abstractからの候補概念フレーズ抽出（アーキテクチャ.txt 5.4）。
//! Phase 2: RAKEによる統計的抽出とMSC2020への軽いgrounding。
//! Phase 3: Ollamaローカルembeddingの生成・保存・コサイン類似度検索。
//! Phase 4: embedding類似度+共起+MSC関連度で重み付けした概念グラフを構築し、
//! Label Propagationでクラスタリングする。
//! Phase 5: クラスタ単位でMSC2020とのalignmentを評価し、確信度の高い
//! クラスタから未申告メンバーへコードを伝播、MSC対応の無いクラスタを
//! 新語彙候補として検出する。
//! Phase 6: パイプライン出力をWeb版Explorerが読める静的JSONへ書き出す。
//! Phase 7: exact / same concept / specialization / related の4段階で
//! 概念候補を検索するhybrid search。
//! Phase 8（2026-09-04、試行のうえ見送り）: `relations.rs`のHearstヒットを
//! Ollama経由の小型LLM（qwen2.5:3b/7b-instruct）で文単位検証する案を
//! 試したが、実測の結果いずれも安定して機能せず不採用——詳細は
//! `relations.rs`冒頭と`アーキテクチャ.txt`参照。

pub mod alignment;
pub mod ann;
pub mod concentration;
pub mod concepts;
pub mod context;
pub mod embed;
pub mod export;
pub mod graph;
pub mod llm_judge;
pub mod louvain;
pub mod lpa;
pub mod rake;
pub mod relations;
pub mod resolve;
pub mod search;
pub mod store;

//! Phase 8（アーキテクチャ.txt 5.8）: full text・theorem・proof dependency
//! までの拡張。arXivのLaTeXソースを取得し（Phase 1〜7はtitle/abstractのみ）、
//! 定理系環境・証明・（同一論文内の）証明依存関係・文献引用を抽出する。

pub mod bridge;
pub mod citation;
pub mod macroexpand;
pub mod source;
pub mod store;
pub mod theorem;

//! mathesis-ast — 層1: 構文正規化層（Syntactic Normalization Layer）
//!
//! 数式を構造化された AST として表現し、束縛変数の α変換を de Bruijn 変換
//! 経由で正規化した上でハッシュ値（Canonical Hash）を計算する。

pub mod ast;
pub mod hash;
pub mod lexer;
pub mod parser;

pub use ast::{BinderKind, Binding, Expr, VarId};
pub use lexer::{lex, SpannedTok, Tok};
pub use parser::{parse_expr, ParseOutcome, ParseStatus, Parser};

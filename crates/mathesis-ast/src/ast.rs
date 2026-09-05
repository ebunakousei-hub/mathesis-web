//! 層1: 構文正規化層 — 数式の抽象構文木（AST）。
//!
//! `Var` はレキシカルスコープ解決済みの束縛変数参照、`Const` は未解決の自由識別子
//! （定数・定義済みシンボル）を表す。中置演算子は全て `App(Const(op), [lhs, rhs])`
//! の形へ脱糖して格納するため、正規化・ハッシュ計算の経路は一本化されている。

use serde::{Deserialize, Serialize};
use std::fmt;

/// 束縛変数のスコープ付き一意 ID。同名変数のシャドーイングを区別するために
/// パース時に単調増加のカウンタから割り当てられる。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct VarId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinderKind {
    /// ∀ / Π 型（依存関数型）。無名変数を使えば非依存の A → B もこの形で表す。
    Pi,
    /// λ / fun（関数抽象）
    Lambda,
    /// ∃（存在量化）
    Exists,
}

impl fmt::Display for BinderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BinderKind::Pi => write!(f, "∀"),
            BinderKind::Lambda => write!(f, "λ"),
            BinderKind::Exists => write!(f, "∃"),
        }
    }
}

/// 束縛変数一つ分の宣言（変数 : 型）。`ty` が `None` なのは型注釈が省略され、
/// 文脈から復元できなかった稀なケースのみ。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub var: VarId,
    /// 表示用の元の識別子名（正規化・ハッシュ計算には使わない）
    pub hint: String,
    pub ty: Option<Box<Expr>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Expr {
    /// レキシカルスコープ内で解決された束縛変数の使用箇所
    Var(VarId, String),
    /// 自由識別子（定数・関数記号・型名など）。ドット区切り名も文字列として保持
    Const(String),
    /// 数値・その他リテラル（正規化済み文字列表現）
    Lit(String),
    /// 関数適用 f a b c
    App(Box<Expr>, Vec<Expr>),
    /// 束縛子（∀ / λ / ∃）とその本体
    Bind(BinderKind, Vec<Binding>, Box<Expr>),
    /// パース器がこれ以上構造化できなかった残余テキスト（システム境界のフォールバック）
    Unparsed(String),
}

impl Expr {
    pub fn app(f: Expr, args: Vec<Expr>) -> Expr {
        if args.is_empty() {
            f
        } else {
            Expr::App(Box::new(f), args)
        }
    }

    pub fn binop(op: &str, lhs: Expr, rhs: Expr) -> Expr {
        Expr::App(Box::new(Expr::Const(op.to_string())), vec![lhs, rhs])
    }

    pub fn unop(op: &str, e: Expr) -> Expr {
        Expr::App(Box::new(Expr::Const(op.to_string())), vec![e])
    }

    /// 式中に現れる `Var` の `VarId` を（束縛・自由を問わず）全て集める。
    /// インポーターが「周辺の `variable` 宣言のうちどれが実際に使われたか」を
    /// 判定するために使う。
    pub fn collect_var_ids(&self, out: &mut std::collections::BTreeSet<u32>) {
        match self {
            Expr::Var(id, _) => {
                out.insert(id.0);
            }
            Expr::Const(_) | Expr::Lit(_) | Expr::Unparsed(_) => {}
            Expr::App(f, args) => {
                f.collect_var_ids(out);
                for a in args {
                    a.collect_var_ids(out);
                }
            }
            Expr::Bind(_, bindings, body) => {
                for b in bindings {
                    if let Some(ty) = &b.ty {
                        ty.collect_var_ids(out);
                    }
                }
                body.collect_var_ids(out);
            }
        }
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Var(_, name) => write!(f, "{name}"),
            Expr::Const(name) => write!(f, "{name}"),
            Expr::Lit(s) => write!(f, "{s}"),
            Expr::App(func, args) => {
                // 既知の二項演算子は中置表示に戻す（可読性のため）
                if let (Expr::Const(op), [lhs, rhs]) = (func.as_ref(), args.as_slice()) {
                    if is_infix_symbol(op) {
                        return write!(f, "({lhs} {op} {rhs})");
                    }
                }
                if let (Expr::Const(op), [x]) = (func.as_ref(), args.as_slice()) {
                    if op == "¬" || op == "-" {
                        return write!(f, "{op}{x}");
                    }
                }
                write!(f, "{func}")?;
                for a in args {
                    // 引数自身が（空でない）関数適用や束縛子式の場合は丸括弧で囲む。
                    // でないと `f (g x)` と `f g x`（= App(f,[g,x])、全く別の式）が
                    // 表示上区別できなくなる。
                    if needs_arg_parens(a) {
                        write!(f, " ({a})")?;
                    } else {
                        write!(f, " {a}")?;
                    }
                }
                Ok(())
            }
            Expr::Bind(kind, bindings, body) => {
                write!(f, "{kind} ")?;
                for (i, b) in bindings.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    match &b.ty {
                        Some(ty) => write!(f, "({} : {ty})", b.hint)?,
                        None => write!(f, "{}", b.hint)?,
                    }
                }
                write!(f, ", {body}")
            }
            Expr::Unparsed(t) => write!(f, "«{t}»"),
        }
    }
}

/// 関数適用の引数としてそのまま並べると曖昧になる式（それ自身が空でない
/// 適用、または束縛子式）かどうか。
fn needs_arg_parens(e: &Expr) -> bool {
    matches!(e, Expr::App(_, args) if !args.is_empty()) || matches!(e, Expr::Bind(..))
}

fn is_infix_symbol(op: &str) -> bool {
    matches!(
        op,
        "+" | "-"
            | "*"
            | "/"
            | "^"
            | "="
            | "≠"
            | "<"
            | "≤"
            | ">"
            | "≥"
            | "∈"
            | "∉"
            | "⊆"
            | "⊂"
            | "∧"
            | "∨"
            | "↔"
            | "→"
            | "∘"
            | "×"
            | "•"
    )
}

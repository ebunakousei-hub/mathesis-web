//! Lean4 風の項構文に対する再帰下降（前置演算子は Pratt 法）パーサー。
//!
//! フル言語の仕様を実装するものではなく、定理・定義文の型注釈として現れる
//! 範囲の式（束縛子・中置/前置演算子・関数適用）を対象とする実用的な部分文法。
//! 未対応の構文に出会っても panic せず `Expr::Unparsed` へフォールバックし、
//! 呼び出し側が `ParseStatus` で成否を判別できるようにする（外部ファイルを読む
//! 境界では「落ちないこと」を「厳密さ」より優先する）。

use crate::ast::{BinderKind, Binding, Expr, VarId};
use crate::lexer::{lex, SpannedTok, Tok};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseStatus {
    /// トークン列を最後まで構造化できた
    Full,
    /// 一部を `Unparsed` に退避した、またはトークンが残った
    Partial,
}

pub struct ParseOutcome {
    pub expr: Expr,
    pub status: ParseStatus,
}

/// 中置演算子の ASCII 表記をハッシュ計算上の正準表記へ寄せる
fn normalize_op(op: &str) -> &str {
    match op {
        "->" => "→",
        "<->" => "↔",
        "<=" => "≤",
        ">=" => "≥",
        "!=" => "≠",
        "/\\" => "∧",
        "\\/" => "∨",
        other => other,
    }
}

struct Scope {
    frames: Vec<Vec<(String, VarId)>>,
}

impl Scope {
    fn new() -> Self {
        Scope { frames: vec![Vec::new()] }
    }
    fn push(&mut self) {
        self.frames.push(Vec::new());
    }
    fn pop(&mut self) {
        self.frames.pop();
    }
    fn bind(&mut self, name: &str, id: VarId) {
        self.frames.last_mut().unwrap().push((name.to_string(), id));
    }
    fn resolve(&self, name: &str) -> Option<VarId> {
        for frame in self.frames.iter().rev() {
            for (n, id) in frame.iter().rev() {
                if n == name {
                    return Some(*id);
                }
            }
        }
        None
    }
}

pub struct Parser<'a> {
    toks: &'a [SpannedTok],
    pos: usize,
    next_id: u32,
    scope: Scope,
    partial: bool,
}

const KEYWORDS: &[&str] = &["fun", "forall", "exists"];

impl<'a> Parser<'a> {
    pub fn new(toks: &'a [SpannedTok]) -> Self {
        Parser {
            toks,
            pos: 0,
            next_id: 0,
            scope: Scope::new(),
            partial: false,
        }
    }

    /// 既存のスコープ（変数名→VarId）を引き継いで開始する。宣言の束縛子群を
    /// 解析済みの状態でその文（`:` 以降の型）を解析する際に使う。
    pub fn with_seed_scope(toks: &'a [SpannedTok], seed: Vec<(String, VarId)>, next_id: u32) -> Self {
        let mut scope = Scope::new();
        for (name, id) in seed {
            scope.bind(&name, id);
        }
        Parser {
            toks,
            pos: 0,
            next_id,
            scope,
            partial: false,
        }
    }

    pub fn next_var_counter(&self) -> u32 {
        self.next_id
    }

    pub fn at_eof(&self) -> bool {
        matches!(self.peek(), Tok::Eof)
    }

    pub fn had_partial(&self) -> bool {
        self.partial
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    fn peek(&self) -> &Tok {
        &self.toks[self.pos.min(self.toks.len() - 1)].tok
    }

    fn advance(&mut self) -> Tok {
        let t = self.toks[self.pos.min(self.toks.len() - 1)].tok.clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn expect(&mut self, want: &Tok) {
        if self.peek() == want {
            self.advance();
        } else {
            self.partial = true;
        }
    }

    fn fresh_var(&mut self) -> VarId {
        self.next_id += 1;
        VarId(self.next_id)
    }

    // ---- 束縛子リスト -------------------------------------------------

    /// `(a b : T) {x : U} [Inst]` のような、宣言直後に並ぶ束縛子群を
    /// カッコが尽きるまで読み進める。呼び出し側は `:` に達するまで繰り返す。
    pub fn parse_binder_group_list(&mut self) -> Vec<Binding> {
        let mut result = Vec::new();
        while matches!(self.peek(), Tok::LParen | Tok::LBrace | Tok::LBracket) {
            result.extend(self.parse_one_group());
        }
        result
    }

    /// `∀`/`∃`/`λ` 直後の「裸の名前列 (: 型)?」または括弧束縛子群を読む。
    fn parse_binder_list(&mut self) -> Vec<Binding> {
        if matches!(self.peek(), Tok::LParen | Tok::LBrace | Tok::LBracket) {
            let mut result = Vec::new();
            while matches!(self.peek(), Tok::LParen | Tok::LBrace | Tok::LBracket) {
                result.extend(self.parse_one_group());
            }
            return result;
        }
        let mut names = Vec::new();
        while let Tok::Ident(name) = self.peek().clone() {
            if KEYWORDS.contains(&name.as_str()) {
                break;
            }
            names.push(name);
            self.advance();
        }
        let ty = if matches!(self.peek(), Tok::Colon) {
            self.advance();
            Some(Box::new(self.parse_bp(0)))
        } else {
            None
        };
        let mut result = Vec::new();
        for name in names {
            let id = self.fresh_var();
            self.scope.bind(&name, id);
            result.push(Binding {
                var: id,
                hint: name,
                ty: ty.clone(),
            });
        }
        result
    }

    fn scan_has_top_level_colon_before(&self, closer: &Tok) -> bool {
        let mut depth = 0i32;
        let mut i = self.pos;
        loop {
            let t = &self.toks[i.min(self.toks.len() - 1)].tok;
            if matches!(t, Tok::Eof) {
                return false;
            }
            if depth == 0 && t == closer {
                return false;
            }
            match t {
                Tok::LParen | Tok::LBrace | Tok::LBracket => depth += 1,
                Tok::RParen | Tok::RBrace | Tok::RBracket => depth -= 1,
                Tok::Colon if depth == 0 => return true,
                _ => {}
            }
            i += 1;
        }
    }

    fn parse_one_group(&mut self) -> Vec<Binding> {
        let closer = match self.peek() {
            Tok::LParen => Tok::RParen,
            Tok::LBrace => Tok::RBrace,
            Tok::LBracket => Tok::RBracket,
            _ => return Vec::new(),
        };
        self.advance();

        if !self.scan_has_top_level_colon_before(&closer) {
            // 名前のない束縛（主に匿名 instance 束縛 `[Monoid M]`）
            let ty = self.parse_bp(0);
            self.expect(&closer);
            let id = self.fresh_var();
            let hint = format!("inst{}", id.0);
            self.scope.bind(&hint, id);
            return vec![Binding {
                var: id,
                hint,
                ty: Some(Box::new(ty)),
            }];
        }

        let mut names = Vec::new();
        while let Tok::Ident(name) = self.peek().clone() {
            names.push(name);
            self.advance();
        }
        self.expect(&Tok::Colon);
        let ty = self.parse_bp(0);
        self.expect(&closer);

        let mut result = Vec::new();
        for name in names {
            let id = self.fresh_var();
            self.scope.bind(&name, id);
            result.push(Binding {
                var: id,
                hint: name,
                ty: Some(Box::new(ty.clone())),
            });
        }
        result
    }

    pub fn expect_colon(&mut self) {
        self.expect(&Tok::Colon);
    }

    // ---- 式（Pratt 法による優先順位パース） ----------------------------

    pub fn parse_bp(&mut self, min_bp: u8) -> Expr {
        let mut lhs = self.parse_prefix();
        loop {
            let (op, l_bp, r_bp) = match self.peek_infix() {
                Some(x) => x,
                None => break,
            };
            if l_bp < min_bp {
                break;
            }
            self.advance();
            let rhs = self.parse_bp(r_bp);
            lhs = Expr::binop(normalize_op(&op), lhs, rhs);
        }
        lhs
    }

    fn peek_infix(&self) -> Option<(String, u8, u8)> {
        let s = match self.peek() {
            Tok::Symbol(s) => s.clone(),
            _ => return None,
        };
        let bp = match s.as_str() {
            "↔" | "<->" => (10u8, 10u8),
            "→" | "->" => (20, 20),
            "∨" | "\\/" => (30, 31),
            "∧" | "/\\" => (40, 41),
            "=" | "≠" | "!=" | "<" | "≤" | "<=" | ">" | "≥" | ">=" | "∈" | "∉" | "⊆" | "⊂"
            | "⊊" | "⊇" | "≡" | "≈" => (50, 51),
            "+" | "-" => (60, 61),
            "*" | "/" | "∘" | "×" | "•" => (70, 71),
            "^" => (80, 80),
            _ => return None,
        };
        Some((s, bp.0, bp.1))
    }

    fn parse_prefix(&mut self) -> Expr {
        match self.peek().clone() {
            Tok::Symbol(s) if s == "¬" => {
                self.advance();
                Expr::unop("¬", self.parse_bp(45))
            }
            Tok::Symbol(s) if s == "-" => {
                self.advance();
                Expr::unop("-", self.parse_bp(65))
            }
            Tok::Symbol(s) if s == "∀" => {
                self.advance();
                self.parse_binder_expr(BinderKind::Pi)
            }
            Tok::Symbol(s) if s == "∃" => {
                self.advance();
                self.parse_binder_expr(BinderKind::Exists)
            }
            Tok::Symbol(s) if s == "λ" => {
                self.advance();
                self.parse_binder_expr(BinderKind::Lambda)
            }
            Tok::Ident(s) if s == "fun" => {
                self.advance();
                self.parse_binder_expr(BinderKind::Lambda)
            }
            Tok::Ident(s) if s == "forall" => {
                self.advance();
                self.parse_binder_expr(BinderKind::Pi)
            }
            Tok::Ident(s) if s == "exists" => {
                self.advance();
                self.parse_binder_expr(BinderKind::Exists)
            }
            _ => self.parse_application(),
        }
    }

    fn parse_binder_expr(&mut self, kind: BinderKind) -> Expr {
        self.scope.push();
        let bindings = self.parse_binder_list();
        if matches!(self.peek(), Tok::Comma) {
            self.advance();
        } else if matches!(self.peek(), Tok::Symbol(s) if s == "=>" || s == "↦") {
            self.advance();
        }
        let body = self.parse_bp(0);
        self.scope.pop();
        if bindings.is_empty() {
            self.partial = true;
            return body;
        }
        Expr::Bind(kind, bindings, Box::new(body))
    }

    fn at_atom_start(&self) -> bool {
        matches!(self.peek(), Tok::Ident(_) | Tok::Number(_) | Tok::LParen)
    }

    fn parse_application(&mut self) -> Expr {
        let first = self.parse_atom();
        let mut args = Vec::new();
        while self.at_atom_start() {
            args.push(self.parse_atom());
        }
        Expr::app(first, args)
    }

    fn parse_atom(&mut self) -> Expr {
        match self.peek().clone() {
            Tok::Ident(name) => {
                self.advance();
                if KEYWORDS.contains(&name.as_str()) {
                    self.partial = true;
                    return Expr::Unparsed(name);
                }
                match self.scope.resolve(&name) {
                    Some(id) => Expr::Var(id, name),
                    None => Expr::Const(name),
                }
            }
            Tok::Number(n) => {
                self.advance();
                Expr::Lit(n)
            }
            Tok::LParen => {
                self.advance();
                let e = self.parse_bp(0);
                self.expect(&Tok::RParen);
                e
            }
            other => {
                if !matches!(other, Tok::Eof) {
                    self.advance();
                }
                self.partial = true;
                Expr::Unparsed(format!("{other:?}"))
            }
        }
    }
}

/// 単体の式（束縛子なしの外部文脈から始まる式）を手軽にパースするための入口。
/// 主にテスト・簡易用途向け。宣言の束縛子込みの解析は `Parser` を直接使う。
pub fn parse_expr(src: &str) -> Result<ParseOutcome, String> {
    let toks = lex(src);
    let mut p = Parser::new(&toks);
    let e = p.parse_bp(0);
    let status = if p.at_eof() && !p.had_partial() {
        ParseStatus::Full
    } else {
        ParseStatus::Partial
    };
    Ok(ParseOutcome { expr: e, status })
}

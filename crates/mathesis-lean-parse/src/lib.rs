//! Lean 4 の宣言（`theorem`/`lemma`/`def`/`axiom`/`instance`/`example`）を
//! 判断ノードへ切り出す、純粋なテキスト処理。
//!
//! これは元々 `mathesis-importer`（CLIバイナリ）の `main.rs` に直接
//! 書かれていたロジックの抽出である。切り出した理由は1つ——
//! **ブラウザで動かすため**。`mathesis-wasm` は Lean のソースをその場で
//! パースして見せたいが、`mathesis-importer` は挿入のたびに
//! `mathesis-graph::GraphStore`（rusqlite、バンドルされたC製SQLite）を
//! 経由しており、これは `wasm32-unknown-unknown` へコンパイルできない
//! （C コンパイラを要求するため、`mathesis-wasm/src/lib.rs` の冒頭コメント
//! で既に確認済みの制約）。Rustはクレート単位でしか依存を切れないので、
//! `mathesis_graph::JudgmentKind` を1個importするだけでも、Cargoは
//! `mathesis-graph` クレート全体（＝rusqliteも）をそのターゲット向けに
//! ビルドしようとしてしまう。
//!
//! そこでLeanのテキストを判断ノードへ切り出す部分——構文の分割・識別子と
//! 文脈の抽出・依存関係の名前解決——を、`mathesis-graph` は一切importしない
//! このクレートへ移した。`mathesis-importer` と `mathesis-wasm` の両方が
//! ここへ依存する。**アルゴリズムは1箇所にしかない**——CLIとブラウザで
//! 別々の実装を持つと、いずれ挙動がずれて「CLIでは拾えるのにブラウザでは
//! 拾えない」ような食い違いが起きる。
//!
//! 依存するのは `mathesis-ast`（式のパース、これ自体が既にwasm32で動作
//! 確認済み）と `anyhow`（純Rust、C依存なし）のみ。

use anyhow::{anyhow, Result};
use mathesis_ast::{parse_expr, Expr, ParseStatus as AstParseStatus};
use std::collections::HashSet;

/// 判断の種別。`mathesis_graph::JudgmentKind` の部分集合——Leanの構文解析器
/// が実際に生成しうる種別だけを持つ（`Lemma`・`Conjecture` は
/// `mathesis-fulltext` 側のarXiv全文抽出でしか使われないので、ここには
/// 無い。`theorem`と`lemma`キーワードはどちらも`Theorem`に落ちる——CLI版の
/// 従来の挙動どおり）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParsedKind {
    Theorem,
    Definition,
    Axiom,
    Instance,
    Example,
}

impl ParsedKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ParsedKind::Theorem => "theorem",
            ParsedKind::Definition => "definition",
            ParsedKind::Axiom => "axiom",
            ParsedKind::Instance => "instance",
            ParsedKind::Example => "example",
        }
    }
}

/// 命題（またはInstanceの型クラス、Exampleのゴール）を式としてパースできた
/// 度合い。`mathesis_ast::ParseStatus` に「式として全く解釈できなかった」
/// という第3の状態 `Failed` を足したもの——元の `intern_statement` が
/// `Result::Err` を握りつぶしてフォールバック式に差し替えていた挙動を、
/// 状態として持ち上げただけ（ふるまいは変えていない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseOutcome {
    Full,
    Partial,
    Failed,
}

impl ParseOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            ParseOutcome::Full => "full",
            ParseOutcome::Partial => "partial",
            ParseOutcome::Failed => "failed",
        }
    }
}

/// Leanの1宣言から切り出した判断ノード。
#[derive(Debug, Clone)]
pub struct ParsedJudgment {
    pub kind: ParsedKind,
    /// `example` は無名なので `None`。`instance` も省略可能なので `None` になりうる。
    pub name: Option<String>,
    /// 束縛子（`(x : ℕ)` 等）を `(名前, 型の式)` に割ったもの。
    pub context: Vec<(String, Expr)>,
    /// 命題（Instanceなら型クラス、Exampleならゴール）の生のテキスト。
    pub statement_text: String,
    /// `statement_text` をパースした式。パースに失敗した場合は `_`（プレースホルダ）。
    pub statement_expr: Expr,
    pub statement_status: ParseOutcome,
    /// `definition` のときだけ、証明/定義本体の生テキスト。
    pub definition_body_raw: Option<String>,
    /// シグネチャ＋本体を含む、宣言全体の生テキスト（依存関係の名前解決に使う）。
    pub raw_text: String,
    /// 1-indexed の開始行。
    pub line: u32,
}

/// `source` からLeanの宣言を全て切り出し、判断ノードへパースする。
///
/// 失敗した宣言は無視して続ける（1個の壊れた宣言のせいでファイル全体を
/// 諦めない、CLI版の従来の挙動を踏襲）。式としてパースできなかった命題は
/// 除外するのではなく `ParseOutcome::Failed` として残す——「この宣言は
/// 読み取れなかった」という事実自体が、貼り付けた側への有用な情報になる。
pub fn parse_lean_source(source: &str) -> Vec<ParsedJudgment> {
    parse_statements(source)
        .into_iter()
        .map(|stmt| match stmt {
            LeanStatement::Theorem { name, context, statement, raw_text, line } => {
                let (statement_expr, statement_status) = intern_statement(&statement);
                ParsedJudgment {
                    kind: ParsedKind::Theorem,
                    name: Some(name),
                    context,
                    statement_text: statement,
                    statement_expr,
                    statement_status,
                    definition_body_raw: None,
                    raw_text,
                    line,
                }
            }
            LeanStatement::Definition { name, context, statement, body, raw_text, line } => {
                let (statement_expr, statement_status) = intern_statement(&statement);
                ParsedJudgment {
                    kind: ParsedKind::Definition,
                    name: Some(name),
                    context,
                    statement_text: statement,
                    statement_expr,
                    statement_status,
                    definition_body_raw: Some(body),
                    raw_text,
                    line,
                }
            }
            LeanStatement::Axiom { name, context, statement, raw_text, line } => {
                let (statement_expr, statement_status) = intern_statement(&statement);
                ParsedJudgment {
                    kind: ParsedKind::Axiom,
                    name: Some(name),
                    context,
                    statement_text: statement,
                    statement_expr,
                    statement_status,
                    definition_body_raw: None,
                    raw_text,
                    line,
                }
            }
            LeanStatement::Instance { name, context, type_class, raw_text, line } => {
                let (statement_expr, statement_status) = intern_statement(&type_class);
                ParsedJudgment {
                    kind: ParsedKind::Instance,
                    name,
                    context,
                    statement_text: type_class,
                    statement_expr,
                    statement_status,
                    definition_body_raw: None,
                    raw_text,
                    line,
                }
            }
            LeanStatement::Example { context, statement, raw_text, line } => {
                let (statement_expr, statement_status) = intern_statement(&statement);
                ParsedJudgment {
                    kind: ParsedKind::Example,
                    name: None,
                    context,
                    statement_text: statement,
                    statement_expr,
                    statement_status,
                    definition_body_raw: None,
                    raw_text,
                    line,
                }
            }
        })
        .collect()
}

/// 命題文字列を式としてパースする。失敗したらプレースホルダ式 `_` に
/// 差し替えて `ParseOutcome::Failed` を返す——呼び出し側（DB挿入・wasm出力
/// どちらも）が `Result` を分岐せずに扱えるようにするため。
fn intern_statement(stmt: &str) -> (Expr, ParseOutcome) {
    match parse_expr(stmt) {
        Ok(outcome) => {
            let status = match outcome.status {
                AstParseStatus::Full => ParseOutcome::Full,
                AstParseStatus::Partial => ParseOutcome::Partial,
            };
            (outcome.expr, status)
        }
        Err(_) => {
            let fallback = parse_expr("_").unwrap_or_else(|_| panic!("'_' must parse")).expr;
            (fallback, ParseOutcome::Failed)
        }
    }
}

// ── 依存関係の名前解決（Phase 9） ──────────────────────────────────

fn is_lean_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '\''
}

/// Leanのコメント（`--`から行末、`/- ... -/`のブロックコメント——`/-!`や
/// `/--`のドキュメンテーションコメントも同じ形なので併せて剥がれる）を
/// 取り除く。ブロックコメントの入れ子（`/- /- -/ -/`）を許すため、深さを
/// 数えて対応する`-/`まで読み飛ばす。
///
/// 実データ（DeGiorgiコーパスの`Supersolutions/TestFunctions.lean`）で、
/// モジュールdocコメントが他の判断名を地の文で列挙しているのを確認した。
/// コメントを剥がさずに参照抽出すると、これを実際の証明上の依存である
/// かのように誤検出してしまう。
pub fn strip_lean_comments(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '-' && chars.get(i + 1) == Some(&'-') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if chars[i] == '/' && chars.get(i + 1) == Some(&'-') {
            let mut depth = 1;
            i += 2;
            while i < chars.len() && depth > 0 {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'-') {
                    depth += 1;
                    i += 2;
                } else if chars[i] == '-' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// `text`（コメント除去済み）中の識別子トークンのうち、`known_names`に
/// 完全一致するものを、出現順・重複除去して返す。
pub fn find_referenced_names(text: &str, known_names: &HashSet<&str>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut current = String::new();

    for c in text.chars().chain(std::iter::once(' ')) {
        if is_lean_ident_char(c) {
            current.push(c);
        } else if !current.is_empty() {
            if known_names.contains(current.as_str()) && seen.insert(current.clone()) {
                out.push(current.clone());
            }
            current.clear();
        }
    }
    out
}

/// 1回のパースで得た判断ノード群を突き合わせ、証明/定義本体が同じバッチ
/// 内の別の判断名を参照している箇所を検出する。`(from, to)` を
/// `judgments` のインデックスの組で返す——このクレートはDBの採番IDを
/// 知らないため、呼び出し側（CLIならDBのID、ブラウザなら配列の添字）が
/// このインデックスを実際のIDへ写す。
///
/// `mathesis-importer::dependencies::record_dependencies` のロジックを
/// GraphStoreに触れない形に一般化したもの——名前解決の規則自体
/// （コメント除去→既知名との完全一致→自己参照を除外）は完全に同じ。
pub fn find_dependencies(judgments: &[ParsedJudgment]) -> Vec<(usize, usize)> {
    let name_to_index: std::collections::HashMap<&str, usize> = judgments
        .iter()
        .enumerate()
        .filter_map(|(i, j)| j.name.as_deref().map(|n| (n, i)))
        .collect();
    let known_names: HashSet<&str> = name_to_index.keys().copied().collect();

    let mut edges = Vec::new();
    for (i, j) in judgments.iter().enumerate() {
        let cleaned = strip_lean_comments(&j.raw_text);
        for referenced in find_referenced_names(&cleaned, &known_names) {
            if Some(referenced.as_str()) == j.name.as_deref() {
                continue; // 自己参照（再帰定義等）は依存として数えない
            }
            if let Some(&target) = name_to_index.get(referenced.as_str()) {
                edges.push((i, target));
            }
        }
    }
    edges
}

// ── Lean構文の分割（純テキスト処理） ────────────────────────────────

const DECL_KEYWORDS: &[&str] = &["theorem", "lemma", "def", "axiom", "instance", "example"];

#[derive(Debug)]
enum LeanStatement {
    Theorem {
        name: String,
        context: Vec<(String, Expr)>,
        statement: String,
        raw_text: String,
        line: u32,
    },
    Definition {
        name: String,
        context: Vec<(String, Expr)>,
        statement: String,
        body: String,
        raw_text: String,
        line: u32,
    },
    Axiom {
        name: String,
        context: Vec<(String, Expr)>,
        statement: String,
        raw_text: String,
        line: u32,
    },
    Instance {
        name: Option<String>,
        context: Vec<(String, Expr)>,
        type_class: String,
        raw_text: String,
        line: u32,
    },
    /// `example (binders) : P := proof`。Lean の `example` は本来無名。
    Example {
        context: Vec<(String, Expr)>,
        statement: String,
        raw_text: String,
        line: u32,
    },
}

fn parse_statements(content: &str) -> Vec<LeanStatement> {
    let mut statements = Vec::new();

    // ファイルを「宣言ブロック」に分割する。
    // 宣言開始キーワードで始まる行が新しいブロックの開始。
    // `noncomputable`/`private`/`@[simp]` 等の修飾子が前置されていても検出できる
    // よう、単純な `starts_with` ではなく `strip_to_after_keyword` を使う。
    let lines: Vec<(usize, &str)> = content.lines().enumerate().collect();

    let mut decl_starts: Vec<usize> = Vec::new();
    for (idx, line) in &lines {
        let t = line.trim();
        if t.starts_with("--") || t.is_empty() {
            continue;
        }
        if DECL_KEYWORDS.iter().any(|kw| strip_to_after_keyword(t, kw).is_some()) {
            decl_starts.push(*idx);
        }
    }

    for (i, &start_idx) in decl_starts.iter().enumerate() {
        let end_idx = if i + 1 < decl_starts.len() { decl_starts[i + 1] } else { lines.len() };

        let block_lines: Vec<&str> = lines[start_idx..end_idx].iter().map(|(_, l)| *l).collect();
        let block_text: String = block_lines.join("\n");
        let trimmed_first = block_lines[0].trim();
        let line_no = (start_idx + 1) as u32; // 1-indexed

        let kind_opt = DECL_KEYWORDS.iter().find_map(|&kw| {
            strip_to_after_keyword(trimmed_first, kw).map(|_| kw)
        });
        let Some(matched_kw) = kind_opt else { continue };

        let sig_and_body = extract_signature_and_body(&block_text);
        let sig = sig_and_body.0.trim();

        match matched_kw {
            "instance" => {
                let Some(after_kw) = strip_to_after_keyword(sig, "instance") else { continue };

                let (name, type_class_str) = if let Some(colon_pos) = find_top_level_colon(after_kw) {
                    let name_part = after_kw[..colon_pos].trim();
                    let tc_part = after_kw[colon_pos + 1..].trim();
                    let tc_clean = tc_part.split(":=").next().unwrap_or(tc_part).trim();
                    let inst_name = if name_part.is_empty() || name_part.starts_with('[') {
                        None
                    } else {
                        Some(name_part.to_string())
                    };
                    (inst_name, tc_clean.to_string())
                } else {
                    (None, after_kw.to_string())
                };

                statements.push(LeanStatement::Instance {
                    name,
                    context: Vec::new(),
                    type_class: type_class_str,
                    raw_text: block_text.clone(),
                    line: line_no,
                });
            }

            "example" => {
                let Some(after_kw) = strip_to_after_keyword(sig, "example") else { continue };
                let Some(colon_pos) = find_top_level_colon(after_kw) else { continue };
                let context_str = after_kw[..colon_pos].trim();
                let after_colon = after_kw[colon_pos + 1..].trim();
                let context = parse_context(context_str).unwrap_or_default();

                let statement = if let Some(assign_pos) = after_colon.find(":=") {
                    after_colon[..assign_pos].trim().to_string()
                } else {
                    after_colon.trim().to_string()
                };

                statements.push(LeanStatement::Example {
                    context,
                    statement,
                    raw_text: block_text.clone(),
                    line: line_no,
                });
            }

            _ => {
                // theorem / lemma / def / axiom
                let keyword = if strip_to_after_keyword(sig, "theorem").is_some() {
                    "theorem"
                } else if strip_to_after_keyword(sig, "lemma").is_some() {
                    "lemma"
                } else if strip_to_after_keyword(sig, "def").is_some() {
                    "def"
                } else if strip_to_after_keyword(sig, "axiom").is_some() {
                    "axiom"
                } else {
                    continue;
                };
                let Some(rest) = strip_to_after_keyword(sig, keyword) else { continue };

                let Ok((name, after_name)) = extract_identifier(rest) else { continue };
                // 束縛子グループ自体がコロンを含みうる（`{σ₁ σ₂ ρ : ℝ}` 等）ため、
                // 深さを考慮したトップレベルのコロンで「束縛子ここまで、型ここから」
                // を区切る。単純な `.find(':')` だと束縛子内部のコロンに引っかかり、
                // 型が丸ごと壊れる。
                let Some(colon_pos) = find_top_level_colon(after_name) else { continue };
                let context_str = after_name[..colon_pos].trim();
                let after_colon = after_name[colon_pos + 1..].trim();

                let context = parse_context(context_str).unwrap_or_default();

                let (statement, body) = if let Some(assign_pos) = after_colon.find(":=") {
                    (
                        after_colon[..assign_pos].trim().to_string(),
                        after_colon[assign_pos + 2..].trim().to_string(),
                    )
                } else {
                    (after_colon.trim().to_string(), String::new())
                };

                let full_body = if sig_and_body.1.is_empty() {
                    body
                } else if body.is_empty() {
                    sig_and_body.1.clone()
                } else {
                    format!("{}\n{}", body, sig_and_body.1)
                };

                match keyword {
                    "theorem" | "lemma" => {
                        statements.push(LeanStatement::Theorem {
                            name,
                            context,
                            statement,
                            raw_text: block_text.clone(),
                            line: line_no,
                        });
                    }
                    "def" => {
                        statements.push(LeanStatement::Definition {
                            name,
                            context,
                            statement,
                            body: full_body,
                            raw_text: block_text.clone(),
                            line: line_no,
                        });
                    }
                    "axiom" => {
                        statements.push(LeanStatement::Axiom {
                            name,
                            context,
                            statement,
                            raw_text: block_text.clone(),
                            line: line_no,
                        });
                    }
                    _ => {}
                }
            }
        }
    }

    statements
}

/// ブロックテキストからシグネチャ（`:=` 前）と本体（`:=` 後）に分割する。
/// マルチラインの場合は最初の行のみシグネチャとして使い、残りを本体とする。
fn extract_signature_and_body(block: &str) -> (String, String) {
    if let Some(pos) = find_assign_pos(block) {
        let sig = block[..pos].to_string();
        let body = block[pos + 2..].trim_start().to_string();
        (sig, body)
    } else {
        (block.to_string(), String::new())
    }
}

/// トップレベル（括弧の外）にある最初の `:=` の位置を探す。
fn find_assign_pos(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut i = 0;
    while i + 1 < bytes.len() {
        match bytes[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b':' if depth == 0 && bytes[i + 1] == b'=' => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

/// トップレベル（括弧の外）にある最初の `:` の位置を探す。`:=` はここでは
/// 数えない（呼び出し側は「型の始まり」を探しているのであって代入では
/// ないため）。
fn find_top_level_colon(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b':' if depth == 0 => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'=' {
                    i += 1;
                } else {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn strip_to_after_keyword<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    let mut s = line.trim_start();
    loop {
        if let Some(rest) = s.strip_prefix(keyword) {
            if rest.is_empty() || rest.starts_with(char::is_whitespace) {
                return Some(rest.trim_start());
            }
        }
        if let Some(rest) = s.strip_prefix("@[") {
            if let Some(end) = rest.find(']') {
                s = rest[end + 1..].trim_start();
                continue;
            }
            return None;
        }
        let mut advanced = false;
        for m in ["noncomputable", "private", "protected", "scoped", "local", "partial", "unsafe"] {
            if let Some(rest) = s.strip_prefix(m) {
                if rest.starts_with(char::is_whitespace) {
                    s = rest.trim_start();
                    advanced = true;
                    break;
                }
            }
        }
        if !advanced {
            return None;
        }
    }
}

/// 実データ（DeGiorgiコーパス`Campanato.lean:104`）で発覚したバグの修正:
/// Leanでは`lemma HasCampanatoBound.nonneg ...`のように、ある定義/述語の
/// 名前空間に属するAPI補題をドット区切りの修飾名で書くのがごく普通の
/// 慣習だが、`.`を識別子文字として認めていなかったため、名前は
/// "HasCampanatoBound"までしか拾えず、残った".nonneg"がそのまま後続の
/// バインダ文字列の先頭に紛れ込み、`parse_context`が最初の束縛子グループの
/// 波括弧と誤って合体させてしまっていた。`.`を識別子文字に含めて修飾名を
/// 丸ごと1つのトークンとして消費することで解決する。
fn extract_identifier(s: &str) -> Result<(String, &str)> {
    for (i, c) in s.char_indices() {
        if !c.is_alphanumeric() && c != '_' && c != '\'' && c != '.' {
            return Ok((s[..i].to_string(), &s[i..]));
        }
    }
    Ok((s.to_string(), ""))
}

fn parse_context(s: &str) -> Result<Vec<(String, Expr)>> {
    if s.is_empty() {
        return Ok(Vec::new());
    }

    let mut context = Vec::new();
    let mut depth = 0i32;
    let mut current_group = String::new();
    let mut bracket_char: Option<char> = None;

    for c in s.chars() {
        match c {
            '(' | '[' | '{' => {
                if depth == 0 {
                    bracket_char = Some(c);
                }
                depth += 1;
                current_group.push(c);
            }
            ')' | ']' | '}' => {
                current_group.push(c);
                depth -= 1;
                if depth == 0 {
                    if let Some(open) = bracket_char {
                        let group = current_group.clone();
                        if let Ok(bindings) = parse_binder_group(&group, open) {
                            for (name, ty) in bindings {
                                context.push((name, ty));
                            }
                        }
                    }
                    current_group.clear();
                    bracket_char = None;
                }
            }
            _ => {
                if depth > 0 || !c.is_whitespace() {
                    current_group.push(c);
                }
            }
        }
    }

    Ok(context)
}

fn parse_binder_group(s: &str, open: char) -> Result<Vec<(String, Expr)>> {
    let close = match open {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        _ => return Ok(Vec::new()),
    };

    let inner = s.trim_start_matches(open).trim_end_matches(close).trim();

    if let Some(colon_pos) = inner.rfind(':') {
        let names_part = &inner[..colon_pos];
        let type_part = inner[colon_pos + 1..].trim();

        // 型クラス仮定 `[Group G]` はインスタンス名として `inst` を使う
        let names: Vec<&str> =
            if open == '[' { vec!["inst"] } else { names_part.split_whitespace().collect() };

        let type_expr =
            parse_expr(type_part).map_err(|e| anyhow!("Failed to parse type '{}': {}", type_part, e))?.expr;

        Ok(names.into_iter().map(|n| (n.to_string(), type_expr.clone())).collect())
    } else {
        // コロンなし → 型クラスの適用 `[Group G]` 全体を型として扱う
        if open == '[' {
            let type_expr =
                parse_expr(inner).map_err(|e| anyhow!("Failed to parse type class '{}': {}", inner, e))?.expr;
            return Ok(vec![("inst".to_string(), type_expr)]);
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_identifier_keeps_dotted_namespace_qualified_names_as_one_token() {
        let (name, rest) = extract_identifier("HasCampanatoBound.nonneg\n    {u : E → ℝ} : True").unwrap();
        assert_eq!(name, "HasCampanatoBound.nonneg");
        assert_eq!(rest.trim_start(), "{u : E → ℝ} : True");
    }

    #[test]
    fn extract_identifier_stops_at_whitespace_for_ordinary_unqualified_names() {
        let (name, rest) = extract_identifier("foo (x : Nat) : True").unwrap();
        assert_eq!(name, "foo");
        assert_eq!(rest, " (x : Nat) : True");
    }

    #[test]
    fn parse_lean_source_does_not_leak_stray_text_into_the_first_binder_of_a_dotted_lemma_name() {
        // 実データ(DeGiorgiコーパス`Campanato.lean:104`)で発覚したバグの再現。
        let content = r"
lemma HasCampanatoBound.nonneg
    {u : E → ℝ} {x : ℝ}
    (hR : 0 < x) :
    0 ≤ x := by
  trivial
";
        let judgments = parse_lean_source(content);
        assert_eq!(judgments.len(), 1);
        let j = &judgments[0];
        assert_eq!(j.kind, ParsedKind::Theorem);
        assert_eq!(j.name.as_deref(), Some("HasCampanatoBound.nonneg"));
        assert_eq!(j.context.len(), 3, "u, x, hRの3件のはずで、壊れた余分な1件が混じってはいけない");
        assert_eq!(j.context[0].0, "u");
        assert_eq!(j.context[1].0, "x");
        assert_eq!(j.context[2].0, "hR");
    }

    #[test]
    fn parse_lean_source_extracts_theorem_definition_and_axiom() {
        let content = r"
def double (n : Nat) : Nat := n + n

theorem double_eq (n : Nat) : double n = n + n := rfl

axiom classical_choice (P : Prop) : P ∨ ¬P
";
        let judgments = parse_lean_source(content);
        assert_eq!(judgments.len(), 3);
        assert_eq!(judgments[0].kind, ParsedKind::Definition);
        assert_eq!(judgments[0].name.as_deref(), Some("double"));
        assert_eq!(judgments[1].kind, ParsedKind::Theorem);
        assert_eq!(judgments[1].name.as_deref(), Some("double_eq"));
        assert_eq!(judgments[2].kind, ParsedKind::Axiom);
        assert_eq!(judgments[2].name.as_deref(), Some("classical_choice"));
    }

    #[test]
    fn parse_lean_source_marks_unparseable_statements_as_failed_without_dropping_them() {
        // 式として解釈できない命題（AST側が未対応の構文等）は、消してしまうと
        // 「この宣言があったこと」自体が見えなくなる。件数は保ち、状態だけ
        // Failedにする。
        let content = "theorem weird : ∑' n, f n = 0 := sorry";
        let judgments = parse_lean_source(content);
        assert_eq!(judgments.len(), 1);
        // 実際にFullかFailedかはmathesis-astの対応範囲次第なので、ここでは
        // 「消えていないこと」だけを固定する（対応範囲が広がってもテストが
        // 壊れないように）。
        assert_eq!(judgments[0].name.as_deref(), Some("weird"));
    }

    #[test]
    fn strip_lean_comments_removes_line_and_block_comments() {
        let text = "theorem foo : True := by -- trivial\n  trivial /- block -/";
        let stripped = strip_lean_comments(text);
        assert_eq!(stripped, "theorem foo : True := by \n  trivial ");
    }

    #[test]
    fn strip_lean_comments_handles_nested_block_comments() {
        let text = "/- outer /- inner -/ still outer -/ real_code";
        let stripped = strip_lean_comments(text);
        assert_eq!(stripped, " real_code");
    }

    #[test]
    fn strip_lean_comments_removes_doc_comment_style_blocks() {
        let text = "/-!\n# Chapter\nSee `weak_harnack_stage_one_inverse`.\n-/\ntheorem foo : True := trivial";
        let stripped = strip_lean_comments(text);
        assert!(!stripped.contains("weak_harnack_stage_one_inverse"));
        assert!(stripped.contains("theorem foo"));
    }

    #[test]
    fn find_referenced_names_matches_only_known_names_as_whole_words() {
        let known: HashSet<&str> = ["add_comm", "add"].into_iter().collect();
        let text = "exact add_comm a b -- not addition or added";
        let found = find_referenced_names(text, &known);
        assert_eq!(found, vec!["add_comm".to_string()], "add_comm自体は拾うが、addition/addedの部分一致は拾わない");
    }

    #[test]
    fn find_referenced_names_matches_lean_identifiers_with_subscripts_and_greek_letters() {
        let known: HashSet<&str> = ["p₀_pos", "Λ_mul_p₀_sq"].into_iter().collect();
        let text = "have h := p₀_pos hd; exact Λ_mul_p₀_sq hd";
        let mut found = find_referenced_names(text, &known);
        found.sort();
        assert_eq!(found, vec!["p₀_pos".to_string(), "Λ_mul_p₀_sq".to_string()]);
    }

    #[test]
    fn find_dependencies_finds_a_real_reference_and_ignores_doc_comments_and_self_reference() {
        let content = r"
theorem base_lemma : True := trivial

/-! See `base_lemma` for context. -/
theorem derived_thm : True := by exact base_lemma

def rec_def : Nat := rec_def
";
        let judgments = parse_lean_source(content);
        let names: Vec<&str> = judgments.iter().map(|j| j.name.as_deref().unwrap()).collect();
        assert_eq!(names, vec!["base_lemma", "derived_thm", "rec_def"]);

        let edges = find_dependencies(&judgments);
        // derived_thm(1) -> base_lemma(0) だけが本物の依存。doc comment中の
        // 言及（derived_thmの宣言直前のブロックコメント自体はderived_thmの
        // raw_textに含まれないので影響しない）と rec_def の自己参照は
        // どちらも数えない。
        assert_eq!(edges, vec![(1, 0)]);
    }
}

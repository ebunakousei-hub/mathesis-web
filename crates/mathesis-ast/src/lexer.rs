//! Lean4 風の項構文向け最小字句解析器。フル言語ではなく、定理文・型注釈に
//! 現れる範囲の式構文（識別子・数値・括弧・中置/前置演算子・束縛子）を対象とする。

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    Ident(String),
    Number(String),
    Symbol(String),
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Colon,
    ColonEq,
    Eof,
}

/// `start`/`end` は元ソース文字列に対するバイトオフセット。宣言の生テキストを
/// 復元する（`raw_text` フォールバック）ためにインポーター側で使う。
#[derive(Debug, Clone)]
pub struct SpannedTok {
    pub tok: Tok,
    pub line: u32,
    pub start: usize,
    pub end: usize,
}

/// 既知の記号（長いものから順にマッチさせる）
const MULTI_CHAR_SYMBOLS: &[&str] = &[
    "<->", "->", "<=", ">=", "!=", "/\\", "\\/", "::", ":=", "=>",
];

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || ('\u{0370}'..='\u{03FF}').contains(&c)
}

fn is_ident_continue(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '\'' || ('\u{2080}'..='\u{2089}').contains(&c)
}

const SINGLE_SYMBOLS: &[char] = &[
    '→', '↔', '∀', '∃', 'λ', '¬', '∧', '∨', '≤', '≥', '≠', '∈', '∉', '⊆', '⊂', '⊊', '⊇', '∘', '×',
    '•', '≡', '≈', '+', '-', '*', '/', '^', '=', '<', '>', '|', '↦',
];

pub fn lex(src: &str) -> Vec<SpannedTok> {
    let cis: Vec<(usize, char)> = src.char_indices().collect();
    let n = cis.len();
    let src_len = src.len();
    let byte_at = |idx: usize| -> usize {
        if idx < n {
            cis[idx].0
        } else {
            src_len
        }
    };

    let mut out: Vec<SpannedTok> = Vec::new();
    let mut i = 0usize;
    let mut line: u32 = 1;

    macro_rules! push_tok {
        ($tok:expr, $start_idx:expr, $end_idx:expr, $line:expr) => {
            out.push(SpannedTok {
                tok: $tok,
                line: $line,
                start: byte_at($start_idx),
                end: byte_at($end_idx),
            })
        };
    }

    while i < n {
        let c = cis[i].1;

        if c == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        // 行コメント --
        if c == '-' && i + 1 < n && cis[i + 1].1 == '-' {
            while i < n && cis[i].1 != '\n' {
                i += 1;
            }
            continue;
        }
        // ブロックコメント /- ... -/ （入れ子対応）
        if c == '/' && i + 1 < n && cis[i + 1].1 == '-' {
            let mut depth = 1i32;
            i += 2;
            while i < n && depth > 0 {
                if cis[i].1 == '/' && i + 1 < n && cis[i + 1].1 == '-' {
                    depth += 1;
                    i += 2;
                } else if cis[i].1 == '-' && i + 1 < n && cis[i + 1].1 == '/' {
                    depth -= 1;
                    i += 2;
                } else {
                    if cis[i].1 == '\n' {
                        line += 1;
                    }
                    i += 1;
                }
            }
            continue;
        }
        // 文字列リテラルは読み飛ばす
        if c == '"' {
            i += 1;
            while i < n && cis[i].1 != '"' {
                if cis[i].1 == '\\' {
                    i += 1;
                }
                if i < n && cis[i].1 == '\n' {
                    line += 1;
                }
                i += 1;
            }
            i += 1; // closing quote
            continue;
        }

        let start_idx = i;
        match c {
            '(' => {
                push_tok!(Tok::LParen, start_idx, i + 1, line);
                i += 1;
                continue;
            }
            ')' => {
                push_tok!(Tok::RParen, start_idx, i + 1, line);
                i += 1;
                continue;
            }
            '{' => {
                push_tok!(Tok::LBrace, start_idx, i + 1, line);
                i += 1;
                continue;
            }
            '}' => {
                push_tok!(Tok::RBrace, start_idx, i + 1, line);
                i += 1;
                continue;
            }
            '[' => {
                push_tok!(Tok::LBracket, start_idx, i + 1, line);
                i += 1;
                continue;
            }
            ']' => {
                push_tok!(Tok::RBracket, start_idx, i + 1, line);
                i += 1;
                continue;
            }
            ',' => {
                push_tok!(Tok::Comma, start_idx, i + 1, line);
                i += 1;
                continue;
            }
            _ => {}
        }

        // 複数文字記号（最長一致）
        let rest: String = cis[i..].iter().take(3).map(|(_, ch)| *ch).collect();
        let mut matched_multi = false;
        for sym in MULTI_CHAR_SYMBOLS {
            if rest.starts_with(sym) {
                let len = sym.chars().count();
                if *sym == ":=" {
                    push_tok!(Tok::ColonEq, start_idx, i + len, line);
                } else {
                    push_tok!(Tok::Symbol((*sym).to_string()), start_idx, i + len, line);
                }
                i += len;
                matched_multi = true;
                break;
            }
        }
        if matched_multi {
            continue;
        }

        if c == ':' {
            push_tok!(Tok::Colon, start_idx, i + 1, line);
            i += 1;
            continue;
        }

        if c.is_ascii_digit() {
            i += 1;
            while i < n && (cis[i].1.is_ascii_digit() || (cis[i].1 == '.' && i + 1 < n && cis[i + 1].1.is_ascii_digit())) {
                i += 1;
            }
            let s: String = cis[start_idx..i].iter().map(|(_, ch)| *ch).collect();
            push_tok!(Tok::Number(s), start_idx, i, line);
            continue;
        }

        if is_ident_start(c) {
            i += 1;
            loop {
                if i < n && is_ident_continue(cis[i].1) {
                    i += 1;
                    continue;
                }
                if i < n && cis[i].1 == '.' && i + 1 < n && is_ident_start(cis[i + 1].1) {
                    i += 2;
                    while i < n && is_ident_continue(cis[i].1) {
                        i += 1;
                    }
                    continue;
                }
                break;
            }
            let s: String = cis[start_idx..i].iter().map(|(_, ch)| *ch).collect();
            push_tok!(Tok::Ident(s), start_idx, i, line);
            continue;
        }

        if SINGLE_SYMBOLS.contains(&c) {
            push_tok!(Tok::Symbol(c.to_string()), start_idx, i + 1, line);
            i += 1;
            continue;
        }

        // 未知の一文字はスキップ（システム境界: 構文全体を落とさず前進する）
        i += 1;
    }

    out.push(SpannedTok {
        tok: Tok::Eof,
        line,
        start: src_len,
        end: src_len,
    });
    out
}

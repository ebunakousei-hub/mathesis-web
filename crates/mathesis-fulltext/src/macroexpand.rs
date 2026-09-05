//! ローカルに宣言された`\newcommand`/`\def`/`\DeclareMathOperator`マクロを、
//! 命題文（`theorem.rs`の`statement_text`）に限って展開する。
//!
//! 実データ(`scratch/papers_100k_fc.db`、取得済み188論文)で検証: informal
//! な命題文2,621件中1,860件（71.0%）が温ML（`web/src/tex.ts`）の知らない
//! `\command`を含んでいて、そのうち1,361件（73.2%、全体では51.9%）は
//! **その論文自身のソースで`\newcommand`/`\def`/`\DeclareMathOperator`に
//! より宣言されたマクロ**だった。例えば`0705.2309`は
//! `\def\Zset{{\mathbb Z}}`・`\def\Rcal{{\mathcal R}}`のような略記法を
//! 定義し、命題文全体でその略記（`\Zset`・`\Rcal`）を使い回している——
//! 数学論文ではごく普通の書き方で、これを展開せずに温MLへ渡すと
//! 未定義コマンドとして赤いエラー表示になる。展開しておけば
//! `{\mathbb Z}`のような温MLが正しく組める形になる。
//!
//! 対応する宣言の形と、対応しない形（実データの引数個数分布、
//! `\newcommand`宣言6,372件中: 0引数86.5%・1引数9.2%・2引数以上4.3%——
//! 2引数以上はレイアウト補助的なマクロが多く、命題文中の記法
//! ショートハンドとしての使用頻度は低いと判断し対象外とした。既知の
//! 制約——展開せずマクロ呼び出しをそのまま残す、中途半端な誤展開より
//! 安全）:
//!
//! - `\newcommand`/`\renewcommand`（`*`付きも同様に扱う）: 0引数・1引数
//!   のみ対応。`[N][default]`のように第1引数を省略可能にする形は
//!   未対応（展開せず残す）。
//! - `\def`: `\def\X{body}`という引数無しの形のみ対応。`\def\X#1{body}`
//!   のようなパラメータテキストを伴う形は未対応（構文の複雑さに対して
//!   実データでの頻度が低いと判断——`\newcommand`の引数付き形が既に
//!   カバーする）。
//! - `\DeclareMathOperator`（`*`付きも）: 常に0引数。本体を
//!   `\operatorname{...}`（`*`付きなら`\operatorname*{...}`）に読み替える
//!   ——温MLは`\operatorname`をそのまま解釈できる。
//!
//! 種別判定（`\newtheorem`の表示名解決）・証明の紐付け・
//! 依存関係抽出（`\ref`/`\cite`）は、この展開の対象に**含めない**——
//! これらは生の`body`に対して従来通り行う。理由は2つ:
//! (1) 実データで`\newtheorem`の表示名が未知のマクロで包まれていた例は
//!     200論文中1件のみ見つかったが、それは`\theoname`のようなbabel
//!     パッケージが内部で提供する翻訳マクロで、論文自身のソースには
//!     `\newcommand`/`\def`宣言が存在しない（babelの言語ファイルは
//!     取得対象外）——展開しても直らない。
//! (2) `\ref`/`\cite`/`\label`/`\bibitem`は数学論文の著者が再定義する
//!     ことがまず無い標準コマンドで、展開の対象を広げる理由が無い。

use crate::theorem::{read_braced_group, strip_latex_comments};
use std::collections::HashMap;

/// 何度パスを重ねても未解決の呼び出しが残り続ける場合
/// （自己参照・相互参照マクロ——実際のLaTeXコンパイルなら無限ループで
/// 失敗する書き方）に備えた上限。実データで観測される入れ子の深さは
/// せいぜい2〜3段なので、8段あれば正当なケースを取りこぼす心配はない。
const MAX_EXPANSION_PASSES: u32 = 8;

#[derive(Debug, Clone, PartialEq)]
pub struct MacroDef {
    /// 0または1のみ（モジュール冒頭コメント参照——2引数以上は展開
    /// テーブルに入れない）。
    pub arity: usize,
    /// `arity == 1`のときだけ`#1`のプレースホルダを含みうる。
    pub body: String,
}

struct ParsedDeclaration {
    name: String,
    arity: usize,
    /// `[N][default]`のように第1引数の省略時値を伴う宣言。展開テーブルに
    /// は入れないが、宣言自体は正しく読み飛ばす（`next_pos`を正しく
    /// 返すことで、同じ宣言を別パターンとして誤って再解釈しないため）。
    has_unsupported_default: bool,
    body: String,
    next_pos: usize,
}

/// バックスラッシュの直後から始まる「制御綴り」を読む——アルファベットの
/// 最大長の連続、無ければ1文字の「制御記号」（TeXの字句規則そのもの）。
/// 素朴な前方一致だと`\a`が`\ada`の頭にもマッチしてしまうが、これは
/// 必ず`ada`まで正しく読み切ってから比較する。返り値は
/// (バックスラッシュを含まない名前, 名前の直後の位置)。
fn read_control_word(tex: &str, backslash_idx: usize) -> Option<(&str, usize)> {
    if tex.as_bytes().get(backslash_idx) != Some(&b'\\') {
        return None;
    }
    let name_start = backslash_idx + 1;
    if name_start >= tex.len() {
        return None;
    }
    let name_end = tex[name_start..]
        .find(|c: char| !c.is_ascii_alphabetic())
        .map(|r| name_start + r)
        .unwrap_or(tex.len());
    if name_end == name_start {
        // 制御記号（`\%`・`\{`等の1文字）。マクロ宣言・呼び出しの対象には
        // ならないが、1文字だけ読んで返す——呼び出し元が安全に読み飛ばせる
        // ように。
        let mut end = name_start + 1;
        while end < tex.len() && !tex.is_char_boundary(end) {
            end += 1;
        }
        return Some((&tex[name_start..end], end));
    }
    Some((&tex[name_start..name_end], name_end))
}

/// 空白（改行・タブ含む）を読み飛ばす。TeXの制御綴りは直後の空白を
/// 食うので、`\R x`のように書いても`\R`と`x`の間の空白は引数取得の
/// 妨げにならない。
fn skip_spaces(tex: &str, mut pos: usize) -> usize {
    let bytes = tex.as_bytes();
    while pos < bytes.len() && matches!(bytes[pos], b' ' | b'\t' | b'\n' | b'\r') {
        pos += 1;
    }
    pos
}

/// `[`...`]`の対応する`]`まで読む（`read_braced_group`の角括弧版）。
/// `\newcommand`の引数個数`[N]`・省略時値`[default]`用。ネストした`[`は
/// 実際のLaTeXでは稀なので、`\newtheorem`の共有カウンタ読み飛ばしと
/// 同じく対応を数えない素朴な実装で十分。
fn read_bracketed_group(tex: &str, open_idx: usize) -> Option<(&str, usize)> {
    if tex.as_bytes().get(open_idx) != Some(&b'[') {
        return None;
    }
    let close = tex[open_idx + 1..].find(']')? + open_idx + 1;
    Some((&tex[open_idx + 1..close], close + 1))
}

/// `\newcommand`/`\renewcommand`の1宣言を、コマンド名の直後（`*`より前）
/// から読む。
fn parse_newcommand_at(tex: &str, mut pos: usize) -> Option<ParsedDeclaration> {
    if tex.as_bytes().get(pos) == Some(&b'*') {
        pos += 1;
    }
    pos = skip_spaces(tex, pos);

    // マクロ名は`{\X}`（波括弧あり）・`\X`（波括弧無し）のどちらも許す。
    let braced_name = tex.as_bytes().get(pos) == Some(&b'{');
    let name_pos = if braced_name { pos + 1 } else { pos };
    if tex.as_bytes().get(name_pos) != Some(&b'\\') {
        return None;
    }
    let (name, after_name) = read_control_word(tex, name_pos)?;
    let name = name.to_string();
    let mut cursor = after_name;
    if braced_name {
        cursor = skip_spaces(tex, cursor);
        if tex.as_bytes().get(cursor) != Some(&b'}') {
            return None;
        }
        cursor += 1;
    }
    cursor = skip_spaces(tex, cursor);

    let mut arity = 0usize;
    let mut has_arg_count = false;
    if tex.as_bytes().get(cursor) == Some(&b'[') {
        let (count_str, after) = read_bracketed_group(tex, cursor)?;
        arity = count_str.trim().parse().ok()?;
        cursor = skip_spaces(tex, after);
        has_arg_count = true;
    }

    let mut has_unsupported_default = false;
    if has_arg_count && tex.as_bytes().get(cursor) == Some(&b'[') {
        let (_, after) = read_bracketed_group(tex, cursor)?;
        cursor = skip_spaces(tex, after);
        has_unsupported_default = true;
    }

    if tex.as_bytes().get(cursor) != Some(&b'{') {
        return None;
    }
    let (body, after_body) = read_braced_group(tex, cursor)?;

    Some(ParsedDeclaration { name, arity, has_unsupported_default, body: body.to_string(), next_pos: after_body })
}

/// `\def`の1宣言を、コマンド名の直後から読む。`\def\X{body}`という
/// パラメータテキストを伴わない形だけを受理する——`\def\X#1{body}`
/// のように`{`の手前に`#1`等が来る形は、この関数が`None`を返すことで
/// 自然に「未対応」として弾かれる（モジュール冒頭コメント参照）。
fn parse_def_at(tex: &str, pos: usize) -> Option<ParsedDeclaration> {
    let pos = skip_spaces(tex, pos);
    if tex.as_bytes().get(pos) != Some(&b'\\') {
        return None;
    }
    let (name, after_name) = read_control_word(tex, pos)?;
    let name = name.to_string();
    let cursor = skip_spaces(tex, after_name);
    if tex.as_bytes().get(cursor) != Some(&b'{') {
        return None;
    }
    let (body, after_body) = read_braced_group(tex, cursor)?;
    Some(ParsedDeclaration { name, arity: 0, has_unsupported_default: false, body: body.to_string(), next_pos: after_body })
}

/// `\DeclareMathOperator`（`*`付きも）の1宣言を、コマンド名の直後から
/// 読む。本体は`\operatorname{...}`（`*`付きなら`\operatorname*{...}`）
/// に読み替える——温MLは`\operatorname`をそのまま解釈できる。
fn parse_declare_math_operator_at(tex: &str, mut pos: usize) -> Option<ParsedDeclaration> {
    let starred = tex.as_bytes().get(pos) == Some(&b'*');
    if starred {
        pos += 1;
    }
    pos = skip_spaces(tex, pos);

    let braced_name = tex.as_bytes().get(pos) == Some(&b'{');
    let name_pos = if braced_name { pos + 1 } else { pos };
    if tex.as_bytes().get(name_pos) != Some(&b'\\') {
        return None;
    }
    let (name, after_name) = read_control_word(tex, name_pos)?;
    let name = name.to_string();
    let mut cursor = after_name;
    if braced_name {
        cursor = skip_spaces(tex, cursor);
        if tex.as_bytes().get(cursor) != Some(&b'}') {
            return None;
        }
        cursor += 1;
    }
    cursor = skip_spaces(tex, cursor);

    if tex.as_bytes().get(cursor) != Some(&b'{') {
        return None;
    }
    let (operator_text, after_body) = read_braced_group(tex, cursor)?;
    let body =
        if starred { format!("\\operatorname*{{{operator_text}}}") } else { format!("\\operatorname{{{operator_text}}}") };
    Some(ParsedDeclaration { name, arity: 0, has_unsupported_default: false, body, next_pos: after_body })
}

/// 文書全体（プリアンブル・本文の両方——一部の論文は本文中でも初出直前に
/// マクロを定義するため）から、対応する4種の宣言を読み、名前→定義の
/// 対応表を作る。同じ名前が複数回宣言された場合（`\renewcommand`による
/// 上書きを含む）は、文書順で最後に見つかったものが勝つ——`\providecommand`
/// 本来の「未定義のときだけ定義する」意味論までは再現しないが（実データで
/// 188論文中14論文が使用、構文は`\newcommand`と同一）、読み取り専用の
/// 抽出であるこの用途では「結果として何に展開されるか」だけが問題で、
/// 上書き順の細かな意味論の違いは実害が無い。
pub fn parse_macro_definitions(tex: &str) -> HashMap<String, MacroDef> {
    let stripped = strip_latex_comments(tex);
    let tex = stripped.as_str();
    let mut out = HashMap::new();
    let mut pos = 0usize;

    while let Some(rel) = tex[pos..].find('\\') {
        let backslash = pos + rel;
        let Some((name, after_name)) = read_control_word(tex, backslash) else {
            pos = backslash + 1;
            continue;
        };
        let parsed = match name {
            "newcommand" | "renewcommand" | "providecommand" => parse_newcommand_at(tex, after_name),
            "def" => parse_def_at(tex, after_name),
            "DeclareMathOperator" => parse_declare_math_operator_at(tex, after_name),
            _ => None,
        };
        match parsed {
            Some(decl) => {
                if decl.arity <= 1 && !decl.has_unsupported_default {
                    out.insert(decl.name, MacroDef { arity: decl.arity, body: decl.body });
                }
                pos = decl.next_pos;
            }
            None => pos = after_name,
        }
    }
    out
}

/// マクロ呼び出しの実引数を1つ読む。波括弧グループ、または単一トークン
/// （英字1文字か、バックスラッシュ制御綴り1つ）のどちらかを許す——
/// 実際のTeXの引数取得規則を簡略化したもの（モジュール冒頭コメント
/// 参照）。
fn read_one_argument(text: &str, pos: usize) -> Option<(&str, usize)> {
    let pos = skip_spaces(text, pos);
    match text.as_bytes().get(pos) {
        Some(b'{') => read_braced_group(text, pos),
        Some(b'\\') => read_control_word(text, pos).map(|(_, after)| (&text[pos..after], after)),
        Some(_) => {
            let mut end = pos + 1;
            while end < text.len() && !text.is_char_boundary(end) {
                end += 1;
            }
            Some((&text[pos..end], end))
        }
        None => None,
    }
}

/// `macros`に登録されたマクロ呼び出しを1パスぶんだけ展開する。
/// 返り値の`bool`は「このパスで少なくとも1回展開したか」——
/// `expand_macros`が複数パスを重ねるかどうかの判定に使う。
fn expand_once(text: &str, macros: &HashMap<String, MacroDef>) -> (String, bool) {
    let mut out = String::with_capacity(text.len());
    let mut changed = false;
    let mut pos = 0usize;

    while pos < text.len() {
        match text[pos..].find('\\') {
            None => {
                out.push_str(&text[pos..]);
                break;
            }
            Some(rel) => {
                let backslash = pos + rel;
                out.push_str(&text[pos..backslash]);
                let Some((name, after_name)) = read_control_word(text, backslash) else {
                    let end = (backslash + 1).min(text.len());
                    out.push_str(&text[backslash..end]);
                    pos = end;
                    continue;
                };
                match macros.get(name) {
                    Some(def) if def.arity == 0 => {
                        out.push_str(&def.body);
                        changed = true;
                        pos = after_name;
                    }
                    Some(def) if def.arity == 1 => match read_one_argument(text, after_name) {
                        Some((arg, after_arg)) => {
                            out.push_str(&def.body.replace("#1", arg));
                            changed = true;
                            pos = after_arg;
                        }
                        None => {
                            // 呼び出しの形に沿う引数が見つからない——無理に
                            // 消費せず、マクロ名だけそのまま残す。
                            out.push_str(&text[backslash..after_name]);
                            pos = after_name;
                        }
                    },
                    _ => {
                        out.push_str(&text[backslash..after_name]);
                        pos = after_name;
                    }
                }
            }
        }
    }
    (out, changed)
}

/// `text`中のマクロ呼び出しを、展開しきるまで（または`MAX_EXPANSION_PASSES`
/// に達するまで）繰り返し展開する。マクロの本体が別のマクロの呼び出しを
/// 含む場合（例: `\newcommand{\Real}{\R}`で`\R`も定義済み）も、複数パスで
/// 正しく解決する。自己参照・相互参照マクロは`MAX_EXPANSION_PASSES`で
/// 安全に打ち切る（ハングしない——実際のLaTeXコンパイルなら無限ループで
/// 失敗する書き方なので、これ以上「正しい」結果は無い）。
pub fn expand_macros(text: &str, macros: &HashMap<String, MacroDef>) -> String {
    let mut current = text.to_string();
    for _ in 0..MAX_EXPANSION_PASSES {
        let (next, changed) = expand_once(&current, macros);
        if !changed {
            return next;
        }
        current = next;
    }
    current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_newcommand_with_braced_name_and_zero_args() {
        let tex = r"\newcommand{\R}{\mathbb{R}}";
        let macros = parse_macro_definitions(tex);
        assert_eq!(macros.get("R"), Some(&MacroDef { arity: 0, body: "\\mathbb{R}".to_string() }));
    }

    #[test]
    fn parses_newcommand_with_unbraced_name() {
        let tex = r"\newcommand\R{\mathbb{R}}";
        let macros = parse_macro_definitions(tex);
        assert_eq!(macros.get("R"), Some(&MacroDef { arity: 0, body: "\\mathbb{R}".to_string() }));
    }

    #[test]
    fn parses_providecommand_the_same_way_as_newcommand() {
        // 実データ(188論文中14論文が使用)。「未定義のときだけ定義する」と
        // いう`\newcommand`との意味論の違いは、読み取り専用の抽出用途では
        // 実害が無い（`parse_macro_definitions`冒頭コメント参照）。
        let tex = r"\providecommand{\R}{\mathbb{R}}";
        let macros = parse_macro_definitions(tex);
        assert_eq!(macros.get("R"), Some(&MacroDef { arity: 0, body: "\\mathbb{R}".to_string() }));
    }

    #[test]
    fn parses_newcommand_star_variant() {
        let tex = r"\newcommand*{\R}{\mathbb{R}}";
        let macros = parse_macro_definitions(tex);
        assert_eq!(macros.get("R"), Some(&MacroDef { arity: 0, body: "\\mathbb{R}".to_string() }));
    }

    #[test]
    fn parses_newcommand_with_one_argument() {
        let tex = r"\newcommand{\norm}[1]{\lVert #1 \rVert}";
        let macros = parse_macro_definitions(tex);
        assert_eq!(macros.get("norm"), Some(&MacroDef { arity: 1, body: "\\lVert #1 \\rVert".to_string() }));
    }

    #[test]
    fn skips_newcommand_with_two_or_more_arguments() {
        // 実データの引数個数分布で2引数以上は4.3%にとどまり、レイアウト
        // 補助的なマクロが多いと判断——展開テーブルには入れない
        // （モジュール冒頭コメント参照）。
        let tex = r"\newcommand{\pair}[2]{(#1, #2)}";
        let macros = parse_macro_definitions(tex);
        assert!(!macros.contains_key("pair"));
    }

    #[test]
    fn skips_newcommand_with_an_optional_default_first_argument() {
        let tex = r"\newcommand{\greet}[1][World]{Hello, #1!}";
        let macros = parse_macro_definitions(tex);
        assert!(!macros.contains_key("greet"));
    }

    #[test]
    fn renewcommand_overrides_an_earlier_declaration() {
        let tex = r"
            \newcommand{\eps}{\epsilon}
            \renewcommand{\eps}{\varepsilon}
        ";
        let macros = parse_macro_definitions(tex);
        assert_eq!(macros.get("eps"), Some(&MacroDef { arity: 0, body: "\\varepsilon".to_string() }));
    }

    #[test]
    fn parses_zero_arg_def() {
        // 実データ(`0705.2309`)そのままの形。
        let tex = r"\def\Zset{{\mathbb Z}}";
        let macros = parse_macro_definitions(tex);
        assert_eq!(macros.get("Zset"), Some(&MacroDef { arity: 0, body: "{\\mathbb Z}".to_string() }));
    }

    #[test]
    fn skips_def_with_parameter_text() {
        // `\def\X#1{...}`はパラメータテキストを伴う形——未対応
        // （モジュール冒頭コメント参照、`\newcommand`の1引数形で
        // 大半のケースはカバーされる）。
        let tex = r"\def\dbl#1{#1#1}";
        let macros = parse_macro_definitions(tex);
        assert!(!macros.contains_key("dbl"));
    }

    #[test]
    fn parses_declare_math_operator_as_an_operatorname_call() {
        let tex = r"\DeclareMathOperator{\Aut}{Aut}";
        let macros = parse_macro_definitions(tex);
        assert_eq!(macros.get("Aut"), Some(&MacroDef { arity: 0, body: "\\operatorname{Aut}".to_string() }));
    }

    #[test]
    fn parses_starred_declare_math_operator_with_limits() {
        let tex = r"\DeclareMathOperator*{\esssup}{ess\,sup}";
        let macros = parse_macro_definitions(tex);
        assert_eq!(macros.get("esssup"), Some(&MacroDef { arity: 0, body: "\\operatorname*{ess\\,sup}".to_string() }));
    }

    #[test]
    fn does_not_misparse_a_command_name_that_merely_starts_with_def() {
        // `\defcolor`は制御綴りとして"defcolor"全体を読むべきで、
        // "\def"が"defcolor"の一部にマッチしても`\def`宣言としては
        // 扱ってはいけない。
        let tex = r"\defcolor{blue}";
        let macros = parse_macro_definitions(tex);
        assert!(macros.is_empty());
    }

    #[test]
    fn expand_macros_substitutes_a_zero_arg_macro() {
        let mut macros = HashMap::new();
        macros.insert("R".to_string(), MacroDef { arity: 0, body: "\\mathbb{R}".to_string() });
        assert_eq!(expand_macros("Let $x \\in \\R$.", &macros), "Let $x \\in \\mathbb{R}$.");
    }

    #[test]
    fn expand_macros_substitutes_a_one_arg_macro_with_braced_argument() {
        let mut macros = HashMap::new();
        macros.insert("norm".to_string(), MacroDef { arity: 1, body: "\\lVert #1 \\rVert".to_string() });
        assert_eq!(expand_macros("$\\norm{x}$", &macros), "$\\lVert x \\rVert$");
    }

    #[test]
    fn expand_macros_substitutes_a_one_arg_macro_with_a_single_token_argument() {
        let mut macros = HashMap::new();
        macros.insert("norm".to_string(), MacroDef { arity: 1, body: "\\lVert #1 \\rVert".to_string() });
        assert_eq!(expand_macros("$\\norm x$", &macros), "$\\lVert x \\rVert$");
    }

    #[test]
    fn expand_macros_does_not_touch_a_longer_command_that_shares_a_prefix() {
        // `\ada`が定義済みでも、`\adamant`という別のコマンドまで
        // 誤って部分展開してはいけない（`read_control_word`が常に
        // 最大長のアルファベット連続を読むことの検証）。
        let mut macros = HashMap::new();
        macros.insert("ada".to_string(), MacroDef { arity: 0, body: "\\mathbf{a}".to_string() });
        assert_eq!(expand_macros("\\adamant and \\ada", &macros), "\\adamant and \\mathbf{a}");
    }

    #[test]
    fn expand_macros_resolves_a_macro_defined_in_terms_of_another_macro() {
        let mut macros = HashMap::new();
        macros.insert("R".to_string(), MacroDef { arity: 0, body: "\\mathbb{R}".to_string() });
        macros.insert("Real".to_string(), MacroDef { arity: 0, body: "\\R".to_string() });
        assert_eq!(expand_macros("\\Real", &macros), "\\mathbb{R}");
    }

    #[test]
    fn expand_macros_terminates_on_a_self_referential_macro_without_hanging() {
        let mut macros = HashMap::new();
        macros.insert("X".to_string(), MacroDef { arity: 0, body: "\\X".to_string() });
        // ハングせず何らかの文字列を返すことだけを確認する——実際の
        // LaTeXコンパイルでも無限ループになる書き方で、これ以上
        // 「正しい」結果は無い。
        let _ = expand_macros("\\X", &macros);
    }

    #[test]
    fn expand_macros_leaves_an_unresolved_call_untouched_when_the_argument_shape_does_not_match() {
        let mut macros = HashMap::new();
        macros.insert("norm".to_string(), MacroDef { arity: 1, body: "\\lVert #1 \\rVert".to_string() });
        // 引数が続かない（文末）ので展開できない——マクロ名だけ残す。
        assert_eq!(expand_macros("\\norm", &macros), "\\norm");
    }

    #[test]
    fn real_world_pattern_from_0705_2309_expands_correctly() {
        // 実データ(`0705.2309`)の実際のパターン: `\def\Zset{{\mathbb Z}}`
        // と定義した上で、命題文中で`\Zset^s`のように使う。展開しないと
        // 温MLは`\Zset`を未定義コマンドとして赤いエラー表示にしていた。
        let tex = r"\def\Zset{{\mathbb Z}}";
        let macros = parse_macro_definitions(tex);
        let statement = r"Let $\ada_j \in \Zset^s$ denote the coefficient.";
        let expanded = expand_macros(statement, &macros);
        assert_eq!(expanded, "Let $\\ada_j \\in {\\mathbb Z}^s$ denote the coefficient.");
    }
}

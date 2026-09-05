//! LaTeXソースから定理系環境・証明・証明の依存関係を抽出する。
//!
//! 実データ（`math/0110329`）を直接取得して確認したところ、論文本文は
//! `\begin{theorem}`のような文字通りの環境名ではなく、`\newtheorem{thm}
//! {Theorem}`のようにプリアンブルで宣言した短い独自名（`thm`/`prop`/`lem`
//! 等）を使うのが通常だった。決め打ちの環境名リストでは実際の論文の
//! ほぼ全てを取りこぼすため、まず`\newtheorem`宣言を読み、環境名→種別
//! （Theorem/Lemma/…）の対応表を作ってから本文を走査する
//! （宣言が見当たらない場合のみ、標準的な名前をフォールバックとして試す）。
//!
//! 証明の依存関係も実データで確認したところ、同一論文内の他の定理を
//! `\ref`で参照する例は相対的に少なく、外部文献を`\cite`で参照する方が
//! 圧倒的に多かった（`math/0110329`のProof of Theoremは`\cite{kmt}`
//! `\cite{witten}`等の外部引用のみで、他定理への`\ref`は無かった）。
//! そのため両方を別々に記録する: `depends_on_labels`は同一論文内の
//! 他定理への依存（ラベル一致で判定）、`cites`は証明中で使われた
//! 文献引用キー（`\bibitem`が同じ論文内に見つかれば、その生テキストも
//! `resolve_citations`で引ける）。他論文（他arxiv_id）への解決はまだ
//! 行わない——著者名・タイトルの文字列一致だけで別の論文へ結びつけるのは
//! 誤結合のリスクが大きく、別途Entity Resolutionの課題として切り出す。

use crate::macroexpand;
use std::collections::{HashMap, HashSet};

/// LaTeXコメント（エスケープされていない`%`から行末まで）を取り除く。
///
/// 実データ(`math-ph/0101008`)で発見した重要なバグ: 著者がドラフト段階の
/// 証明をコメントアウトしたまま残していた（`%\begin{proof}` ... `%
/// \end{proof}`のように、全行が`%`で始まる）。コメントを剥がさずに
/// 生のテキストを走査していたため、これを本物の証明として拾い、
/// 実際には存在しない依存関係（コメント内の`\ref{...}`）まで抽出して
/// いた。`extract_from_tex`/`extract_bibitems`のどちらも、他の走査を
/// 始める前に必ずこれを通す。`\%`（エスケープされた%、リテラルの
/// パーセント記号）はコメント開始とみなさない。verbatim環境内の`%`は
/// 本来コメントではないが、その区別まではしない——既知の制約（数学論文の
/// 定理・証明の文脈でverbatimはまず出てこない）。
pub(crate) fn strip_latex_comments(tex: &str) -> String {
    let mut out = String::with_capacity(tex.len());
    let mut chars = tex.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            out.push(c);
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else if c == '%' {
            for next in chars.by_ref() {
                if next == '\n' {
                    out.push(next);
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// このパターンの標準名を使う論文が一定数あることは実データでも確認済み
/// （`\newtheorem`宣言がプリアンブル分離されたファイルに書かれていて、
/// 単一ファイルの取得では追えない場合のフォールバック）。
const FALLBACK_THEOREM_ENVS: &[(&str, &str)] = &[
    ("theorem", "Theorem"),
    ("lemma", "Lemma"),
    ("proposition", "Proposition"),
    ("corollary", "Corollary"),
    ("definition", "Definition"),
    ("claim", "Claim"),
    ("conjecture", "Conjecture"),
];

/// 通常「証明」の対象にならない、注釈・補足的な種別（`kind`の表示名で
/// 判定——英語の一般的な種別名にのみ対応、他言語や独自の種別名までは
/// 追いつけない、既知の制約）。実データ(`math/0209001`)で、Corollaryと
/// その本物の証明の間にRemarkが1つ挟まっており、隣接だけで判定すると
/// 証明がRemarkの方に誤って紐付いてしまうのを発見した——直前がこれらの
/// 種別なら読み飛ばしてさらに前を見る（`extract_from_tex`のフェーズ2）。
const NON_PROVABLE_KINDS: &[&str] = &[
    "Remark",
    "Example",
    "Question",
    "Observation",
    "Addendum",
    "Assumption",
    "Assumptions",
    "Notation",
    "Convention",
    "Definition",
    // フランス語の論文でも同じ問題を実データ(`math/0011098`)で確認した
    // （"Remarque"の直後・本来のProofの間に挟まる書き方）。アクセント
    // 記号を含む語（"Définition"のLaTeXエスケープ表記"D\'efinition"等）は
    // 非エスケープ化を行っていないため対象外——アクセント無しの単語のみ。
    "Remarque",
    "Exemple",
];

/// 証明をどうやってこの定理に紐付けたか——確度が全く違うので区別する。
/// `Titled`はamsthmの`\begin{proof}[Proof of Theorem \ref{X}]`のように
/// 著者が明示的に対象を書いている場合（実データ`math/0106165`で確認済み、
/// 宣言から750行も離れた場所にある証明も正しく紐付けられる）。`Adjacent`は
/// タイトルが無い場合のフォールバックで、文書順で直後に来る証明を割り当てる
/// （複数の定理をまとめて後段で証明する書き方にも、直前の未証明の定理が
/// 連続していればまとめて対応する——ただし「6節でまとめて証明する」の
/// ように物理的に離れている場合はタイトルが無い限り検出できない、
/// 既知の制約）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProofMatch {
    Titled,
    Adjacent,
    None,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TheoremRecord {
    pub kind: String,
    pub label: Option<String>,
    /// 文書内の出現順（0始まり）。定理系環境のみでの通し番号。
    pub order: usize,
    pub has_proof: bool,
    pub proof_match: ProofMatch,
    /// 同一論文内の他定理への依存（ラベルで一致したもののみ）。
    pub depends_on_labels: Vec<String>,
    /// 証明中で使われた文献引用キー（`\bibitem`のキーと同じ語彙）。
    pub cites: Vec<String>,
    /// 診断⑥（Statementノード）への対応で追加。`\begin{kind}...\end{kind}`
    /// の中身そのもの（連続空白を1個に圧縮しただけ、LaTeXコマンドの除去は
    /// 行わない——`extract_bibitems`と同じ最小限の正規化）。`mathesis-graph`
    /// 側で`Expr::Unparsed`として保持する生の命題文になる。
    pub statement_text: String,
    /// 元のTeXソース中の行番号（1始まり）。`\begin{kind}`の出現位置から
    /// 数える。複数ファイルを連結して渡した場合は連結後の通し行番号になる
    /// ——ファイルをまたいだ行番号の意味は薄れるが、`order`（文書内の
    /// 出現順）と組み合わせれば実用上は十分特定できる。
    pub line: u32,
}

enum EnvKind {
    Theorem(String),
    /// `title`は`\begin{proof}[ここ]`の角括弧の中身（無ければNone）。
    Proof { title: Option<String> },
}

struct RawEnv {
    kind: EnvKind,
    body: String,
    /// `\begin{name}`の`\`の位置（`document_body`が返すスライス内での
    /// 相対オフセット）。行番号計算にのみ使う。
    start: usize,
}

/// `\newtheorem{env}[...]{DisplayName}[...]` を全て読み、
/// 環境名→表示名（Theorem/Lemma/…）の対応表を作る。
fn parse_newtheorem_kinds(tex: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let marker = "\\newtheorem{";
    let mut pos = 0;
    while let Some(rel) = tex[pos..].find(marker) {
        let name_start = pos + rel + marker.len();
        let Some(name_end_rel) = tex[name_start..].find('}') else { break };
        let env_name = tex[name_start..name_start + name_end_rel].to_string();
        let mut cursor = name_start + name_end_rel + 1;

        // 任意の `[共有カウンタ名]` を読み飛ばす（表示名の前に来る形）。
        if tex[cursor..].starts_with('[') {
            match tex[cursor..].find(']') {
                Some(r) => cursor += r + 1,
                None => break,
            }
        }

        if tex.as_bytes().get(cursor) == Some(&b'{') {
            // 波括弧の対応を数えずに最初の`}`で打ち切る素朴な実装だと、
            // 実データ(`math/0001102`)にあった`\newtheorem{theo}{{\sc
            // Theorem}}`のような二重波括弧（フォント切替コマンドで表示名を
            // 囲む書き方）で`"{\sc Theorem"`という壊れた表示名になっていた
            // （前半の`{`だけを剥がして、中の`{`を表示名の一部として拾って
            // しまう）。`read_braced_group`で対応する`}`まで正しく読み取り、
            // 残った`{...}`や`\sc`等のフォントコマンドは`clean_display_name`
            // で剥がす。
            if let Some((raw_display, after)) = read_braced_group(tex, cursor) {
                let display = clean_display_name(raw_display);
                if !display.is_empty() {
                    out.insert(env_name, display);
                }
                cursor = after;
            }
        }
        pos = cursor;
    }
    out
}

/// `{`の位置`open_idx`から始まる波括弧グループを、ネストを数えながら
/// 読み取る（`{\begin{trivlist}\item[]{\sc Proof.}}`のように内側に
/// さらに`{}`を含む場合に、最初の`}`で誤って打ち切らないため）。
/// 返り値は (中身, 閉じ括弧の直後の位置)。
pub(crate) fn read_braced_group(tex: &str, open_idx: usize) -> Option<(&str, usize)> {
    let bytes = tex.as_bytes();
    if bytes.get(open_idx) != Some(&b'{') {
        return None;
    }
    let mut depth = 0i32;
    let mut i = open_idx;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&tex[open_idx + 1..i], i + 1));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// `\newtheorem`宣言の表示名から取り除く、意味を持たない先頭コマンド。
/// フォントスタイル切替の短縮形（`\sc`等）と`\noindent`（実データ
/// `math/0001102`系の別論文で`\newtheorem{...}{\noindent Theorem}`という
/// 形を確認済み、単なる字下げ抑制で「種別」の意味には無関係）に加えて、
/// 診断⑥「残っている不足」の追加調査（実データ200論文の再測定）で
/// 新たに見つかった3系統:
/// - **長い綴りのフォント切替**（`textbf`・`rmfamily`等）: 実データ
///   `1111.4652`に`\newtheorem{ex}{{\textbf Example}}`、`0712.2580`に
///   `\newtheorem{def}{{\rmfamily Definition}}`という形があった——
///   `\textbf{...}`本来の「引数を取るコマンド」としてではなく、内側の
///   波括弧越しに古いLaTeX2.09的な「宣言」として使われている（著者の
///   混用）。短縮形と同じく種別の意味には無関係なので、綴り違いの兄弟
///   コマンド（`textit`・`textsc`・`textrm`・`textsf`・`texttt`・
///   `sffamily`・`ttfamily`）もまとめて対象にする——見た目は違っても
///   このリストが対象とする「フォント切替」という同じ種類のコマンドで、
///   誤って剥がすリスクは無い。
/// - **`\protect`**: 実データ`1303.4065`の`\newtheorem{...}{\protect
///   \theoremname}`で確認——移動引数の中でも壊れないようにするだけの
///   ラッパーで、それ自体は何も意味しない。剥がした後に残る
///   `\theoremname`はbabel翻訳マクロとして`bridge.rs::kind_synonym`が
///   別途解釈する。
/// - **`n`**: 実データ`0810.2276`の`\newtheorem{thm}{{\n{Theorem}}}`等
///   （プリアンブル全体で9種の定理環境すべてがこの`\n{...}`という
///   独自マクロで包まれていた）——著者が独自に定義した1文字の書式
///   マクロと見られる（正体は特定できないが、全ての種別宣言を機械的に
///   同じ形で包んでいるという使われ方自体が、`\sc`等と同じ「フォント/
///   書式切替であって種別の意味を持たない」ことを強く示している）。
///
/// 既知の範囲に限定して剥がす——「表示名の先頭が偶然バックスラッシュで
/// 始まる別の意味を持つコマンド」まで無差別に剥がして意味を壊さないため。
const MEANINGLESS_LEADING_COMMANDS: &[&str] = &[
    "sc", "scshape", "it", "itshape", "bf", "bfseries", "em", "rm", "sl", "tt", "noindent",
    "textbf", "textit", "textsc", "textrm", "textsf", "texttt", "rmfamily", "sffamily", "ttfamily",
    "protect", "n",
];

/// `\newtheorem`の表示名引数から、余分な外側`{...}`と先頭の無意味な
/// コマンド（`\sc`「\noindent`等）を剥がす。`read_braced_group`のコメント
/// 参照。
fn clean_display_name(raw: &str) -> String {
    let mut s = raw.trim();
    loop {
        if s.starts_with('{') && s.ends_with('}') && s.len() >= 2 {
            s = s[1..s.len() - 1].trim();
            continue;
        }
        if let Some(rest) = s.strip_prefix('\\') {
            let cmd_end = rest.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(rest.len());
            let (cmd, tail) = rest.split_at(cmd_end);
            if MEANINGLESS_LEADING_COMMANDS.contains(&cmd) {
                s = tail.trim_start();
                continue;
            }
        }
        break;
    }
    s.to_string()
}

/// 実データ（`alg-geom/9710014`）で発見した重要なパターン: 証明は
/// amsthmの`\begin{proof}`ではなく、著者が`\newenvironment{pf}{\begin{
/// trivlist}\item[]{\sc Proof.}}{...}`のように独自に定義した環境
/// （名前は"pf"等さまざま）で書かれていることが珍しくない。開始定義
/// （`\newenvironment{NAME}{ここ}{...}`の最初の波括弧グループ）の中に
/// 大文字小文字を問わず"proof"という語が現れたら、その環境名をproofの
/// 別名として扱う——決め打ちの別名リストでは著者ごとの命名の揺れに
/// 追いつけないため、この段階でも`\newtheorem`と同じく「宣言を読んで
/// 判定する」方針を貫く。
fn parse_proof_env_aliases(tex: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    out.insert("proof".to_string());

    let marker = "\\newenvironment{";
    let mut pos = 0;
    while let Some(rel) = tex[pos..].find(marker) {
        let name_start = pos + rel + marker.len();
        let Some(name_end_rel) = tex[name_start..].find('}') else { break };
        let name = tex[name_start..name_start + name_end_rel].to_string();
        let after_name = name_start + name_end_rel + 1;

        // `\newenvironment{name}[nargs][default]{begin-def}{end-def}` の
        // 任意引数`[...]`を読み飛ばしてから、begin-defの波括弧グループを探す。
        let mut cursor = after_name;
        while tex[cursor..].starts_with('[') {
            match tex[cursor..].find(']') {
                Some(r) => cursor += r + 1,
                None => break,
            }
        }

        if tex.as_bytes().get(cursor) == Some(&b'{') {
            if let Some((begin_def, after)) = read_braced_group(tex, cursor) {
                if begin_def.to_lowercase().contains("proof") {
                    out.insert(name);
                }
                pos = after;
                continue;
            }
        }
        pos = after_name;
    }
    out
}

/// `\begin{X}...\end{X}` を先頭から順に走査し、定理系環境とproof環境を
/// 文書順のまま返す。同名環境の入れ子は前提としない（amsthmの通常の
/// 使い方では起きない）。対応する`\end`が見つからない`\begin`は
/// 諦めて読み飛ばす（コメントアウトやマクロ展開の都合で稀に起きる）。
fn scan_environments(tex: &str, kind_of: &HashMap<String, String>, proof_aliases: &HashSet<String>) -> Vec<RawEnv> {
    let mut out = Vec::new();
    let mut pos = 0;
    let begin_marker = "\\begin{";

    while let Some(rel) = tex[pos..].find(begin_marker) {
        let env_start = pos + rel;
        let name_start = env_start + begin_marker.len();
        let Some(name_end_rel) = tex[name_start..].find('}') else { break };
        let name_end = name_start + name_end_rel;
        let name = &tex[name_start..name_end];
        let is_proof = proof_aliases.contains(name);

        let mut body_start = name_end + 1;
        let mut title: Option<String> = None;
        // `\begin{proof}[Proof of Theorem \ref{X}]`のような任意タイトルは
        // proof環境にだけ許す（amsthmの実際の仕様通り——定理側の任意引数
        // は今回のペアリングには使わないので読み飛ばさない）。
        if is_proof && tex.as_bytes().get(body_start) == Some(&b'[') {
            if let Some(close_rel) = tex[body_start..].find(']') {
                let close = body_start + close_rel;
                title = Some(tex[body_start + 1..close].to_string());
                body_start = close + 1;
            }
        }

        let resolved = if is_proof {
            Some(EnvKind::Proof { title })
        } else if let Some(k) = kind_of.get(name) {
            Some(EnvKind::Theorem(k.clone()))
        } else if let Some(&(_, display)) = FALLBACK_THEOREM_ENVS.iter().find(|&&(n, _)| n == name) {
            Some(EnvKind::Theorem(display.to_string()))
        } else {
            None
        };

        let end_marker = format!("\\end{{{name}}}");
        match tex[body_start..].find(&end_marker) {
            Some(end_rel) => {
                let body_end = body_start + end_rel;
                if let Some(kind) = resolved {
                    out.push(RawEnv { kind, body: tex[body_start..body_end].to_string(), start: env_start });
                }
                pos = body_end + end_marker.len();
            }
            None => pos = name_end + 1,
        }
    }
    out
}

fn find_all(body: &str, command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(rel) = body[pos..].find(command) {
        let start = pos + rel + command.len();
        match body[start..].find('}') {
            Some(end_rel) => {
                out.push(body[start..start + end_rel].to_string());
                pos = start + end_rel + 1;
            }
            None => break,
        }
    }
    out
}

fn find_cite_keys(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    for command in ["\\cite{", "\\citep{", "\\citet{"] {
        for group in find_all(body, command) {
            for key in group.split(',') {
                let key = key.trim();
                if !key.is_empty() {
                    out.push(key.to_string());
                }
            }
        }
    }
    out
}

fn dedup_preserve_order(mut items: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    items.retain(|x| seen.insert(x.clone()));
    items
}

fn first_label(body: &str) -> Option<String> {
    find_all(body, "\\label{").into_iter().next()
}

/// 1つのTeX文書（複数ファイルは事前に連結しておく）から定理レコード列を
/// 抽出する。各定理は「文書順で直後に続くproof環境」とだけペアリングする
/// ——複数の定理をまとめて後段で証明する書き方（例:
/// 「これらの証明は5節で行う」）は補足できない、既知の制約。
/// `\begin{document}`から`\end{document}`までを本文として切り出す。
/// 実データ（`alg-geom/9710014`）で発覚した重要なバグの修正——プリアンブルの
/// `\newenvironment{warning}{\begin{warningp}}{\end{warningp}}`のような
/// 「既存の`\newtheorem`環境をラップする別名を定義する」イディオムは、
/// マクロ定義の中身として`\begin{warningp}...\end{warningp}`という
/// トークン列をそのまま含む。本文と地の文を区別しないナイーブな文字列
/// 走査では、これを実際の使用箇所と誤認してしまう（このファイルの実例
/// では12件もの幽霊定理が生成された）。`\begin{document}`より前は
/// 走査しないことで、この種のマクロ定義由来の偽陽性を丸ごと排除する。
/// `\begin{document}`が見つからない断片ファイル（tarの中のsection別
/// ファイル等）は、全体をそのまま本文として扱うフォールバックにする。
/// 返り値は (本文スライス, `tex`内でのそのスライスの開始オフセット)。
/// オフセットは行番号計算（`byte_offset_to_line`）でスライス内の相対
/// 位置を`tex`全体での絶対位置に戻すために使う。
fn document_body(tex: &str) -> (&str, usize) {
    let begin_marker = "\\begin{document}";
    let end_marker = "\\end{document}";
    match (tex.find(begin_marker), tex.find(end_marker)) {
        (Some(start), Some(end)) if end > start => {
            let body_start = start + begin_marker.len();
            (&tex[body_start..end], body_start)
        }
        _ => (tex, 0),
    }
}

/// `byte_offset`（`text`内の絶対位置）が何行目か（1始まり）。
fn byte_offset_to_line(text: &str, byte_offset: usize) -> u32 {
    let clamped = byte_offset.min(text.len());
    1 + text.as_bytes()[..clamped].iter().filter(|&&b| b == b'\n').count() as u32
}

/// 1つの定理の候補スロット（envs内のインデックスを保持し、後段で
/// 「この定理にどの証明が対応するか」を書き込めるようにする）。
struct ThmSlot {
    env_idx: usize,
    kind: String,
    label: Option<String>,
    /// `tex`（`document_body`呼び出し前の全文）内での絶対バイトオフセット。
    absolute_start: usize,
}

pub fn extract_from_tex(tex: &str) -> Vec<TheoremRecord> {
    // コメントアウトされた`%\begin{proof}`等を実際の内容と誤認しないよう、
    // 他の走査を始める前に必ず`%`コメントを剥がす（`strip_latex_comments`
    // のコメント参照）。
    let tex = strip_latex_comments(tex);
    let tex = tex.as_str();

    // `\newtheorem`/`\newenvironment`の宣言はプリアンブルにあるので全文から
    // 読む。環境の実使用は本文（`\begin{document}`以降）だけを見る——
    // 理由は`document_body`のコメント参照。
    let kind_of = parse_newtheorem_kinds(tex);
    let proof_aliases = parse_proof_env_aliases(tex);
    // 診断⑥拡張（LaTeX側で先に正規化する）: `statement_text`だけに適用する
    // 独自マクロ展開。`macroexpand`モジュール冒頭のコメント参照——実データで
    // informalな命題文の51.9%が、この論文自身の`\newcommand`/`\def`/
    // `\DeclareMathOperator`で宣言済みの未展開マクロ（`\Zset`→`{\mathbb
    // Z}`等）を含んでいた。種別判定・証明の紐付け・依存関係抽出は従来通り
    // 生の`body`に対して行う（展開の対象は表示用の`statement_text`のみに
    // 絞る——他の走査まで展開後のテキストに切り替える必要性は無く、
    // 意図しない副作用の芽を減らす）。
    let macros = macroexpand::parse_macro_definitions(tex);
    let (body, body_offset) = document_body(tex);
    let envs = scan_environments(body, &kind_of, &proof_aliases);

    let thm_slots: Vec<ThmSlot> = envs
        .iter()
        .enumerate()
        .filter_map(|(i, e)| match &e.kind {
            EnvKind::Theorem(k) => {
                Some(ThmSlot { env_idx: i, kind: k.clone(), label: first_label(&e.body), absolute_start: body_offset + e.start })
            }
            EnvKind::Proof { .. } => None,
        })
        .collect();

    let theorem_labels: HashSet<String> = thm_slots.iter().filter_map(|t| t.label.clone()).collect();
    let label_to_slot: HashMap<&str, usize> =
        thm_slots.iter().enumerate().filter_map(|(si, t)| t.label.as_deref().map(|l| (l, si))).collect();
    let env_idx_to_slot: HashMap<usize, usize> =
        thm_slots.iter().enumerate().map(|(si, t)| (t.env_idx, si)).collect();

    let mut proof_for: Vec<Option<(&str, ProofMatch)>> = vec![None; thm_slots.len()];

    // フェーズ1（優先・確度が高い）: `\begin{proof}[Proof of Theorem
    // \ref{X}]`のように著者が明示的に対象を書いているものは、文書内の
    // 物理的な距離に関係なく紐付ける。実データ(`math/0106165`)では、
    // 宣言から750行も離れた場所にある証明がこの形で正しく特定できた。
    let mut claimed_by_title: HashSet<usize> = HashSet::new();
    for (i, e) in envs.iter().enumerate() {
        let EnvKind::Proof { title: Some(title) } = &e.kind else { continue };
        let mut used = false;
        for target_label in find_all(title, "\\ref{") {
            if let Some(&si) = label_to_slot.get(target_label.as_str()) {
                if proof_for[si].is_none() {
                    proof_for[si] = Some((e.body.as_str(), ProofMatch::Titled));
                }
                used = true;
            }
        }
        if used {
            claimed_by_title.insert(i);
        }
    }

    // フェーズ2（フォールバック）: タイトルで解決できなかった証明は、
    // 文書順で直前に来る「ただ1つ」の未証明の定理にだけ割り当てる。
    //
    // 最初はここで「直前に連続する未証明の定理を全てまとめて割り当てる」
    // 実装を試したが、実データ(`math/0201099`)を読んで誤りだと判明した:
    // この論文には`\begin{Pro}[Imbalance Principle]`のように**他人の
    // 既知の結果を（証明なしで）引用として書き並べる**箇所が何度もあり、
    // それらは互いに無関係な独立した命題である。実際に1つの`\begin{proof}`
    // は直前の"Bypass attachment"命題(`p:b_at`)だけを証明しており、証明
    // 本文も`p:b_at`の内容（Edge-Rounding Lemma等）にしか触れていない
    // にもかかわらず、まとめて割り当てる実装だと2つ前の"Imbalance
    // Principle"や、さらにその前の無関係な命題にまで同じ証明を誤って
    // 付けてしまった（間に`\begin{figure}`のような未認識環境が挟まる
    // だけで「連続」とみなしてしまうため、実質的に「直近の証明から次の
    // 証明までの間にある定理全部」を無差別に対象にしていた）。複数定理を
    // まとめて証明する書き方は、フェーズ1のタイトル解決
    // （`\begin{proof}[Proof of Theorems~\ref{a} and~\ref{b}]`のように
    // 複数の`\ref`を書ける）に一本化し、著者自身が明示した場合だけ
    // 束ねることにした——著者の明示が無い「なんとなく直前に並んでいる」
    // 状態からの推測は、取りこぼす（既知の制約）方が、無関係な定理に
    // 証明を誤って付けるより安全だと判断した。
    //
    // 「ただ1つ前」にしてもなお別の実データ(`math/0209001`)でバグが
    // あった: `\begin{cor}...\end{cor} \begin{rem}...\end{rem}
    // \begin{proof}...` のように、CorollaryとProofの間に短い注釈的な
    // Remarkが1つ挟まる書き方は珍しくない。実際にこのProofの本文
    // （"Let $M$ be the virtual Chow motive..."）はCorollaryの主張
    // （"virtual Chow motives $M(\sigma)$"）そのものを証明しており、
    // 間のRemark（"The trace of Frobenius is to be interpreted as..."）
    // とは無関係だった。Remark/Example/Definition等は通常「証明」の
    // 対象にならない注釈的な種別なので、直前がこれらに該当する場合は
    // 読み飛ばしてさらに1つ前を見る（英語の一般的な種別名にのみ対応、
    // 他言語や独自の種別名までは追いつけない——`FALLBACK_THEOREM_ENVS`
    // と同じ割り切り）。ただし別のproofか、既に証明済みの定理、あるいは
    // 「証明され得る」種別に行き当たったらそこで確定/打ち切る——
    // 注釈系を無制限に読み飛ばして遠くまで遡ることはしない。
    for (i, e) in envs.iter().enumerate() {
        if claimed_by_title.contains(&i) {
            continue;
        }
        let EnvKind::Proof { .. } = &e.kind else { continue };

        let mut j = i;
        while j > 0 {
            j -= 1;
            match &envs[j].kind {
                EnvKind::Proof { .. } => break,
                EnvKind::Theorem(kind) => {
                    let si = env_idx_to_slot[&j];
                    if proof_for[si].is_some() {
                        break;
                    }
                    if NON_PROVABLE_KINDS.contains(&kind.as_str()) {
                        continue;
                    }
                    proof_for[si] = Some((e.body.as_str(), ProofMatch::Adjacent));
                    break;
                }
            }
        }
    }

    thm_slots
        .into_iter()
        .enumerate()
        .map(|(si, t)| {
            let (depends_on_labels, cites, has_proof, proof_match) = match proof_for[si] {
                Some((body, m)) => {
                    let mut refs = find_all(body, "\\ref{");
                    refs.extend(find_all(body, "\\eqref{"));
                    let depends: Vec<String> = refs
                        .into_iter()
                        .filter(|l| theorem_labels.contains(l) && t.label.as_deref() != Some(l.as_str()))
                        .collect();
                    (dedup_preserve_order(depends), dedup_preserve_order(find_cite_keys(body)), true, m)
                }
                None => (Vec::new(), Vec::new(), false, ProofMatch::None),
            };
            let expanded_body = macroexpand::expand_macros(&envs[t.env_idx].body, &macros);
            let statement_text = expanded_body.split_whitespace().collect::<Vec<_>>().join(" ");
            let line = byte_offset_to_line(tex, t.absolute_start);
            TheoremRecord {
                kind: t.kind,
                label: t.label,
                order: si,
                has_proof,
                proof_match,
                depends_on_labels,
                cites,
                statement_text,
                line,
            }
        })
        .collect()
}

/// `\bibitem{key} ...本文...` を全て読み、キー→生テキスト（改行・連続空白を
/// 1個に圧縮しただけ、LaTeXコマンドの除去までは行わない）の対応表を作る。
pub fn extract_bibitems(tex: &str) -> HashMap<String, String> {
    // コメントアウトされた`\bibitem`を拾わないよう、`extract_from_tex`と
    // 同様にまず`%`コメントを剥がす。
    let tex = strip_latex_comments(tex);
    let tex = tex.as_str();

    let mut out = HashMap::new();
    let marker = "\\bibitem";
    let mut pos = 0;

    while let Some(rel) = tex[pos..].find(marker) {
        let mut cursor = pos + rel + marker.len();
        if tex[cursor..].starts_with('[') {
            match tex[cursor..].find(']') {
                Some(r) => cursor += r + 1,
                None => {
                    pos = cursor;
                    continue;
                }
            }
        }
        if !tex[cursor..].starts_with('{') {
            pos = cursor;
            continue;
        }
        let key_start = cursor + 1;
        let Some(key_end_rel) = tex[key_start..].find('}') else { break };
        let key = tex[key_start..key_start + key_end_rel].to_string();
        let body_start = key_start + key_end_rel + 1;

        let next_bibitem = tex[body_start..].find(marker).map(|r| body_start + r);
        let end_bibliography = tex[body_start..].find("\\end{thebibliography}").map(|r| body_start + r);
        let body_end = [next_bibitem, end_bibliography].into_iter().flatten().min().unwrap_or(tex.len());

        let cleaned = tex[body_start..body_end].split_whitespace().collect::<Vec<_>>().join(" ");
        out.insert(key, cleaned);
        pos = body_end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_latex_comments_removes_comment_text_but_keeps_escaped_percent_signs() {
        let tex = "Real content. % a comment \\ref{fake}\nMore real content, 50\\% done.";
        let stripped = strip_latex_comments(tex);
        assert_eq!(stripped, "Real content. \nMore real content, 50\\% done.");
    }

    #[test]
    fn extract_from_tex_ignores_a_commented_out_proof_and_its_fake_dependency() {
        // 実データ(`math-ph/0101008`)で発覚した重大なバグの再現: 著者が
        // ドラフト段階の証明を`%\begin{proof}` ... `%\end{proof}`のように
        // コメントアウトしたまま残していた。コメントを剥がさずに走査すると、
        // これを本物の証明として拾い、コメント内の`\ref{first}`まで
        // 実在しない依存関係として抽出してしまっていた。
        let tex = r"
            \newtheorem{lem}{Lemma}
            \begin{lem}\label{first}
            First statement.
            \end{lem}
            \begin{proof}
            Trivial.
            \end{proof}
            \begin{lem}\label{second}
            Second statement.
            \end{lem}
            %\begin{proof}
            %This is commented out, citing \ref{first} and \cite{fake}.
            %\end{proof}
        ";
        let records = extract_from_tex(tex);
        assert_eq!(records.len(), 2);
        let second = records.iter().find(|r| r.label.as_deref() == Some("second")).unwrap();
        assert!(!second.has_proof, "コメントアウトされた証明は本物として数えてはいけない");
        assert!(second.depends_on_labels.is_empty(), "コメント内の\\refを依存関係として拾ってはいけない");
        assert!(second.cites.is_empty());
    }

    #[test]
    fn extract_bibitems_ignores_a_commented_out_bibitem() {
        let tex = "\\bibitem{real} A real reference.\n%\\bibitem{fake} A commented-out reference.";
        let bibitems = extract_bibitems(tex);
        assert_eq!(bibitems.len(), 1);
        assert!(bibitems.contains_key("real"));
        assert!(!bibitems.contains_key("fake"));
    }

    #[test]
    fn parse_newtheorem_kinds_strips_font_switch_markup_around_the_display_name() {
        // 実データ(`math/0001102`)で見つかった実際のパターン:
        // `\newtheorem{theo}{{\sc Theorem}}[section]`のように表示名が
        // フォント切替コマンドの波括弧グループで二重に囲まれていた。
        // 波括弧の対応を数えない素朴な実装だと`"{\sc Theorem"`という
        // 壊れた表示名になる。
        let tex = r"
            \newtheorem{theo}{{\sc Theorem}}[section]
            \newtheorem{cor}[theo]{{\sc Corollary}}
        ";
        let kinds = parse_newtheorem_kinds(tex);
        assert_eq!(kinds.get("theo"), Some(&"Theorem".to_string()));
        assert_eq!(kinds.get("cor"), Some(&"Corollary".to_string()));
    }

    #[test]
    fn parse_newtheorem_kinds_strips_a_leading_noindent() {
        // 実データで見つかった別のパターン: `\newtheorem{...}{\noindent Theorem}`。
        let tex = r"\newtheorem{theo}{\noindent Theorem}";
        let kinds = parse_newtheorem_kinds(tex);
        assert_eq!(kinds.get("theo"), Some(&"Theorem".to_string()));
    }

    #[test]
    fn parse_newtheorem_kinds_strips_long_form_font_switch_declarations() {
        // 実データ(1111.4652・0712.2580): `\textbf`・`\rmfamily`を古い
        // LaTeX2.09的な「宣言」として使う（本来の`\textbf{...}`という
        // 引数の取り方ではなく、外側の二重波括弧の中で書式だけ変える）。
        let tex = r"
            \newtheorem{ex}{{\textbf Example}}
            \newtheorem{def}{{\rmfamily Definition}}
        ";
        let kinds = parse_newtheorem_kinds(tex);
        assert_eq!(kinds.get("ex"), Some(&"Example".to_string()));
        assert_eq!(kinds.get("def"), Some(&"Definition".to_string()));
    }

    #[test]
    fn parse_newtheorem_kinds_strips_protect_and_reveals_the_babel_macro_beneath() {
        // 実データ(1303.4065): `\protect`は移動引数向けの頑健化ラッパーで
        // それ自体は無意味——剥がした後の`\theoremname`はbabel翻訳マクロ名
        // として`bridge.rs::kind_synonym`が解釈する。
        let tex = r"\newtheorem{thm}{\protect\theoremname}";
        let kinds = parse_newtheorem_kinds(tex);
        assert_eq!(kinds.get("thm"), Some(&"\\theoremname".to_string()));
    }

    #[test]
    fn parse_newtheorem_kinds_strips_an_idiosyncratic_single_letter_wrapper_macro() {
        // 実データ(0810.2276): 著者が独自に定義したと見られる1文字の書式
        // マクロ`\n`が、宣言した9種の定理環境全てを`\n{...}`という同じ形で
        // 包んでいた。
        let tex = r"\newtheorem{thm}{\n{Theorem}}";
        let kinds = parse_newtheorem_kinds(tex);
        assert_eq!(kinds.get("thm"), Some(&"Theorem".to_string()));
    }

    #[test]
    fn parse_newtheorem_kinds_reads_custom_short_names() {
        let tex = r"
            \newtheorem{thm}{Theorem}
            \newtheorem{prop}[thm]{Proposition}
            \newtheorem{lem}[thm]{Lemma}
        ";
        let kinds = parse_newtheorem_kinds(tex);
        assert_eq!(kinds.get("thm"), Some(&"Theorem".to_string()));
        assert_eq!(kinds.get("prop"), Some(&"Proposition".to_string()));
        assert_eq!(kinds.get("lem"), Some(&"Lemma".to_string()));
    }

    #[test]
    fn extract_from_tex_ignores_begin_end_tokens_embedded_in_a_preamble_macro_definition() {
        // 実データ(alg-geom/9710014)で発覚したバグの再現: プリアンブルの
        // `\newenvironment{warning}{\begin{warningp}}{\end{warningp}}`は
        // "warningp"環境の実際の使用ではなく、単なるマクロ定義の中身。
        // これを本物の使用箇所と誤認すると、実在しない「幽霊定理」が
        // 大量に生成される（このファイルでは実際に12件発生した）。
        let tex = r"
            \newtheorem{warningp}[section]{Warning}
            \newenvironment{warning}{\begin{warningp}}{\end{warningp}}
            \begin{document}
            \newtheorem{thm}{Theorem}
            \begin{thm}\label{real}
            A genuine theorem in the actual document body.
            \end{thm}
            \end{document}
        ";
        let records = extract_from_tex(tex);
        assert_eq!(records.len(), 1, "プリアンブルのマクロ定義由来の幽霊定理を含めてはいけない");
        assert_eq!(records[0].kind, "Theorem");
        assert_eq!(records[0].label.as_deref(), Some("real"));
    }

    #[test]
    fn extract_from_tex_falls_back_to_the_whole_text_when_there_is_no_begin_document() {
        // tar中のsection別ファイル等、\begin{document}を持たない断片も
        // 抽出対象から漏らしてはいけない。
        let tex = r"
            \newtheorem{thm}{Theorem}
            \begin{thm}\label{frag}
            A theorem in a fragment file with no \begin{document}.
            \end{thm}
        ";
        let records = extract_from_tex(tex);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].label.as_deref(), Some("frag"));
    }

    #[test]
    fn parse_proof_env_aliases_recognizes_a_custom_newenvironment_whose_definition_says_proof() {
        // 実データ(alg-geom/9710014)で見つかった実際のパターン:
        // `\begin{proof}`ではなく著者独自の`pf`環境で証明が書かれていた。
        let tex = r"\newenvironment{pf}{\begin{trivlist}\item[]{\sc Proof.}}{\hfill$\Box$\end{trivlist}}";
        let aliases = parse_proof_env_aliases(tex);
        assert!(aliases.contains("pf"));
        assert!(aliases.contains("proof"), "標準名は常に含まれているべき");
    }

    #[test]
    fn parse_proof_env_aliases_does_not_treat_unrelated_newenvironments_as_proofs() {
        let tex = r"\newenvironment{myquote}{\begin{quotation}}{\end{quotation}}";
        let aliases = parse_proof_env_aliases(tex);
        assert!(!aliases.contains("myquote"));
    }

    #[test]
    fn extract_from_tex_pairs_a_theorem_with_a_custom_proof_alias_environment() {
        let tex = r"
            \newtheorem{thm}{Theorem}
            \newenvironment{pf}{\begin{trivlist}\item[]{\sc Proof.}}{\hfill$\Box$\end{trivlist}}
            \begin{thm}\label{main}
            Statement here.
            \end{thm}
            \begin{pf}
            Argument here, citing \cite{gwi}.
            \end{pf}
        ";
        let records = extract_from_tex(tex);
        assert_eq!(records.len(), 1);
        assert!(records[0].has_proof, "独自定義のpf環境も証明として認識されるべき");
        assert_eq!(records[0].cites, vec!["gwi".to_string()]);
    }

    #[test]
    fn extract_from_tex_pairs_each_theorem_with_its_immediately_following_proof() {
        let tex = r"
            \newtheorem{thm}{Theorem}
            \begin{thm}\label{main}
            Statement here.
            \end{thm}
            \begin{proof}
            Some argument citing \cite{smith99} and \cite{jones,doe}.
            \end{proof}
        ";
        let records = extract_from_tex(tex);
        assert_eq!(records.len(), 1);
        let r = &records[0];
        assert_eq!(r.kind, "Theorem");
        assert_eq!(r.label.as_deref(), Some("main"));
        assert!(r.has_proof);
        assert_eq!(r.cites, vec!["smith99".to_string(), "jones".to_string(), "doe".to_string()]);
        assert!(r.depends_on_labels.is_empty());
    }

    #[test]
    fn extract_from_tex_captures_the_statement_text_with_normalized_whitespace() {
        // 診断⑥（Statementノード）への対応で追加。`mathesis-graph`側は
        // この文字列をそのまま`Expr::Unparsed`として保持するので、LaTeX
        // コマンドを除去したりはしない——連続空白の圧縮だけ
        // （`extract_bibitems`と同じ最小限の正規化）。`\label{...}`も
        // 環境の中身の一部としてそのまま残る（意図的——`first_label`は
        // 別途これを読み取るが、`statement_text`側からは取り除かない）。
        let tex = "\\newtheorem{thm}{Theorem}\n\\begin{thm}\\label{main}\n  Let   $X$\nbe a space.\n\\end{thm}\n";
        let records = extract_from_tex(tex);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].statement_text, "\\label{main} Let $X$ be a space.");
    }

    #[test]
    fn extract_from_tex_expands_a_locally_declared_macro_in_the_statement_text() {
        // 診断⑥拡張（LaTeX側で先に正規化する）。実データ(`0705.2309`)の
        // 実際のパターン: `\def\Zset{{\mathbb Z}}`と定義した上で命題文中で
        // `\Zset`を使う。展開しないと温MLは未定義コマンドとして赤い
        // エラー表示にしていた（`macroexpand.rs`冒頭のコメント参照——
        // 実データでinformalな命題文の51.9%がこのパターンの影響を受けて
        // いた）。種別判定・依存関係抽出は生の`body`に対して従来通り行う
        // ——ここでは`statement_text`だけが展開されることを確認する。
        let tex = r"
            \def\Zset{{\mathbb Z}}
            \newtheorem{lem}{Lemma}
            \begin{lem}\label{main}
            Let $x \in \Zset$ be given.
            \end{lem}
        ";
        let records = extract_from_tex(tex);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].statement_text, "\\label{main} Let $x \\in {\\mathbb Z}$ be given.");
    }

    #[test]
    fn extract_from_tex_reports_the_correct_source_line_including_the_preamble() {
        // 診断⑥への対応で追加。行番号はプリアンブル（\begin{document}より
        // 前）を含めた、ファイル全体での通し番号であるべき。
        let tex = "line1 preamble\n\\newtheorem{thm}{Theorem}\n\\begin{document}\nline4\n\\begin{thm}\\label{main}\nStatement.\n\\end{thm}\n\\end{document}\n";
        let records = extract_from_tex(tex);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].line, 5, "1:preamble 2:newtheorem 3:begin-document 4:line4 5:begin-thm");
    }

    #[test]
    fn extract_from_tex_reports_line_one_when_the_theorem_is_on_the_first_line() {
        let tex = "\\begin{theorem}\\label{t}\nStatement.\n\\end{theorem}\n";
        let records = extract_from_tex(tex);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].line, 1);
    }

    #[test]
    fn extract_from_tex_falls_back_to_standard_names_when_no_newtheorem_declared() {
        let tex = r"
            \begin{theorem}\label{t1}
            A statement.
            \end{theorem}
        ";
        let records = extract_from_tex(tex);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].kind, "Theorem");
        assert!(!records[0].has_proof, "この定理には直後のproofが無い");
    }

    #[test]
    fn extract_from_tex_finds_intra_paper_dependency_via_ref_to_another_theorem_label() {
        let tex = r"
            \newtheorem{thm}{Theorem}
            \begin{thm}\label{first}
            First statement.
            \end{thm}
            \begin{proof}
            Trivial.
            \end{proof}
            \begin{thm}\label{second}
            Second statement, builds on the first.
            \end{thm}
            \begin{proof}
            By Theorem \ref{first}, the result follows.
            \end{proof}
        ";
        let records = extract_from_tex(tex);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].label.as_deref(), Some("first"));
        assert!(records[0].depends_on_labels.is_empty());
        assert_eq!(records[1].label.as_deref(), Some("second"));
        assert_eq!(records[1].depends_on_labels, vec!["first".to_string()]);
    }

    #[test]
    fn extract_from_tex_does_not_treat_a_ref_to_a_non_theorem_label_as_a_dependency() {
        // \ref{eq:main} は定理のラベル集合に含まれない数式ラベル——
        // 依存関係としてカウントしてはいけない。
        let tex = r"
            \newtheorem{thm}{Theorem}
            \begin{thm}\label{only}
            $$x = y \label{eq:main}$$
            \end{thm}
            \begin{proof}
            See equation \ref{eq:main}.
            \end{proof}
        ";
        let records = extract_from_tex(tex);
        assert_eq!(records.len(), 1);
        assert!(records[0].depends_on_labels.is_empty());
    }

    #[test]
    fn extract_from_tex_gives_an_untitled_proof_only_to_the_single_immediately_preceding_theorem() {
        // 実データ(`math/0201099`)で見つかった実際のバグの再現:
        // "Imbalance Principle"のような他者の既知の結果を証明なしで
        // 引用として書き並べる箇所が多数あり、それぞれ独立した命題
        // だった。「直前に連続する未証明の定理全てにまとめて割り当てる」
        // 実装だと、直後の無関係なproofを2つも3つも前の命題にまで
        // 誤って付けてしまっていた（間に`\begin{figure}`のような
        // 未認識環境が挟まるだけで「連続」とみなされるため）。タイトルの
        // 無いproofは直前の"ただ1つ"の未証明の定理にしか割り当てない
        // ことで、この誤結合を防ぐ。
        let tex = r"
            \newtheorem{pro}{Proposition}
            \begin{pro}\label{a}
            An unrelated known fact, stated without its own proof here.
            \end{pro}
            \begin{pro}\label{b}
            Another independent known fact.
            \end{pro}
            \begin{figure}
            \caption{a diagram, not a theorem-like environment}
            \end{figure}
            \begin{pro}\label{c}
            The proposition this proof actually proves.
            \end{pro}
            \begin{proof}
            This argument is specifically about c, citing \cite{ref1}.
            \end{proof}
        ";
        let records = extract_from_tex(tex);
        assert_eq!(records.len(), 3);
        assert!(!records[0].has_proof, "aは無関係な既知の事実で、この証明の対象ではない");
        assert!(!records[1].has_proof, "bも同様に対象ではない");
        assert!(records[2].has_proof, "cだけがこの証明の対象");
        assert_eq!(records[2].proof_match, ProofMatch::Adjacent);
        assert_eq!(records[2].cites, vec!["ref1".to_string()]);
    }

    #[test]
    fn extract_from_tex_skips_over_an_intervening_remark_to_find_the_real_theorem_a_proof_belongs_to() {
        // 実データ(`math/0209001`)で見つかった別の実際のバグの再現:
        // `\begin{cor}...\end{cor} \begin{rem}...\end{rem} \begin{proof}...`
        // という並びで、"ただ1つ前"ルールだと証明がRemarkの方に誤って
        // 紐付いてしまっていた。実際にはCorollaryの主張そのものを証明する
        // 内容だった。Remarkのような注釈的種別は読み飛ばして、その前の
        // "証明され得る"種別（Corollary）まで遡るべき。
        let tex = r"
            \newtheorem{cor}{Corollary}
            \newtheorem{rem}[cor]{Remark}
            \begin{cor}\label{main}
            There exist virtual Chow motives M(sigma).
            \end{cor}
            \begin{rem}
            The trace of Frobenius is to be interpreted as the alternating trace.
            \end{rem}
            \begin{proof}
            Let M be the virtual Chow motive attached to the virtual set, citing \cite{W}.
            \end{proof}
        ";
        let records = extract_from_tex(tex);
        assert_eq!(records.len(), 2);
        let cor = records.iter().find(|r| r.kind == "Corollary").unwrap();
        let rem = records.iter().find(|r| r.kind == "Remark").unwrap();
        assert!(cor.has_proof, "証明は本来Corollaryを証明している");
        assert_eq!(cor.cites, vec!["W".to_string()]);
        assert!(!rem.has_proof, "Remarkは注釈であって証明の対象ではない");
    }

    #[test]
    fn extract_from_tex_shares_one_titled_proof_across_multiple_explicitly_named_theorems() {
        // 複数定理をまとめて証明する正当なやり方: タイトルに複数の`\ref`を
        // 書く（著者自身の明示的な意図なので、隣接推測より安全に束ねられる）。
        let tex = r"
            \newtheorem{thm}{Theorem}
            \begin{thm}\label{a}
            Statement A.
            \end{thm}
            \begin{thm}\label{b}
            Statement B, unrelated content in between does not matter here.
            \end{thm}
            \begin{proof}[Proof of Theorems~\ref{a} and~\ref{b}]
            Proof of both A and B, citing \cite{ref1}.
            \end{proof}
        ";
        let records = extract_from_tex(tex);
        assert_eq!(records.len(), 2);
        assert!(records[0].has_proof, "タイトルで明示的に名指しされたAは証明されるべき");
        assert!(records[1].has_proof, "同じくBも");
        assert_eq!(records[0].proof_match, ProofMatch::Titled);
        assert_eq!(records[1].proof_match, ProofMatch::Titled);
        assert_eq!(records[0].cites, vec!["ref1".to_string()]);
    }

    #[test]
    fn extract_from_tex_resolves_a_titled_proof_far_from_its_theorem_via_ref() {
        // 実データ(`math/0106165`)で見つかった実際のパターン: 定理の宣言
        // から750行も離れた場所に、`\begin{proof}[Proof of Theorem \ref{X}]`
        // という形で証明が置かれていた。物理的な距離に関係なく、タイトルの
        // `\ref`で明示的に紐付ける。
        let tex = r"
            \newtheorem{thm}{Theorem}
            \begin{thm}\label{main}
            The main statement, proved much later.
            \end{thm}
            \begin{thm}\label{other}
            An unrelated theorem stated in between.
            \end{thm}
            \begin{proof}
            Proof of the unrelated theorem.
            \end{proof}
            Lots of unrelated discussion goes here, spanning what would be
            many lines in a real paper.
            \begin{proof}[Proof of Theorem \ref{main}]
            This finally proves the main theorem, citing \cite{smith}.
            \end{proof}
        ";
        let records = extract_from_tex(tex);
        assert_eq!(records.len(), 2);
        let main = records.iter().find(|r| r.label.as_deref() == Some("main")).unwrap();
        assert!(main.has_proof, "離れた場所にあるタイトル付き証明も紐付けられるべき");
        assert_eq!(main.proof_match, ProofMatch::Titled);
        assert_eq!(main.cites, vec!["smith".to_string()]);

        let other = records.iter().find(|r| r.label.as_deref() == Some("other")).unwrap();
        assert_eq!(other.proof_match, ProofMatch::Adjacent, "こちらは直後のuntitled proofで隣接一致するべき");
    }

    #[test]
    fn extract_from_tex_falls_back_to_adjacency_when_a_titled_proof_names_an_unresolvable_target() {
        // タイトルはあるが`\ref`が無い（プレーンテキストの番号だけ）場合は
        // 解決できないので、通常の隣接フォールバックに任せる。
        let tex = r"
            \newtheorem{thm}{Theorem}
            \begin{thm}\label{only}
            Statement.
            \end{thm}
            \begin{proof}[Proof of Theorem 1.2]
            Argument.
            \end{proof}
        ";
        let records = extract_from_tex(tex);
        assert_eq!(records.len(), 1);
        assert!(records[0].has_proof);
        assert_eq!(records[0].proof_match, ProofMatch::Adjacent);
    }

    #[test]
    fn extract_from_tex_does_not_let_an_already_titled_proof_bleed_into_an_earlier_untitled_theorem() {
        // Bがタイトル付きproofで正しく特定された場合、直前のAに
        // その証明を誤って再利用してはいけない（Aは証明無しのまま）。
        let tex = r"
            \newtheorem{thm}{Theorem}
            \begin{thm}\label{a}
            Statement A, left unproved here.
            \end{thm}
            \begin{thm}\label{b}
            Statement B.
            \end{thm}
            \begin{proof}[Proof of Theorem \ref{b}]
            Only proves B.
            \end{proof}
        ";
        let records = extract_from_tex(tex);
        assert_eq!(records.len(), 2);
        assert!(!records[0].has_proof, "Aはタイトル付き証明の対象に指定されていない");
        assert!(records[1].has_proof);
        assert_eq!(records[1].proof_match, ProofMatch::Titled);
    }

    #[test]
    fn extract_bibitems_reads_key_to_raw_text_pairs_until_the_next_bibitem() {
        let tex = r"
            \begin{thebibliography}{9}
            \bibitem{kmt}
            D. Kotschick, J. Morgan, and C. Taubes,
            Four-manifolds without symplectic structures.

            \bibitem{lno}
            C. LeBrun, Four-manifolds without Einstein metrics.
            \end{thebibliography}
        ";
        let bibitems = extract_bibitems(tex);
        assert_eq!(bibitems.len(), 2);
        assert!(bibitems["kmt"].contains("Kotschick"));
        assert!(bibitems["lno"].contains("LeBrun"));
        assert!(!bibitems["kmt"].contains("LeBrun"), "次のbibitemの本文が混入してはいけない");
    }

    #[test]
    fn extract_bibitems_handles_the_optional_bracket_label_form() {
        let tex = r"\bibitem[KMT95]{kmt} Kotschick et al.";
        let bibitems = extract_bibitems(tex);
        assert_eq!(bibitems.get("kmt").map(String::as_str), Some("Kotschick et al."));
    }
}

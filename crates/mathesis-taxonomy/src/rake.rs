//! RAKE（Rapid Automatic Keyword Extraction, Rose et al. 2010）による、
//! 1論文（title+abstract）からの候補概念フレーズ抽出。
//!
//! アーキテクチャ.txt 5.4「候補語抽出」の実装。LLMを最初から使わず、まず
//! 統計的手法で候補を出す（5.4「LLMは全件の一次分類器にはしない」）ための
//! 土台。手順は原論文どおり:
//!   1. 句読点・数式デリミタでテキストを分割する
//!   2. 各分割区間をさらにストップワードで割り、残った連続語を候補フレーズとする
//!   3. コーパス（ここでは1論文）内の単語共起から degree(w)/freq(w) を求め、
//!      フレーズ内の単語ごとのスコアの総和をフレーズスコアとする
//!
//! コーパス全体（全論文）にまたがる文書頻度での足切りは `concepts.rs` 側の仕事。

use std::collections::HashMap;

/// 標準的な英語のストップワード（文法語）に加え、抄録に頻出する定型的な
/// 前置き語（"paper", "result" 等）も含める——これらは概念名の一部になる
/// ことがほぼ無く、含めておいた方がフレーズの切れ目がきれいになる。
const STOPWORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "nor", "so", "yet", "of", "in", "on", "at", "to", "for",
    "with", "by", "from", "as", "is", "are", "was", "were", "be", "been", "being", "this", "that",
    "these", "those", "it", "its", "we", "our", "us", "you", "your", "they", "their", "he", "she",
    "his", "her", "i", "my", "which", "who", "whom", "whose", "what", "where", "when", "how",
    "why", "also", "than", "then", "if", "not", "no", "do", "does", "did", "can", "cannot",
    "could", "may", "might", "must", "shall", "should", "will", "would", "have", "has", "had",
    "such", "some", "any", "all", "each", "every", "other", "another",
    "into", "onto", "over", "under", "between", "among", "within", "without", "upon", "about", "above",
    "below", "up", "down", "out", "off", "again", "further", "here", "there", "very", "more",
    "most", "much", "many", "several", "same", "own", "only", "just", "even", "still", "yet",
    "paper", "papers", "result", "results", "show", "shows", "shown", "showing", "prove",
    "proves", "proved", "proving", "study", "studies", "studied", "studying", "obtain", "obtains",
    "obtained", "give", "gives", "given", "giving", "present", "presents", "presented",
    "presenting", "consider", "considers", "considered", "considering", "known", "well", "new",
    "using", "use", "uses", "used", "let",
    // 実データ（arXivの抄録5,000件）で確認された、数学的内容を一切運ばない
    // 定型的な前置き・つなぎ語。放置すると単独の高頻度語として上位を占拠する
    // うえ、隣接する本当の内容語同士を誤って1つのフレーズに繋げてしまう。
    "discuss", "discusses", "discussed", "discussing", "describe", "describes", "described",
    "describing", "introduce", "introduces", "introduced", "introducing", "find", "finds",
    "found", "finding", "finally", "moreover", "furthermore", "however", "therefore", "thus",
    "hence", "note", "notes", "noted", "establish", "establishes", "established", "apply",
    "applies", "applied", "applying", "derive", "derives", "derived", "deriving", "compute",
    "computes", "computed", "computing", "fact", "particular", "construct", "constructs",
    "constructed", "constructing", "recent", "recently", "certain", "case", "cases",
    // 実データ（arXiv 10,000論文へのスケールテストで確認）: 他の論文への
    // 参照に使われる定型語（"(see math/9912150)"・"cf. [12]"・
    // "preprint math/9601010" 等）。"math/9912150"のようなID自体は
    // `strip_arxiv_id_citations` で除去済みだが、"see"・"cf"・"preprint"・
    // "arxiv" 自体は普通の単語として残るため、別途ストップワード化する。
    "see", "cf", "preprint", "preprints", "arxiv", "et", "al",
    // 実データ（arXiv 100,000論文への100kスケールテストで確認）:
    // 5,000〜10,000論文規模では埋もれていたが、10万論文規模では純粋な
    // 論文執筆の定型語が複合語候補として大量の文書頻度を稼ぎ、本物の
    // 数学概念（"moduli space"・"lie algebra"等）と並んで上位を占拠していた
    // （例: "sufficient conditions"・"closely related"・"previous work"・
    // "first part"・"second part"）。ここに挙げる語は「ほぼ常に定型句の
    // 一部としてしか出現せず、それ単体で数学概念の名前を構成することが
    // 無い」と判断できたものに限る——"first"/"second"は"first Chern
    // class"のような実在の概念にも現れるが、その場合はストップワードで
    // 区切られることでより一般的な"chern class"として集約される側に
    // 倒れるだけで、失われるわけではない（頻度の低い"first X"/"second X"
    // が別々の候補に分散するより、むしろ集約された方が概念として拾い
    // やすくなる）。一方、"large"（"large deviations"「large cardinal"）・
    // "simple"（"simple group"「simple pole"）・"finite"「number"（"finite
    // group"「number theory"）のように、一般語でありながら実在の数学概念の
    // 一部としても頻出する語はここでは止めない——それらが作る定型句
    // （"large class"「simple proof"「finite number"）は`concepts.rs`側で
    // フレーズ単位の完全一致で個別に除外する。
    "sufficient", "closely", "previous", "first", "second",
    // 実データ（100kコーパスの`concept_candidates`を直接検査して発見、
    // 2026-09-05）: 上の"first"/"second"とは逆に、"one"/"two"/"three"は
    // 元々の英語ストップワードリストにそのまま含まれていたが、これは
    // 数学分野では誤りだと判明した。"genus zero"(100件)・"genus four"
    // (14件)・"genus five"(4件)・"genus six"(4件)は候補として生き残る
    // 一方、**"genus one"・"genus two"・"genus three"だけが存在しない**
    // ——ゼロ・四・五・六は数詞として止めていないのに、一・二・三だけ
    // "one"/"two"/"three"がストップワードだったせいで消えていた
    // （"characteristic zero"650件は残るが"characteristic two"
    // "characteristic three"が同様に消えている）。genus 0/1/2/3や
    // characteristic 0/2/3は数学的に全く別の対象を指す固有の値であり、
    // "genus"や"characteristic"に一般化して失ってよい情報ではない
    // （"first"/"second"の場合の「より一般的な語に集約されるだけ」という
    // 正当化がここでは成立しない）。"one"/"two"/"three"を止めるべき
    // 特別な理由（"one of the"のような用法の除去）は測定でも確認できず、
    // 実害の方が大きいと判断し、ストップワードから外した。
];

/// アカデミックな英語の書き方特有の「つなぎの分詞・動詞」で、ほぼ常に
/// 前後の内容語を関係節的に接続するだけの働きしかせず、それ自体が
/// 数学概念の名前になることが無いと実データ（100kコーパスの
/// `concept_candidates`を直接検査、2026-09-05）で確認できたもの。
///
/// 例（実データ、いずれも完全な主張の断片で概念名ではない）:
///   "naturally associated"は無害だが"algebras associated"「polynomials
///   associated"のように前置詞ごと欠落した断片が374件、"functions
///   defined"「elliptic curve defined"のような断片が193件、"approach
///   based"「method based"が155件、"problems related"「directly
///   related"が131+116件、"following question"「following theorem"の
///   ような"the following X"という単なる前方参照が123件、"admits"単体が
///   1206件、"contain"単体が778件、"goes"単体が365件——これらは(subject,
///   kind, object)の形で他の候補と関係付けられても意味を持たない。
///
/// 一方、**同じ語幹でも別の語は意図的に含めていない**——実データで
/// 頭に付く用法が正当な数学用語を構成すると確認できたため:
///   "associated"（"associated primes"51件・"associated graded ring"
///   35件・"associated graded algebra"17件、いずれも可換代数の標準的な
///   概念）、"generated"/"generating"（"generating function"520件・
///   "finitely generated"416件・"generating set"109件）、"defined"の
///   語幹でも"defining"（"defining relations"74件・"defining ideal"
///   48件・"defining equations"44件）、"induced"（"induced
///   representations"60件・"induced subgraph"47件・"induced metric"
///   29件、表現論・グラフ理論の標準語彙）、"representing"（"representing
///   measure"7件、関数解析の標準語彙）、"turn"/"turning"/"return"
///   （"turning point"19件・"return time"19件・"return map"12件、
///   力学系・特異点論の標準語彙）。これらは末尾に付く場合は断片になりうる
///   （例:"metric induced"）が、先頭の正当な用法を守る方を優先し、
///   ストップワード化を見送った——RAKEのストップワードは出現位置を
///   区別できないため、位置に応じた片側だけの除去はできない制約による。
const CONNECTIVE_FRAGMENT_WORDS: &[&str] = &[
    "due", "corresponding", "corresponds", "correspond", "existing", "exist", "exists",
    "existed", "following", "follow", "follows", "followed", "related", "relates", "relate",
    "relating", "arising", "arise", "arises", "arisen", "arose", "satisfying", "satisfies",
    "satisfy", "satisfied", "admitting", "admits", "admit", "admitted", "contains",
    "containing", "contained", "contain", "involving", "involves", "involve", "involved",
    "based", "yields", "yield", "yielded", "yielding", "goes", "go", "going", "gone", "comes",
    "come", "coming",
    // 実データ（本番100kコーパスへの再抽出後、`relations`の出力を実際に
    // 読んで発見、2026-09-05・同日追記）: 上の初回リストでは"having"を
    // 見落としており、"2-smooth banach spaces ≡ banach spaces having"の
    // ように断片が`relations`の候補にまで残っていた。"having"（"graphs
    // having"10件・"spaces having"10件）・"consisting"/"consists"/
    // "consist"（"pairs consisting"22件・"method consists"39件、"consist
    // of"の定型句で先頭・末尾どちらでも概念名にならない）・"depends"/
    // "depend"/"depending"（"constant depending"57件、常に断片）・
    // "implies"/"imply"/"implying"（"conjecture implies"26件、常に断片）・
    // "required"/"requires"/"require"/"requiring"（"conditions
    // required"14件、常に断片）・"denote"/"denotes"/"denoted"/"denoting"
    // （"usually denoted"6件、定義を導入するだけの語）を追加。
    //
    // 一方、**同じ語幹でも意図的に含めていない語**（実データで先頭の
    // 用法が正当な数学用語だと確認済み）: "cover"/"covers"/"covering"/
    // "covered"（"universal cover"165件・"covering space"44件・"double
    // cover"72件、位相幾何の標準語彙）、"lie"/"lies"/"lying"（"lie
    // algebra"1061件・"lie algebras"684件・"lie groups"366件——Sophus
    // Lieの固有名で、このコーパス最頻出の概念の一つ）、"implied"
    // （"implied volatility"12件・"implied constant"8件、解析数論・
    // 数理ファイナンスの標準語彙——"imply"の他の活用形とは別に扱う）。
    "having", "consisting", "consists", "consist", "consisted", "depends", "depend",
    "depending", "depended", "implies", "imply", "implying", "required", "requires", "require",
    "requiring", "denote", "denotes", "denoted", "denoting",
];

fn stopword_set() -> std::collections::HashSet<&'static str> {
    STOPWORDS.iter().chain(CONNECTIVE_FRAGMENT_WORDS).copied().collect()
}

/// LaTeXのアクセント記法（生のソースがabstractに残ったもの）を実際の
/// Unicode文字に変換する。実データで "Schrodinger"（無表記）/
/// "Schroedinger"（oe転写）/ "Schr\"odinger"（生LaTeX）の3通りの綴りが
/// 同一コーパス中に混在することを確認した。ここでの目的は生LaTeXが原因で
/// 単語が "schr" + "odinger" のように破損して分裂するのを防ぐことだけで、
/// 3通りの綴りを1つの概念へ統一すること自体は本格的なEntity Resolution
/// （Phase 3以降）の仕事として残す。
fn unescape_latex_accents(text: &str) -> String {
    const ACCENTS: &[(char, char, char)] = &[
        ('"', 'a', 'ä'), ('"', 'o', 'ö'), ('"', 'u', 'ü'),
        ('"', 'A', 'Ä'), ('"', 'O', 'Ö'), ('"', 'U', 'Ü'),
        ('\'', 'a', 'á'), ('\'', 'e', 'é'), ('\'', 'i', 'í'), ('\'', 'o', 'ó'), ('\'', 'u', 'ú'), ('\'', 'y', 'ý'),
        ('\'', 'A', 'Á'), ('\'', 'E', 'É'), ('\'', 'I', 'Í'), ('\'', 'O', 'Ó'), ('\'', 'U', 'Ú'),
        ('`', 'a', 'à'), ('`', 'e', 'è'), ('`', 'i', 'ì'), ('`', 'o', 'ò'), ('`', 'u', 'ù'),
        ('^', 'a', 'â'), ('^', 'e', 'ê'), ('^', 'i', 'î'), ('^', 'o', 'ô'), ('^', 'u', 'û'),
        ('~', 'n', 'ñ'), ('~', 'o', 'õ'), ('~', 'a', 'ã'),
    ];

    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            let accent = chars[i + 1];
            let braced = i + 3 < chars.len() && chars[i + 2] == '{';
            let base_idx = if braced { i + 3 } else { i + 2 };
            if base_idx < chars.len() {
                let base = chars[base_idx];
                if let Some(&(_, _, resolved)) = ACCENTS.iter().find(|(a, b, _)| *a == accent && *b == base) {
                    out.push(resolved);
                    let has_close_brace = braced && base_idx + 1 < chars.len() && chars[base_idx + 1] == '}';
                    i = base_idx + 1 + usize::from(has_close_brace);
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// 旧形式のarXiv自己引用（"math/9912150"・"math.AG/9904159"・
/// "hep-th/9801109" のような「アーカイブ名+7桁ID」、2007年以前の
/// 命名規則）をまるごと取り除く。取り除かないと `/` が区切り文字として
/// 扱われるせいでアーカイブ名（多くの場合 "math"）だけが独立した単語
/// として残ってしまい、無内容な "see math" のような候補ができる
/// （実データ、min_df=5を満たすほど頻出することを確認済み）。
fn strip_arxiv_id_citations(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_alphabetic() {
            let mut j = i;
            while j < chars.len() && (chars[j].is_ascii_alphabetic() || chars[j] == '.' || chars[j] == '-') {
                j += 1;
            }
            if j < chars.len() && chars[j] == '/' {
                let mut k = j + 1;
                while k < chars.len() && chars[k].is_ascii_digit() {
                    k += 1;
                }
                if k - (j + 1) == 7 {
                    // アーカイブ名+"/"+7桁ID全体をまとめて空白1つに置き換える
                    out.push(' ');
                    i = k;
                    continue;
                }
            }
            out.extend(&chars[i..j]);
            i = j;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// バックスラッシュで始まるLaTeXコマンド（`\bC`・`\fg`・`\ep`・`\mathfrak`
/// 等——多くは数学者が独自に定義した私的マクロで、事前に列挙できない）を
/// まるごと取り除く。呼ぶのは`unescape_latex_accents`より後にすること
/// （`\"o`のようなアクセント記法はそちらで先にUnicode文字へ解決済みに
/// しておき、ここでは「バックスラッシュの直後が英字」という、アクセント
/// 記法とは異なる形だけを対象にする）。
///
/// 実データ（10,000論文へのスケールテストで発見）: このステップが無いと
/// "\bC"（黒板太字のℂ）→"bc"、"\fg"（\mathfrak{g}相当の私的マクロ）→"fg"、
/// "\ep"（epsilonの私的マクロ）→"ep" のように、無内容な2文字前後の
/// 候補が大量に生き残っていた（アルファベット順にほぼ総当たりで"ab"〜
/// "gg"のような組み合わせが100件以上）。
fn strip_latex_commands(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() && chars[i + 1].is_ascii_alphabetic() {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_ascii_alphabetic() {
                j += 1;
            }
            out.push(' ');
            i = j;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// テキストを、句読点・数式デリミタ（$ など）で区切った「区間」の列に分ける。
/// 各区間はさらに小文字化した単語の並び。ハイフンは語の一部として保持する
/// （"p-group", "co-limit" のような数学用語のため）。
fn word_segments(text: &str) -> Vec<Vec<String>> {
    let stripped = strip_arxiv_id_citations(text);
    let unescaped = unescape_latex_accents(&stripped);
    let no_commands = strip_latex_commands(&unescaped);
    let lower = no_commands.to_lowercase();
    let mut segments = Vec::new();
    let mut cur_segment: Vec<String> = Vec::new();
    let mut cur_word = String::new();

    let flush_word = |word: &mut String, seg: &mut Vec<String>| {
        if !word.is_empty() {
            seg.push(std::mem::take(word));
        }
    };

    for c in lower.chars() {
        if c.is_alphanumeric() || c == '-' {
            cur_word.push(c);
        } else if c.is_whitespace() {
            flush_word(&mut cur_word, &mut cur_segment);
        } else {
            // 句読点・$ 等の「硬い」区切り
            flush_word(&mut cur_word, &mut cur_segment);
            if !cur_segment.is_empty() {
                segments.push(std::mem::take(&mut cur_segment));
            }
        }
    }
    flush_word(&mut cur_word, &mut cur_segment);
    if !cur_segment.is_empty() {
        segments.push(cur_segment);
    }
    segments
}

/// 純粋な数字トークン、および1文字トークン（数式の変数名の残骸が多い）は
/// 候補フレーズの構成語として扱わない。
/// 見つけたバグ（実データでOllama embeddingにかけて発覚）:
/// 1) 旧実装は「全部が数字ではない」ことしか見ておらず、"-1"・"-2"・"--"・
///    "-n" のような、ハイフンと数字/1文字だけのトークンを内容語として
///    通してしまっていた（ハイフンは語の一部として保持しているため、
///    "3n" のように数字と文字が混ざるだけで「全部数字」判定を回避できる）。
/// 2) "$p$-group" のように数式変数1文字だけが `$...$` の中にある表記では、
///    その1文字（"p"）が`$`で囲まれて独立トークンになり別途捨てられる一方、
///    残った "-group" だけがそのまま内容語として通ってしまい、
///    "-algebra"・"-group"・"-function" のような意味の欠けた候補が
///    大量に生き残っていた（実際にembeddingの近傍検索で発覚）。
///    "p-group" のような正当な複合語ではハイフンは常に語の途中に現れる
///    ——先頭または末尾がハイフンの語は、必ず何か（数式変数など）が
///    削れた残骸なので、その形自体を弾く。
fn is_content_word(w: &str) -> bool {
    let starts_ok = w.chars().next().is_some_and(char::is_alphanumeric);
    let ends_ok = w.chars().next_back().is_some_and(char::is_alphanumeric);
    starts_ok && ends_ok && w.chars().filter(|c| c.is_alphabetic()).count() >= 2
}

/// 区間をストップワードでさらに割り、候補フレーズ（連続する内容語）を得る。
/// 極端に長いフレーズ（句読点の少ない箇条書き等に由来）は概念名としての
/// 妥当性が下がるため、6語を超えるものは捨てる。
fn candidate_phrases(text: &str) -> Vec<Vec<String>> {
    let stopwords = stopword_set();
    let mut phrases = Vec::new();

    for segment in word_segments(text) {
        let mut cur: Vec<String> = Vec::new();
        for w in segment {
            if stopwords.contains(w.as_str()) || !is_content_word(&w) {
                if !cur.is_empty() {
                    phrases.push(std::mem::take(&mut cur));
                }
            } else {
                cur.push(w);
            }
        }
        if !cur.is_empty() {
            phrases.push(cur);
        }
    }

    phrases.retain(|p| !p.is_empty() && p.len() <= 6);
    phrases
}

/// 1論文のtitle+abstractテキストから、RAKEスコア付きの候補フレーズを返す
/// （同一フレーズが複数回出現しても1件にまとめる。スコアはRAKEの定義上
/// 出現位置によらず一定なのでそのまま採用してよい）。
pub fn score_document(text: &str) -> Vec<(String, f64)> {
    let phrases = candidate_phrases(text);

    // 単語ごとの degree（自身を含む、その語が現れた全フレーズの長さの和）
    // と freq（その語の出現フレーズ数）を集計する。
    let mut degree: HashMap<&str, usize> = HashMap::new();
    let mut freq: HashMap<&str, usize> = HashMap::new();
    for phrase in &phrases {
        let len = phrase.len();
        for w in phrase {
            *degree.entry(w.as_str()).or_default() += len;
            *freq.entry(w.as_str()).or_default() += 1;
        }
    }

    let mut scores: HashMap<String, f64> = HashMap::new();
    for phrase in &phrases {
        let score: f64 = phrase
            .iter()
            .map(|w| degree[w.as_str()] as f64 / freq[w.as_str()] as f64)
            .sum();
        let key = phrase.join(" ");
        // 同一フレーズの重複はスコアが必ず一致するので上書きで十分。
        scores.insert(key, score);
    }

    let mut out: Vec<(String, f64)> = scores.into_iter().collect();
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scores_a_hand_checkable_two_phrase_example() {
        // "of" のみがストップワードとして効く、手計算で検証可能な例。
        // 区間1: "linear diophantine equations"（区切りなし、そのまま1フレーズ、長さ3）
        //   → 各単語 degree=3, freq=1 → phrase score = 3+3+3 = 9
        // 区間2: "minimal generating sets of solutions"
        //   "of"で分割 → "minimal generating sets"(長さ3, score=9) / "solutions"(長さ1, score=1)
        let text = "Linear diophantine equations. Minimal generating sets of solutions.";
        let scores: HashMap<String, f64> = score_document(text).into_iter().collect();

        assert_eq!(scores.get("linear diophantine equations"), Some(&9.0));
        assert_eq!(scores.get("minimal generating sets"), Some(&9.0));
        assert_eq!(scores.get("solutions"), Some(&1.0));
        assert_eq!(scores.len(), 3, "exactly 3 candidate phrases expected, got {scores:?}");
    }

    #[test]
    fn strips_old_style_arxiv_self_citations() {
        // 実データ（10,000論文のスケールテストで発見）: これらの引用が
        // "see math"のような無意味な候補として残っていた。
        assert_eq!(
            strip_arxiv_id_citations("This paper is based on a part of my PhD Thesis (see math/9912150)."),
            "This paper is based on a part of my PhD Thesis (see  )."
        );
        assert_eq!(
            strip_arxiv_id_citations("equivariant intersection cohomology for toric varieties (see math.AG/9904159)."),
            "equivariant intersection cohomology for toric varieties (see  )."
        );
        assert_eq!(
            strip_arxiv_id_citations("the case of GL(n) (see math/9801109); it relies"),
            "the case of GL(n) (see  ); it relies"
        );
    }

    #[test]
    fn strip_arxiv_id_citations_leaves_ordinary_fractions_and_words_untouched() {
        // "/"の前後にあっても7桁の数字が続かなければ引用ではない。
        assert_eq!(strip_arxiv_id_citations("the ratio x/2 and y/300 are fine"), "the ratio x/2 and y/300 are fine");
        assert_eq!(strip_arxiv_id_citations("mathematics is unaffected"), "mathematics is unaffected");
    }

    #[test]
    fn old_style_arxiv_citation_no_longer_leaves_the_archive_name_as_a_candidate() {
        let phrases = candidate_phrases(
            "This paper is based on a part of my PhD Thesis (see math/9912150).",
        );
        let all_words: Vec<&str> = phrases.iter().flatten().map(String::as_str).collect();
        assert!(!all_words.contains(&"math"), "got {all_words:?}");
    }

    #[test]
    fn strips_private_latex_macros_found_in_real_abstracts() {
        // 実データ（10,000論文へのスケールテストで発見）。バックスラッシュ+
        // 英字の並びを丸ごと1個の空白に置き換える（前後の記号はそのまま）。
        assert_eq!(strip_latex_commands(r"the polar decomposition of $SL(2,\bC)$"), "the polar decomposition of $SL(2, )$");
        assert_eq!(strip_latex_commands(r"as a module over $\SS(\fg^*)$"), "as a module over $ ( ^*)$");
        assert_eq!(strip_latex_commands(r"the limit of small noise strength ($\ep$ -> 0)"), "the limit of small noise strength ($ $ -> 0)");
    }

    #[test]
    fn strip_latex_commands_does_not_touch_backslash_accent_escapes() {
        // "\"o" のようなアクセント記法（バックスラッシュの直後が英字ではない）
        // はこの関数の対象外——`unescape_latex_accents` の仕事。
        assert_eq!(strip_latex_commands(r#"Schr\"odinger"#), r#"Schr\"odinger"#);
    }

    #[test]
    fn private_latex_macro_no_longer_leaves_a_bare_two_letter_candidate() {
        let phrases = candidate_phrases(r"We study $SL(2,\bC)$ acting on the space, following ideas from $\fg^*$.");
        let all_words: Vec<&str> = phrases.iter().flatten().map(String::as_str).collect();
        assert!(!all_words.contains(&"bc"), "got {all_words:?}");
        assert!(!all_words.contains(&"fg"), "got {all_words:?}");
    }

    #[test]
    fn unescapes_latex_accent_commands_found_in_real_abstracts() {
        assert_eq!(unescape_latex_accents(r#"Schr\"odinger equation"#), "Schrödinger equation");
        assert_eq!(unescape_latex_accents(r#"Poincar\'e conjecture"#), "Poincaré conjecture");
        assert_eq!(unescape_latex_accents(r#"G\"odel's theorem"#), "Gödel's theorem");
        assert_eq!(unescape_latex_accents(r#"K\"{a}hler manifold"#), "Kähler manifold", "braced form must also resolve");
        assert_eq!(unescape_latex_accents("plain text"), "plain text", "text without accents must be unchanged");
    }

    #[test]
    fn a_raw_latex_accent_no_longer_fractures_the_word_it_sits_inside() {
        // 実データで見つかったバグ: "Schr\"odinger operator" が
        // "schr" + "odinger" に分裂していた（\" が区切り文字扱いされたため）。
        let phrases = candidate_phrases(r#"We study the Schr\"odinger operator here."#);
        let all_words: Vec<&str> = phrases.iter().flatten().map(String::as_str).collect();
        assert!(all_words.contains(&"schrödinger"), "got {all_words:?}");
        assert!(!all_words.contains(&"schr"), "must not fracture into schr + odinger, got {all_words:?}");
        assert!(!all_words.contains(&"odinger"), "must not fracture into schr + odinger, got {all_words:?}");
    }

    #[test]
    fn breaks_phrases_at_dollar_delimited_inline_math() {
        // $...$ の中身（バックスラッシュや波括弧を含む）が候補フレーズに
        // 混入してはいけない。数式の前後で切れることだけを確認する。
        let text = "We study the group $\\mathrm{SL}_2(\\mathbb{Z})$ acting on the upper half plane.";
        let phrases = candidate_phrases(text);
        for phrase in &phrases {
            let joined = phrase.join(" ");
            assert!(
                !joined.contains('\\') && !joined.contains('{'),
                "LaTeX markup leaked into a candidate phrase: {joined:?}"
            );
        }
    }

    #[test]
    fn keeps_hyphenated_terms_as_a_single_word() {
        let phrases = candidate_phrases("This uses a p-group and a co-limit construction.");
        let all_words: Vec<&str> = phrases.iter().flatten().map(String::as_str).collect();
        assert!(all_words.contains(&"p-group"));
        assert!(all_words.contains(&"co-limit"));
    }

    #[test]
    fn drops_pure_numeric_and_single_character_tokens() {
        let phrases = candidate_phrases("The constant c 2 appears in the bound x < 3n.");
        let all_words: Vec<&str> = phrases.iter().flatten().map(String::as_str).collect();
        assert!(!all_words.contains(&"c"));
        assert!(!all_words.contains(&"2"));
        assert!(!all_words.contains(&"x"));
        assert!(!all_words.contains(&"3n"), "a digit+single-letter token carries no real concept content");
    }

    #[test]
    fn drops_hyphen_prefixed_numeric_or_single_letter_junk() {
        // 実データ: これらを候補フレーズとしてOllamaでembedding化してしまい、
        // 意味のない近傍集合ができていた。ハイフンを語の一部として許すぶん、
        // "-1" のようなトークンが「全部数字ではない」判定をすり抜けていた。
        let phrases = candidate_phrases("We show that $f(-1) = -2$ and $g(-n) = -3$, unlike $h(--) = 0$.");
        let all_words: Vec<&str> = phrases.iter().flatten().map(String::as_str).collect();
        for junk in ["-1", "-2", "-3", "-n", "--"] {
            assert!(!all_words.contains(&junk), "{junk:?} must not survive as a content word, got {all_words:?}");
        }
    }

    #[test]
    fn drops_leading_hyphen_fragments_left_by_a_stripped_math_variable() {
        // 実データ: "$p$-group" のような表記で "p" が $...$ の中に隔離され、
        // 残った "-group" だけが（先頭の p を失ったまま）候補になっていた。
        let phrases = candidate_phrases("This is a result about $p$-groups and $C^*$-algebras.");
        let all_words: Vec<&str> = phrases.iter().flatten().map(String::as_str).collect();
        assert!(!all_words.contains(&"-groups"), "got {all_words:?}");
        assert!(!all_words.contains(&"-algebras"), "got {all_words:?}");
    }

    #[test]
    fn keeps_numeral_words_that_distinguish_different_mathematical_objects() {
        // 実データ（100kコーパス）で発見: "one"/"two"/"three"がストップ
        // ワードだったせいで、genus 0/4/5/6は候補として残るのにgenus
        // 1/2/3だけが存在しなかった。数詞は概念名の一部として保持する。
        let phrases = candidate_phrases("The genus one curve. The genus two curve. The genus three curve.");
        let all: Vec<String> = phrases.iter().map(|p| p.join(" ")).collect();
        assert!(all.contains(&"genus one curve".to_string()), "got {all:?}");
        assert!(all.contains(&"genus two curve".to_string()), "got {all:?}");
        assert!(all.contains(&"genus three curve".to_string()), "got {all:?}");
    }

    #[test]
    fn drops_academic_connective_fragments_that_trail_or_lead_a_phrase() {
        // 実データ（100kコーパス）: これらの語は前置詞や補語ごと欠落した
        // 断片としてしか候補に残っておらず、それ自体が概念名になっていない
        // ことを確認済み（"approach based"155件・"following question"123件・
        // "admits"単体1206件・"corresponding eigenfunctions"39件・
        // "polynomials satisfy"10件等）。"defined"は`defining relations`
        // 等の正当な用法があるため意図的にこのリストから除いている
        // （別テスト`keeps_real_terms_...`参照）。
        let text = "The approach based here. \
                    The following property. The manifold admits a metric. \
                    The corresponding bundle. The polynomials satisfy this. \
                    The spaces having this. The pairs consisting of this. \
                    The constant depending on this. The conjecture implies this. \
                    The conditions required here.";
        let phrases = candidate_phrases(text);
        let all_words: Vec<&str> = phrases.iter().flatten().map(String::as_str).collect();
        for junk in [
            "based", "following", "admits", "corresponding", "satisfy", "having", "consisting",
            "depending", "implies", "required",
        ] {
            assert!(!all_words.contains(&junk), "{junk:?} must not survive, got {all_words:?}");
        }
    }

    #[test]
    fn keeps_real_terms_whose_head_word_overlaps_a_connective_fragment_word() {
        // 実データで頭に付く用法が正当な数学用語だと確認できた語は、
        // 同じ語幹でも意図的にストップワード化していない
        // （"associated primes"51件・"generating function"520件・
        // "defining relations"74件・"induced representations"60件・
        // "representing measure"7件・"turning point"19件・"universal
        // cover"165件・"lie algebra"1061件・"implied volatility"12件）。
        let text = "The associated primes. The generating functions. The defining relations. \
                    The induced representations. The representing measures. The turning points. \
                    The universal cover. The covering spaces. The lie algebra. The lie groups. \
                    The implied volatility.";
        let phrases = candidate_phrases(text);
        let all: Vec<String> = phrases.iter().map(|p| p.join(" ")).collect();
        for kept in [
            "associated primes",
            "generating functions",
            "defining relations",
            "induced representations",
            "representing measures",
            "turning points",
            "universal cover",
            "covering spaces",
            "lie algebra",
            "lie groups",
            "implied volatility",
        ] {
            assert!(all.contains(&kept.to_string()), "{kept:?} must survive, got {all:?}");
        }
    }
}

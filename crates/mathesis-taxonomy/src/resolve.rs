//! Entity Resolution——表記ゆれを1つの概念へ畳む（アーキテクチャ.txt 5.4 が
//! *クラスタリングの前段*に置いていた工程）。
//!
//! # なぜ独立した段階として要るか
//!
//! これまでこの工程は独立に存在せず、結果としてクラスタリングが肩代わり
//! していた。実データ（100,000論文）のクラスタを読むと:
//!
//!   cluster 22 (10件) = elliptic curves / elliptic curve / elliptic curve defined /
//!                       elliptic curves defined / fixed elliptic curve / …
//!   cluster 28 (12件) = quantum groups / quantum group / compact quantum groups / …
//!
//! アーキテクチャ.txt はこれを「『同一概念の表記ゆれ』としてこれ以上ない
//! 出力」と成功として記録しているが、逆に言えば**クラスタリングの出力枠が
//! 表記ゆれの吸収に使い切られている**ということでもある。分野の地図を
//! 作るための機構が、辞書の見出し語をまとめる仕事に費やされていた。
//!
//! そこで表記ゆれの吸収をここへ分離し、クラスタリングには「解決済みの
//! 概念」だけを渡す。
//!
//! # 何を畳み、何を畳まないか
//!
//! 畳むのは**同じ語の書き方の違いだけ**に限る:
//!
//!   大小文字            Elliptic Curves / elliptic curves
//!   ダイアクリティカル  kähler manifold / kahler manifold
//!   ハイフン・空白      zeta-function / zeta function / navier--stokes equations
//!   単数複数            elliptic curve / elliptic curves
//!
//! 語そのものが違うものは**畳まない**。"compact kähler manifold" と
//! "kähler manifold" は別の概念（コンパクト性は本物の仮定）であり、
//! "rational elliptic curves" も "elliptic curves" とは別の概念。
//! この線引きは既存の判断——`concepts.rs` が「構成語ごとの集中度で落とす
//! 規則は"fixed points"を巻き込んで失敗した」と記録し、
//! 「誤って本物の概念を消すより、個別列挙のいたちごっこの方が安全」と
//! している——と同じ側に倒している。**畳み損ねは検索結果に重複が残る
//! だけだが、畳み過ぎは別の概念を消す。**
//!
//! 正規化は辞書ではなく規則なので、単数化が英語として間違う場合がある
//! （"matrices" → "matrice"）。それでも同じ規則を全候補に適用する限り
//! **鍵としては一貫している**ので、起きるのは「畳み損ね」だけで
//! 「誤った併合」にはならない。

use std::collections::HashMap;

/// 表記ゆれを畳んだ結果の1概念。
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedConcept {
    /// 代表表記（このグループで最も文書頻度が高いもの）。
    pub representative: String,
    /// 代表以外の表記ゆれ（文書頻度の降順）。
    pub aliases: Vec<String>,
    /// グループ全体が現れた論文数の上界（各表記のdoc_freqの和。
    /// 同じ論文が複数の表記を含む場合は重複しうるので「上界」と呼ぶ——
    /// 正確な値が要るなら`paper_concepts`を引き直す必要がある）。
    pub doc_freq_sum: usize,
    /// 入力配列における各メンバーの添字（代表が先頭）。
    pub members: Vec<usize>,
}

/// 表記ゆれの正規化鍵。同じ鍵を持つフレーズが同じ概念とみなされる。
///
/// 鍵は人間に見せるものではなく突き合わせ用なので、英語として正しい
/// 単数形になっている必要はない（"matrices" → "matrice"）。必要なのは
/// **同じ概念の異表記が同じ鍵になること**と、**違う概念が同じ鍵に
/// ならないこと**の2つだけ。
pub fn canonical_key(phrase: &str) -> String {
    let folded: String = phrase
        .chars()
        .flat_map(|c| fold_char(c).chars().collect::<Vec<_>>())
        .collect();
    folded
        .split(|c: char| c.is_whitespace() || c == '-' || c == '_' || c == '/')
        .filter(|w| !w.is_empty())
        .map(|w| singularize(&fold_known_umlaut_transliteration(w)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// ドイツ語由来の数学者名・用語で、ウムラウトの音訳（ä→"ae"等）が
/// アクセント落とし（ä→"a"、`fold_char`が既に処理）とは別に、独立した
/// 表記として実データに残っている語を揃える。
///
/// 実データ（142,948論文）で確認済み: "kaehler manifolds"(72件)が
/// "kähler manifolds"(201件、`fold_char`でアクセントを落とした
/// "kahler manifolds"へ既に畳まれている)とは別の検索結果として残って
/// いた。同様に"schroedinger"系（135+110+62+…件）と"schrodinger"系、
/// "goedel"(32件)と"godel"(34件)も同じ理由で分裂していた。
///
/// **一般的な「ae→a」「oe→o」「ue→u」という文字列置換はしない。**
/// 数学の語彙には"unique"「"frequency"「"aesthetic"のように、ウムラウトとは
/// 無関係な本物のae/oe/ueを含む頻出語が多数あり、置換すると
/// `"unique"→"uniqu"`「`"frequency"→"frequncy"`のように壊れる——
/// これは畳み過ぎが実際に起こる具体例（`fold_char`のコメントが警告する
/// 「畳み過ぎは別の概念を消す」をそのまま体現する）。安全なのは、
/// 実データで確認できた具体的な語根だけを名指しすることだけ——
/// `kind_synonym`（`mathesis-fulltext::bridge`）が閉じた対応表だけを
/// 受け止めているのと同じ方針。接尾辞として判定するのは
/// "hyperkaehler"（実データで確認済み、複合語）のような派生語も
/// 同じ語根を含む限り拾うため。
fn fold_known_umlaut_transliteration(word: &str) -> String {
    const KNOWN_SUFFIXES: &[(&str, &str)] = &[
        ("kaehler", "kahler"),
        ("schroedinger", "schrodinger"),
        ("goedel", "godel"),
    ];
    for &(ae_form, a_form) in KNOWN_SUFFIXES {
        if let Some(prefix) = word.strip_suffix(ae_form) {
            return format!("{prefix}{a_form}");
        }
    }
    word.to_string()
}

/// ダイアクリティカルマークを落として小文字化する。Web側
/// （`queryIndex.ts` の正規化）と同じ方針——索引側とクエリ側で同じ
/// 畳み方をしないと、利用者が打った "kahler" が索引の "kähler" に
/// 当たらない。
fn fold_char(c: char) -> String {
    let lower = c.to_lowercase().next().unwrap_or(c);
    match lower {
        'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ā' => "a".into(),
        'é' | 'è' | 'ê' | 'ë' | 'ē' => "e".into(),
        'í' | 'ì' | 'î' | 'ï' | 'ī' => "i".into(),
        'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ō' => "o".into(),
        'ú' | 'ù' | 'û' | 'ü' | 'ū' => "u".into(),
        'ý' | 'ÿ' => "y".into(),
        'ñ' => "n".into(),
        'ç' | 'ć' | 'č' => "c".into(),
        'ø' => "o".into(),
        'ł' => "l".into(),
        'š' | 'ś' => "s".into(),
        'ž' | 'ź' | 'ż' => "z".into(),
        'ř' => "r".into(),
        'ß' => "ss".into(),
        'æ' => "ae".into(),
        'œ' => "oe".into(),
        other => other.to_string(),
    }
}

/// 英語の単数化（保守的な規則ベース）。
fn singularize(word: &str) -> String {
    // 規則が英語として破綻する少数の語だけ例外にする。"series" を
    // "ies → y" 規則に通すと "sery" になる——鍵としては一貫するので
    // 実害は無いが、数学の語彙で頻出するものは素直に扱う。
    // 規則で単数化すると壊れる語を個別に挙げる。"atlas" はここに置く——
    // 以前は「末尾が as なら複数形ではない」という一般規則にしていたが、
    // それは **"algebras" を "algebra" に畳めなくする**（評価セットが
    // "hopf algebra — hopf algebras" の漏れとして検出した）。数学の語彙
    // では -as で終わる単数形（atlas）より -as で終わる複数形
    // （algebras / formulas / areas / ideas）の方が圧倒的に多いので、
    // 一般規則ではなく例外表で扱う。
    const INVARIANT: [&str; 8] =
        ["series", "species", "lens", "axes", "indices", "vertices", "atlas", "canvas"];
    if INVARIANT.contains(&word) {
        return word.to_string();
    }
    let n = word.len();
    if n <= 3 {
        return word.to_string();
    }
    if let Some(stem) = word.strip_suffix("sses") {
        // classes → class, masses → mass
        return format!("{stem}ss");
    }
    if n > 4 {
        if let Some(stem) = word.strip_suffix("ies") {
            // varieties → variety, categories → category
            return format!("{stem}y");
        }
    }
    for suffix in ["ches", "shes", "xes", "zes"] {
        if let Some(stem) = word.strip_suffix(suffix) {
            // branches → branch, boxes → box
            return format!("{stem}{}", &suffix[..suffix.len() - 2]);
        }
    }
    if word.ends_with("ss") || word.ends_with("us") || word.ends_with("is") {
        // class / modulus / basis — 複数形ではない。
        return word.to_string();
    }
    if let Some(stem) = word.strip_suffix('s') {
        return stem.to_string();
    }
    word.to_string()
}

/// `phrases[i]` と `doc_freqs[i]` を受け取り、表記ゆれを畳んだ概念の一覧を
/// 返す。出力は文書頻度の降順（同数なら代表表記の辞書順）——実行ごとに
/// 順序が変わるとクラスタIDが変わってしまうため、決定的に並べる。
pub fn resolve(phrases: &[String], doc_freqs: &[usize]) -> Vec<ResolvedConcept> {
    debug_assert_eq!(phrases.len(), doc_freqs.len());
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, phrase) in phrases.iter().enumerate() {
        groups.entry(canonical_key(phrase)).or_default().push(i);
    }

    let mut out: Vec<ResolvedConcept> = groups
        .into_values()
        .map(|mut members| {
            // 代表は文書頻度が最大のもの。同数なら短い表記、それも同じなら
            // 辞書順——どこまでも決定的にする。
            members.sort_by(|&a, &b| {
                doc_freqs[b]
                    .cmp(&doc_freqs[a])
                    .then_with(|| phrases[a].len().cmp(&phrases[b].len()))
                    .then_with(|| phrases[a].cmp(&phrases[b]))
            });
            ResolvedConcept {
                representative: phrases[members[0]].clone(),
                aliases: members[1..].iter().map(|&i| phrases[i].clone()).collect(),
                doc_freq_sum: members.iter().map(|&i| doc_freqs[i]).sum(),
                members,
            }
        })
        .collect();

    out.sort_by(|a, b| {
        b.doc_freq_sum
            .cmp(&a.doc_freq_sum)
            .then_with(|| a.representative.cmp(&b.representative))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| canonical_key(s)).collect()
    }

    #[test]
    fn plural_and_singular_share_a_key() {
        let k = keys(&["elliptic curve", "elliptic curves"]);
        assert_eq!(k[0], k[1], "{k:?}");
        let k = keys(&["abelian variety", "abelian varieties"]);
        assert_eq!(k[0], k[1], "{k:?}");
        let k = keys(&["moduli space", "moduli spaces"]);
        assert_eq!(k[0], k[1], "{k:?}");
    }

    #[test]
    fn hyphenation_and_diacritics_and_case_share_a_key() {
        let k = keys(&["zeta function", "zeta-function", "Zeta Functions", "zeta functions"]);
        assert!(k.windows(2).all(|w| w[0] == w[1]), "{k:?}");
        let k = keys(&["kähler manifold", "kahler manifold", "Kähler Manifolds"]);
        assert!(k.windows(2).all(|w| w[0] == w[1]), "{k:?}");
        // arXivの本文には "navier--stokes"（TeXのenダッシュ）も現れる。
        let k = keys(&["navier-stokes equations", "navier--stokes equations", "navier stokes equation"]);
        assert!(k.windows(2).all(|w| w[0] == w[1]), "{k:?}");
    }

    #[test]
    fn known_umlaut_transliterations_share_a_key_with_the_accent_dropped_form() {
        // 実データ(142,948論文)で確認済みの3組: "kaehler manifolds"(72件)が
        // "kähler manifolds"→"kahler manifolds"(201件)とは別行に残って
        // いた。同様にschroedinger系・goedel。
        let k = keys(&["kaehler manifold", "kähler manifold", "kahler manifold"]);
        assert!(k.windows(2).all(|w| w[0] == w[1]), "{k:?}");
        // 複合語（実データ"hyperkaehler manifolds"）も同じ語根を含む限り拾う。
        assert_eq!(canonical_key("hyperkaehler manifolds"), canonical_key("hyperkahler manifolds"));
        assert_eq!(canonical_key("schroedinger equation"), canonical_key("schrodinger equation"));
        assert_eq!(canonical_key("goedel"), canonical_key("godel"));
    }

    #[test]
    fn umlaut_transliteration_folding_does_not_mangle_unrelated_english_words() {
        // "kind_synonym"と同じ安全側の設計——接尾辞を具体的な語根だけに
        // 限定しているので、"unique"「"frequency"のようにウムラウトとは
        // 無関係な本物のae/oe/ueを含む頻出語は変化しない。ここが壊れると
        // `singularize`の`atlas`/`series`と同種の重大な回帰になる。
        assert_eq!(canonical_key("unique factorization"), "unique factorization");
        assert_eq!(canonical_key("frequency domain"), "frequency domain");
        assert_eq!(canonical_key("sequence"), "sequence");
    }

    #[test]
    fn genuinely_different_concepts_never_share_a_key() {
        // ここが壊れると本物の概念が消える。畳み損ねより遥かに重い失敗。
        let pairs = [
            ("kähler manifold", "compact kähler manifold"),
            ("elliptic curves", "rational elliptic curves"),
            ("group", "quantum group"),
            ("class group", "group class"),
            ("finite group", "finite groups theory"),
            ("moduli space", "moduli stack"),
        ];
        for (a, b) in pairs {
            assert_ne!(canonical_key(a), canonical_key(b), "{a:?} と {b:?} を併合してはいけない");
        }
    }

    #[test]
    fn words_ending_in_ss_us_is_are_not_treated_as_plurals() {
        assert_eq!(canonical_key("equivalence class"), canonical_key("equivalence classes"));
        assert_eq!(canonical_key("modulus"), "modulus");
        assert_eq!(canonical_key("basis"), "basis");
        assert_eq!(canonical_key("atlas"), "atlas");
        // "class" が "clas" に削られていないこと。
        assert_eq!(canonical_key("class"), "class");
    }

    #[test]
    fn words_ending_in_as_are_still_pluralised() {
        // 回帰テスト。「末尾が as なら複数形ではない」という一般規則を
        // 置いていたせいで "hopf algebras" が "hopf algebra" に畳まれず、
        // 「関連概念」段階に綴り違いが漏れていた（評価セットが検出）。
        assert_eq!(canonical_key("hopf algebra"), canonical_key("hopf algebras"));
        assert_eq!(canonical_key("lie algebras"), "lie algebra");
        assert_eq!(canonical_key("formulas"), "formula");
        // それでも atlas は単数のまま。
        assert_eq!(canonical_key("atlas"), "atlas");
    }

    #[test]
    fn irregular_words_that_matter_in_mathematics_stay_intact() {
        assert_eq!(canonical_key("power series"), "power series");
        assert_eq!(canonical_key("formal power series"), "formal power series");
    }

    #[test]
    fn representative_is_the_highest_doc_freq_form() {
        let phrases = vec![
            "elliptic curve".to_string(),
            "elliptic curves".to_string(),
            "Elliptic Curves".to_string(),
            "quantum group".to_string(),
        ];
        let doc_freqs = vec![291, 412, 7, 234];
        let resolved = resolve(&phrases, &doc_freqs);
        assert_eq!(resolved.len(), 2);
        let ec = resolved.iter().find(|r| r.representative == "elliptic curves").unwrap();
        assert_eq!(ec.aliases, vec!["elliptic curve".to_string(), "Elliptic Curves".to_string()]);
        assert_eq!(ec.doc_freq_sum, 291 + 412 + 7);
        assert_eq!(ec.members[0], 1, "代表は入力添字1（doc_freq 412）");
    }

    #[test]
    fn output_order_is_deterministic() {
        let phrases: Vec<String> = ["b concept", "a concept", "c concepts", "c concept"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let doc_freqs = vec![5, 5, 3, 2];
        let first = resolve(&phrases, &doc_freqs);
        let second = resolve(&phrases, &doc_freqs);
        assert_eq!(first, second);
        // doc_freq_sum同数なら代表表記の辞書順。
        assert_eq!(first[0].representative, "a concept");
        assert_eq!(first[1].representative, "b concept");
    }

    #[test]
    fn every_input_phrase_appears_in_exactly_one_group() {
        let phrases: Vec<String> = ["zeta function", "zeta functions", "l function", "moduli space"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let doc_freqs = vec![10, 20, 5, 30];
        let resolved = resolve(&phrases, &doc_freqs);
        let mut seen: Vec<usize> = resolved.iter().flat_map(|r| r.members.clone()).collect();
        seen.sort_unstable();
        assert_eq!(seen, vec![0, 1, 2, 3]);
    }
}

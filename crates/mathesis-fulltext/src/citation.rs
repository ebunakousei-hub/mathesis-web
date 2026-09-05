//! `\bibitem`本文からarXiv IDを抽出し、`\cite`が指す引用先を論文単位で
//! 特定する（診断⑥「残っている不足」への対応——`\cite`による論文をまたぐ
//! 依存は誤結合のリスクを理由に見送っていたが、著者名・タイトルの文字列
//! 一致に頼らず**明示的な"arXiv:"記載を抽出するだけ**なら曖昧さが無い）。
//!
//! # なぜこれなら安全か
//!
//! 見送っていた理由は「著者名・タイトルの文字列一致に頼らざるを得ず
//! 誤結合のリスクが大きい」ことだった（`bridge.rs`冒頭のコメント参照）。
//! ここではその一致を一切行わない——`\bibitem`本文に著者自身が明記した
//! "arXiv:1234.56789"のような具体的なIDをそのまま読むだけで、推測も
//! 補完もしない。該当する記載が無い（著者名・誌名・年だけの伝統的な
//! 書誌情報）場合は、判定材料が無いので単に見送る——`resolve.rs`と
//! 同じ「畳み損ねは安全、誤った紐付けは危険」という判断。

/// `bibitem`本文からarXiv IDを1つ抽出する。"arXiv:"（大小文字表記ゆれ込み）
/// の直後に続く新形式（`1234.56789`）・旧形式（`math/0123456`、
/// `hep-th/9711200`等）のIDだけを対象にする——"arXiv"という明示的な語を
/// 伴わない裸の数字列は、ページ番号や巻号と区別が付かないため対象にしない。
///
/// 文字列全体を`to_lowercase()`してから検索しない——アクセント付き文字
/// （著者名に頻出）は小文字化でバイト長が変わりうるため、その後の
/// バイトオフセットが元の文字列とずれる事故を避ける。代わりに実際に
/// 観測しうる大小文字の組み合わせを直接探す。
pub fn extract_arxiv_id(bibitem_text: &str) -> Option<String> {
    for marker in ["arXiv:", "arxiv:", "ArXiv:", "Arxiv:"] {
        if let Some(pos) = bibitem_text.find(marker) {
            let rest = bibitem_text[pos + marker.len()..].trim_start();
            if let Some(id) = parse_arxiv_id_token(rest) {
                return Some(id);
            }
        }
    }
    None
}

/// 文字列の先頭から、新形式または旧形式のarXiv IDをちょうど1つ読み取る。
/// 末尾の"v2"のようなバージョン番号は結果に含めない
/// （`GraphStore::find_paper_by_arxiv_id`はバージョン無しのIDで引くため）。
fn parse_arxiv_id_token(s: &str) -> Option<String> {
    parse_new_style(s).or_else(|| parse_old_style(s))
}

/// 新形式: `\d{4}\.\d{4,5}`（例: `1234.5678`、2007年4月以降の投稿）。
fn parse_new_style(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let year_month_end = digit_run_len(bytes, 0);
    if year_month_end != 4 || bytes.get(4) != Some(&b'.') {
        return None;
    }
    let seq_len = digit_run_len(bytes, 5);
    if !(4..=5).contains(&seq_len) {
        return None;
    }
    Some(s[..5 + seq_len].to_string())
}

/// 旧形式: `[a-z-]+/\d{7}`（例: `math/0123456`、`hep-th/9711200`、
/// `alg-geom/9710014`、2007年3月以前の投稿。実データで確認した範囲では
/// サブカテゴリのドット表記は伴わない）。
fn parse_old_style(s: &str) -> Option<String> {
    let slash = s.find('/')?;
    let category = &s[..slash];
    if category.is_empty() || !category.bytes().all(|b| b.is_ascii_lowercase() || b == b'-') {
        return None;
    }
    let after = s[slash + 1..].as_bytes();
    if digit_run_len(after, 0) != 7 {
        return None;
    }
    Some(format!("{category}/{}", &s[slash + 1..slash + 8]))
}

fn digit_run_len(bytes: &[u8], start: usize) -> usize {
    bytes[start..].iter().take_while(|b| b.is_ascii_digit()).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_a_new_style_id_with_explicit_arxiv_marker() {
        assert_eq!(
            extract_arxiv_id("D. Kotschick, Four-manifolds without symplectic structures, arXiv:1234.5678."),
            Some("1234.5678".to_string())
        );
    }

    #[test]
    fn extracts_a_new_style_id_with_five_digit_sequence() {
        assert_eq!(extract_arxiv_id("see arXiv:2606.25363 for details"), Some("2606.25363".to_string()));
    }

    #[test]
    fn extracts_an_old_style_id_with_a_hyphenated_archive() {
        assert_eq!(
            extract_arxiv_id("C. LeBrun, Four-manifolds without Einstein metrics, arXiv:alg-geom/9710014."),
            Some("alg-geom/9710014".to_string())
        );
    }

    #[test]
    fn extracts_an_old_style_id_with_a_plain_archive() {
        assert_eq!(extract_arxiv_id("Preprint arXiv:math/0110329"), Some("math/0110329".to_string()));
    }

    #[test]
    fn drops_a_trailing_version_suffix() {
        assert_eq!(extract_arxiv_id("arXiv:1234.5678v2"), Some("1234.5678".to_string()));
    }

    #[test]
    fn recognizes_case_variants_of_the_marker() {
        assert_eq!(extract_arxiv_id("ArXiv:1234.5678"), Some("1234.5678".to_string()));
        assert_eq!(extract_arxiv_id("arxiv:1234.5678"), Some("1234.5678".to_string()));
    }

    #[test]
    fn returns_none_when_there_is_no_arxiv_marker() {
        // 伝統的な書誌情報（著者・誌名・年・巻号）だけの場合、判定材料が
        // 無いので見送る——著者名・タイトルの文字列一致には頼らない。
        assert_eq!(
            extract_arxiv_id("D. Kotschick, J. Morgan, and C. Taubes, Four-manifolds without symplectic structures."),
            None
        );
    }

    #[test]
    fn returns_none_for_a_bare_number_without_the_marker() {
        // ページ番号・巻号と区別が付かないので、"arXiv"を伴わない裸の
        // 数字列は対象にしない。
        assert_eq!(extract_arxiv_id("Comm. Math. Phys. 1234, 5678 (1999)"), None);
    }

    #[test]
    fn does_not_get_confused_by_accented_author_names_before_the_marker() {
        // 実データ(0704.2869系)で見た形: アクセント付き著者名がarXiv
        // マーカーより前に来る。全体をto_lowercase()せず元の文字列を
        // 直接走査するので、バイトオフセットがずれない。
        assert_eq!(
            extract_arxiv_id("Brigitte Bid\u{e9}garay-Fesquet, Static ferromagnetic materials, arXiv:1111.2421."),
            Some("1111.2421".to_string())
        );
    }

    #[test]
    fn rejects_a_malformed_new_style_id_with_too_few_digits_after_the_dot() {
        assert_eq!(extract_arxiv_id("arXiv:1234.567"), None, "3桁は新形式のシーケンス番号として短すぎる");
    }
}

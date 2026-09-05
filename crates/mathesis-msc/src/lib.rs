//! MSC2020（Mathematics Subject Classification 2020）を「概念タクソノミー・
//! エンジン」のseed ontologyとして読み込むクレート（アーキテクチャ.txt 5.1）。
//!
//! ゼロから分野階層を発見するのではなく、msc2020.org が公開している公式の
//! 機械可読データ（`data/MSC_2020.csv`、2026-09-01取得、6,603件、CC等の
//! 出所情報なしの公開データをそのまま同梱）をそのまま埋め込み、コード文字列
//! の形から親子関係を復元する。
//!
//! MSC2020のコード体系（実データで確認済み）:
//!   "00-XX"  トップレベル（63件） — 2桁の分野コード
//!   "00-01"  総称サブコード（503件） — 歴史・概説・計算手法など、
//!            どのトップレベルにも横断的に存在する定型サブコード
//!   "00Axx"  セクション（534件） — トップレベル配下の節
//!   "00A05"  リーフ（5,503件） — 実際に論文へ付与される最も細かいコード
//!
//! MSC自体は単一親の木構造であり、DAG化（概念の多重所属）はこのcrateの上に
//! 積む `mathesis-taxonomy` 側の仕事（アーキテクチャ.txt 5.2）。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::LazyLock;

const MSC_2020_CSV: &str = include_str!("../data/MSC_2020.csv");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MscLevel {
    /// "00-XX" のような2桁のトップレベル分野
    TopLevel,
    /// "00-01" のような、トップレベル配下の総称サブコード（歴史・概説等）
    Generic,
    /// "00Axx" のような3桁のセクション
    Section,
    /// "00A05" のような5桁のリーフコード
    Leaf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MscCode {
    /// 例: "18A05"
    pub code: String,
    /// 短い表示名（CSVの `text` 列）
    pub name: String,
    /// 相互参照や補足を含む説明（CSVの `description` 列。多くは `name` と同一）
    pub notes: String,
    pub level: MscLevel,
    /// 親コード。トップレベルのみ `None`。
    pub parent: Option<String>,
}

#[derive(Debug)]
pub enum MscError {
    Csv(csv::Error),
    UnrecognizedCodeShape(String),
}

impl std::fmt::Display for MscError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MscError::Csv(e) => write!(f, "MSC2020 CSV parse error: {e}"),
            MscError::UnrecognizedCodeShape(c) => {
                write!(f, "unrecognized MSC2020 code shape: {c:?}")
            }
        }
    }
}

impl std::error::Error for MscError {}

/// コード文字列の形から `(level, parent)` を復元する。
fn classify(code: &str) -> Result<(MscLevel, Option<String>), MscError> {
    let bytes = code.as_bytes();
    let is_digit = |b: u8| b.is_ascii_digit();

    if code.len() == 5 && is_digit(bytes[0]) && is_digit(bytes[1]) && bytes[2] == b'-' && bytes[3] == b'X' && bytes[4] == b'X' {
        return Ok((MscLevel::TopLevel, None));
    }
    if code.len() == 5 && is_digit(bytes[0]) && is_digit(bytes[1]) && bytes[2] == b'-' && is_digit(bytes[3]) && is_digit(bytes[4]) {
        let top = format!("{}-XX", &code[0..2]);
        return Ok((MscLevel::Generic, Some(top)));
    }
    if code.len() == 5 && is_digit(bytes[0]) && is_digit(bytes[1]) && bytes[2].is_ascii_alphabetic() && bytes[3] == b'x' && bytes[4] == b'x' {
        let top = format!("{}-XX", &code[0..2]);
        return Ok((MscLevel::Section, Some(top)));
    }
    if code.len() == 5 && is_digit(bytes[0]) && is_digit(bytes[1]) && bytes[2].is_ascii_alphabetic() && is_digit(bytes[3]) && is_digit(bytes[4]) {
        let section = format!("{}xx", &code[0..3]);
        return Ok((MscLevel::Leaf, Some(section)));
    }
    Err(MscError::UnrecognizedCodeShape(code.to_string()))
}

fn parse_all(csv_text: &str) -> Result<Vec<MscCode>, MscError> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b'\t')
        .from_reader(csv_text.as_bytes());

    let mut out = Vec::with_capacity(6_700);
    for record in reader.records() {
        let record = record.map_err(MscError::Csv)?;
        let code = record.get(0).unwrap_or_default().to_string();
        let name = record.get(1).unwrap_or_default().to_string();
        let notes = record.get(2).unwrap_or_default().to_string();
        let (level, parent) = classify(&code)?;
        out.push(MscCode {
            code,
            name,
            notes,
            level,
            parent,
        });
    }
    Ok(out)
}

struct Table {
    codes: Vec<MscCode>,
    by_code: HashMap<String, usize>,
    children: HashMap<String, Vec<usize>>,
}

static TABLE: LazyLock<Table> = LazyLock::new(|| {
    let codes = parse_all(MSC_2020_CSV).expect("bundled MSC_2020.csv must parse");
    let by_code: HashMap<String, usize> = codes
        .iter()
        .enumerate()
        .map(|(i, c)| (c.code.clone(), i))
        .collect();
    let mut children: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, c) in codes.iter().enumerate() {
        if let Some(parent) = &c.parent {
            children.entry(parent.clone()).or_default().push(i);
        }
    }
    Table {
        codes,
        by_code,
        children,
    }
});

/// 全MSC2020コード（63トップレベル+503総称+534セクション+5,503リーフ）
pub fn all() -> &'static [MscCode] {
    &TABLE.codes
}

pub fn by_code(code: &str) -> Option<&'static MscCode> {
    TABLE.by_code.get(code).map(|&i| &TABLE.codes[i])
}

/// 直下の子コードのみ返す（孫以下は含まない）
pub fn children_of(code: &str) -> Vec<&'static MscCode> {
    TABLE
        .children
        .get(code)
        .map(|idxs| idxs.iter().map(|&i| &TABLE.codes[i]).collect())
        .unwrap_or_default()
}

/// 63件のトップレベル分野
pub fn top_level() -> Vec<&'static MscCode> {
    TABLE
        .codes
        .iter()
        .filter(|c| c.level == MscLevel::TopLevel)
        .collect()
}

/// 任意のコードから根（トップレベル）までの祖先チェーンを返す（自分自身を含む、根が先頭）
pub fn ancestor_chain(code: &str) -> Vec<&'static MscCode> {
    let mut chain = Vec::new();
    let mut current = by_code(code);
    while let Some(c) = current {
        chain.push(c);
        current = c.parent.as_deref().and_then(by_code);
    }
    chain.reverse();
    chain
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_the_real_bundled_msc2020_data() {
        let codes = all();
        // msc2020.orgから2026-09-01に取得した実データの行数（ヘッダ除く）
        assert_eq!(codes.len(), 6603);
    }

    #[test]
    fn has_exactly_63_top_level_fields() {
        assert_eq!(top_level().len(), 63);
    }

    #[test]
    fn category_theory_top_level_is_present_with_correct_name() {
        let c = by_code("18-XX").expect("18-XX (category theory) must exist");
        assert_eq!(c.level, MscLevel::TopLevel);
        assert!(c.name.contains("Category theory"));
        assert!(c.parent.is_none());
    }

    #[test]
    fn leaf_code_resolves_full_ancestor_chain_to_top_level() {
        // 18A05 = "Definitions and generalizations" (category theory section)
        let leaf = by_code("18A05").expect("18A05 must exist in real data");
        assert_eq!(leaf.level, MscLevel::Leaf);
        assert_eq!(leaf.parent.as_deref(), Some("18Axx"));

        let chain = ancestor_chain("18A05");
        let chain_codes: Vec<&str> = chain.iter().map(|c| c.code.as_str()).collect();
        assert_eq!(chain_codes, vec!["18-XX", "18Axx", "18A05"]);
    }

    #[test]
    fn section_children_of_top_level_include_known_category_theory_sections() {
        let children = children_of("18-XX");
        let codes: Vec<&str> = children.iter().map(|c| c.code.as_str()).collect();
        assert!(codes.contains(&"18Axx"), "18-XX should have section child 18Axx, got {codes:?}");
        // 総称サブコード(18-01等)もトップレベルの子として拾える
        assert!(codes.contains(&"18-01"));
    }

    #[test]
    fn every_non_top_level_code_has_a_parent_present_in_the_table() {
        for c in all() {
            if let Some(parent) = &c.parent {
                assert!(
                    by_code(parent).is_some(),
                    "{}'s parent {} is missing from the table",
                    c.code,
                    parent
                );
            }
        }
    }

    #[test]
    fn no_duplicate_codes() {
        let mut seen = std::collections::HashSet::new();
        for c in all() {
            assert!(seen.insert(c.code.as_str()), "duplicate MSC code: {}", c.code);
        }
    }
}

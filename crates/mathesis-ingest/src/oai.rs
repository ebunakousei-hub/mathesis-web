//! arXivのOAI-PMHインターフェース（`https://export.arxiv.org/oai2`）から
//! `arXivRaw` メタデータ（title/abstract/authors/categories/msc-class）を
//! 収集する。Atom検索APIではなくOAI-PMHを使うのは、`resumptionToken` による
//! 素直なページングと、`msc-class` フィールドを直接持つ点で、
//! アーキテクチャ.txt 5.8 Phase 1（数万〜数十万件規模）の一括収集に向くため。

use crate::model::Paper;
use anyhow::{anyhow, Context, Result};
use roxmltree::{Document, Node};
use std::thread::sleep;
use std::time::Duration;

const OAI_BASE: &str = "https://export.arxiv.org/oai2";

/// 1回の収集を識別する引数。増分収集（`from`/`until`）と再開に対応する。
#[derive(Debug, Clone)]
pub struct HarvestRequest {
    /// OAI-PMHのセット名（例: "math"、"math:math:CT"）
    pub set: String,
    /// この日付以降に更新されたレコードだけを取る（YYYY-MM-DD）。
    ///
    /// これが無かった頃、コーパスを最新に保つ唯一の方法は**毎回全件を
    /// 取り直す**ことだった。10万件で21.9分、100万件なら3時間半を、
    /// 数百件の新着を取り込むためだけに毎回払うことになる。OAI-PMHは
    /// 元から `from` による差分取得を持っているので、それを使う。
    pub from: Option<String>,
    /// この日付までのレコードだけを取る（YYYY-MM-DD）。
    pub until: Option<String>,
    /// 取得件数の上限（検証用）。
    pub max: Option<usize>,
    /// 中断した収集の続きから始めるための `resumptionToken`。
    pub resume_token: Option<String>,
    /// 再開時点で既に収集済みの件数（`max` と進捗表示の基準に使う）。
    pub already_harvested: usize,
}

impl HarvestRequest {
    pub fn new(set: impl Into<String>) -> Self {
        Self {
            set: set.into(),
            from: None,
            until: None,
            max: None,
            resume_token: None,
            already_harvested: 0,
        }
    }

    /// 収集の同一性を表すキー。`set` と期間が同じなら同じ収集の続きと見なす
    /// （別の期間を取りに行ったときに、前回のトークンを誤って使い回さない）。
    pub fn key(&self) -> String {
        format!(
            "{}|{}|{}",
            self.set,
            self.from.as_deref().unwrap_or(""),
            self.until.as_deref().unwrap_or("")
        )
    }
}

/// OAI-PMHから収集する。**ページごとにコールバックを呼ぶストリーミング形式**。
///
/// 以前は全ページをメモリに溜めてから呼び出し側が一括保存していたため、
/// 途中で失敗するとそれまでの収集が丸ごと失われた（embedding生成で実際に
/// 起きたのと同じ「最後にしか永続化しない」失敗パターン。10万件中39,400件
/// でクラッシュして全損した）。現在は1ページごとに呼び出し側へ渡して
/// 保存させ、同時に次の `resumptionToken` も渡すので、中断したところから
/// 再開できる。
///
/// `on_page` は `(そのページの論文, 次のresumptionToken, 累計件数, 取り切ったか)`
/// を受け取る。`--max` で打ち切った場合は「取り切っていない」ので、
/// トークンはそのまま渡す——次回 `--resume` で続きから取れるようにするため
/// （100万件規模のコーパスを毎回少しずつ伸ばす運用がこれで成立する）。
/// 戻り値は収集した総件数（再開分を含む）。
pub fn harvest_streaming(
    request: &HarvestRequest,
    mut on_page: impl FnMut(&[Paper], Option<&str>, usize, bool) -> Result<()>,
) -> Result<usize> {
    let mut total = request.already_harvested;
    let mut resumption: Option<String> = request.resume_token.clone();

    loop {
        let body = fetch_page(request, resumption.as_deref())?;
        let doc = Document::parse(&body).context("OAI-PMH応答のXMLパースに失敗")?;

        if let Some(err) = doc.descendants().find(|n| n.has_tag_name("error")) {
            let code = err.attribute("code").unwrap_or("unknown");
            let text = err.text().unwrap_or("").trim();
            // noRecordsMatch は「差分が無かった」だけで失敗ではない
            // （増分収集では最も普通の結果）。
            if code == "noRecordsMatch" {
                on_page(&[], None, total, true)?;
                return Ok(total);
            }
            return Err(anyhow!("OAI-PMHエラー [{code}]: {text}"));
        }

        let mut page: Vec<Paper> = Vec::new();
        let mut reached_cap = false;
        for record in doc.descendants().filter(|n| n.has_tag_name("record")) {
            if let Some(paper) = parse_record(record) {
                page.push(paper);
            }
            if let Some(cap) = request.max {
                if total + page.len() >= cap {
                    reached_cap = true;
                    break;
                }
            }
        }
        total += page.len();

        let token = doc
            .descendants()
            .find(|n| n.has_tag_name("resumptionToken"))
            .and_then(|n| n.text())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        let exhausted = token.is_none();
        on_page(&page, token.as_deref(), total, exhausted)?;

        if reached_cap {
            break;
        }
        match token {
            Some(t) => resumption = Some(t),
            None => break,
        }
    }

    Ok(total)
}

/// 一時的なネットワーク障害を何回まで待って再試行するか。
///
/// 以前は503（レート制限）だけを扱い、接続リセットやタイムアウトのような
/// 一時的な失敗はその場で収集全体を失敗させていた。数時間かかる収集で
/// 1回の瞬断が全損に繋がるのは割に合わない。
///
/// 実データ（500k論文への拡張、322,348件収集した時点）で発覚した重要な
/// バグの再現: `req.call()`自体は成功し（`Ok(resp)`）、その**後**の
/// `resp.into_string()`（応答本体の読み取り）中に接続がリセットされる
/// （Windowsの`os error 10054`）ケースを、この定数によるリトライが
/// 一切カバーしていなかった——`Ok(resp) => return Ok(resp.into_string()?)`
/// の`?`がそのまま関数全体を失敗させ、数十分かけて集めた進捗ごと
/// 収集全体を落としていた。OAI-PMHの1ページが数MB（実測2.6MB）ある以上、
/// 本文読み取り中の瞬断は接続確立時の失敗と同じくらい起こりうる。
const MAX_TRANSIENT_RETRIES: u32 = 5;

fn fetch_page(request: &HarvestRequest, resumption: Option<&str>) -> Result<String> {
    let mut transient_failures = 0u32;
    loop {
        // resumptionToken を使うときは、OAI-PMHの規約上、他の絞り込み引数を
        // 一緒に送ってはいけない（トークン側がセットも期間も保持している）。
        let req = match resumption {
            Some(tok) => ureq::get(OAI_BASE)
                .query("verb", "ListRecords")
                .query("resumptionToken", tok),
            None => {
                let mut r = ureq::get(OAI_BASE)
                    .query("verb", "ListRecords")
                    .query("metadataPrefix", "arXivRaw")
                    .query("set", &request.set);
                if let Some(from) = &request.from {
                    r = r.query("from", from);
                }
                if let Some(until) = &request.until {
                    r = r.query("until", until);
                }
                r
            }
        };

        match req.call() {
            Ok(resp) => match resp.into_string() {
                Ok(body) => return Ok(body),
                // 接続確立には成功したが、本文（数MBのXML）を読み切る前に
                // 切れたケース。上のコメント参照——接続確立時の失敗と
                // 同じ扱いにする。
                Err(e) if transient_failures < MAX_TRANSIENT_RETRIES => {
                    transient_failures += 1;
                    let wait = 2u64.pow(transient_failures);
                    eprintln!(
                        "  … 応答本体の読み取り中に通信エラー（{transient_failures}回目、{wait}秒後に再試行）: {e}"
                    );
                    sleep(Duration::from_secs(wait));
                }
                Err(e) => return Err(anyhow!("応答本体の読み取りに失敗: {e}")),
            },
            Err(ureq::Error::Status(503, resp)) => {
                let wait = resp
                    .header("Retry-After")
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(15);
                eprintln!("  … arXivにレート制限され、{wait}秒待機します");
                sleep(Duration::from_secs(wait));
            }
            Err(ureq::Error::Transport(t)) if transient_failures < MAX_TRANSIENT_RETRIES => {
                transient_failures += 1;
                // 指数バックオフ（2,4,8,16,32秒）。
                let wait = 2u64.pow(transient_failures);
                eprintln!("  … 通信エラー（{transient_failures}回目、{wait}秒後に再試行）: {t}");
                sleep(Duration::from_secs(wait));
            }
            Err(e) => return Err(anyhow!("OAI-PMHリクエスト失敗: {e}")),
        }
    }
}

fn parse_record(record: Node) -> Option<Paper> {
    let raw = record
        .descendants()
        .find(|n| n.has_tag_name("arXivRaw"))?;

    let text_of = |tag: &str| -> Option<String> {
        raw.children()
            .find(|n| n.has_tag_name(tag))
            .and_then(|n| n.text())
            .map(|s| s.trim().to_string())
    };

    let arxiv_id = text_of("id")?;
    let title = text_of("title").unwrap_or_default();
    let abstract_text = text_of("abstract").unwrap_or_default();
    let authors_raw = text_of("authors").unwrap_or_default();
    let categories_raw = text_of("categories").unwrap_or_default();
    let msc_raw = text_of("msc-class").unwrap_or_default();

    let submitted = raw
        .children()
        .find(|n| n.has_tag_name("version"))
        .and_then(|v| v.children().find(|n| n.has_tag_name("date")))
        .and_then(|n| n.text())
        .unwrap_or_default()
        .to_string();

    Some(Paper {
        arxiv_id,
        title,
        abstract_text,
        authors: split_authors(&authors_raw),
        categories: categories_raw
            .split_whitespace()
            .map(str::to_string)
            .collect(),
        msc_codes: split_msc_codes(&msc_raw),
        submitted,
    })
}

/// `msc-class` フィールドの自由記述を個々のMSCコードへ分割する。
///
/// 実データを見ると区切り方がかなり揺れている（例:
/// "18D05; 46L08" / "18D10, 18E20" / "18B25 12F10"（カンマなし・空白のみ）/
/// "16E40 (Primary) 17B55 17B67 (Secondary)" / "19D23(primary)"（空白なしで
/// 括弧が直接続く））。コード自体は内部に空白や括弧を含まないため、カンマ・
/// セミコロン・空白・括弧のいずれも区切り文字として扱い、"primary"/
/// "secondary" 注記だけを取り除く。
fn split_msc_codes(raw: &str) -> Vec<String> {
    raw.split(|c: char| matches!(c, ',' | ';' | '(' | ')') || c.is_whitespace())
        .filter(|tok| !tok.is_empty())
        .filter(|tok| !tok.eq_ignore_ascii_case("primary") && !tok.eq_ignore_ascii_case("secondary"))
        .map(str::to_string)
        .collect()
}

/// "First Last, First2 Last2 and First3 Last3" のようなarXivの著者表記を分割する
fn split_authors(raw: &str) -> Vec<String> {
    raw.replace(" and ", ", ")
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../tests/fixtures/oai_list_records_sample.xml");

    #[test]
    fn parses_real_arxivraw_records_from_a_captured_response() {
        let doc = Document::parse(SAMPLE).unwrap();
        let records: Vec<Paper> = doc
            .descendants()
            .filter(|n| n.has_tag_name("record"))
            .filter_map(parse_record)
            .collect();

        assert!(
            records.len() >= 2,
            "captured fixture should contain at least a couple of records"
        );

        let hines = records
            .iter()
            .find(|p| p.arxiv_id == "cs/9812019")
            .expect("cs/9812019 must be present in the captured fixture");
        assert_eq!(hines.title, "Symmetries and transitions of bounded Turing machines");
        assert_eq!(hines.authors, vec!["Peter M. Hines"]);
        assert!(hines.categories.contains(&"math.CT".to_string()));
        assert!(hines.msc_codes.is_empty(), "this paper has no author-supplied msc-class");

        let landsman = records
            .iter()
            .find(|p| p.title.starts_with("Bicategories of operator algebras"))
            .expect("Landsman paper must be present in the captured fixture");
        assert_eq!(
            landsman.msc_codes,
            vec!["18D05", "46L08", "22A22", "53D17"],
            "semicolon-separated msc-class must split into individual codes"
        );
    }

    #[test]
    fn splits_msc_codes_across_the_real_separator_styles_seen_in_the_wild() {
        // 実際にmath:math:CTを収集して見つかった表記ゆれ（アーキテクチャ.txt
        // 5.8 Phase 1の実行時に発覚）。
        assert_eq!(split_msc_codes("18D05; 46L08; 22A22; 53D17"), vec!["18D05", "46L08", "22A22", "53D17"]);
        assert_eq!(split_msc_codes("18D10, 18E20"), vec!["18D10", "18E20"]);
        assert_eq!(split_msc_codes("18B25 12F10"), vec!["18B25", "12F10"], "space-only separator, no comma");
        assert_eq!(
            split_msc_codes("16E40 (Primary) 17B55 17B67 (Secondary)"),
            vec!["16E40", "17B55", "17B67"],
            "(Primary)/(Secondary) annotations must be dropped, not kept as tokens"
        );
        assert_eq!(
            split_msc_codes("18D10, 19D23 (primary) 18D50 (secondary)"),
            vec!["18D10", "19D23", "18D50"],
            "lowercase primary/secondary must also be dropped"
        );
        assert_eq!(
            split_msc_codes("19D23(primary) 18D50(secondary)"),
            vec!["19D23", "18D50"],
            "parenthesis directly touching the code with no space must still split"
        );
    }

    #[test]
    fn splits_authors_on_commas_and_a_trailing_and() {
        assert_eq!(
            split_authors("Maxim Kontsevich, Yan Soibelman"),
            vec!["Maxim Kontsevich", "Yan Soibelman"]
        );
        assert_eq!(
            split_authors("A. One, B. Two and C. Three"),
            vec!["A. One", "B. Two", "C. Three"]
        );
    }
}

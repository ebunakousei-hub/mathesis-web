//! P4: OpenAlexからの引用スナップショット取得（`docs/P4_PLAN.md`）。
//!
//! ARCHITECTURE_NEXT.md §3の「最初のマイルストーンとして万能クローラを
//! 作らない」という境界を守るため、OpenAlexの全件クロールはしない——
//! 既にカタログにある論文（`mathesis-graph`の`papers`テーブル、実データで
//! 138件）を種として、その各論文自身のOpenAlex Workレコード**だけ**を
//! 取得する（雪だるま式収集、`docs/P4_PLAN.md`参照）。
//!
//! `mathesis-fulltext::source`と同じ設計:ネットワークに触れる関数
//! （`fetch_work_by_arxiv_id`）はテストしない（ライブAPIへの依存を単体
//! テストに持ち込まない）。純粋な変換関数（`arxiv_doi`/`bare_work_id`/
//! `canonical_content_hash`）だけをテストする。

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::thread::sleep;
use std::time::Duration;

const OPENALEX_WORKS_BASE: &str = "https://api.openalex.org/works";
const USER_AGENT: &str = "Mathesis research bot (contact: ebunakousei@hotmail.com)";

/// OpenAlexの「polite pool」は連絡先入りUser-Agentで秒間十数リクエスト
/// までを許容している（arXivのe-print取得ほど厳しくない）。実データでの
/// 種論文数(138件)なら200msでも数十秒で終わる——arXiv本体への負荷は
/// 発生しない(OpenAlexのAPIサーバへのアクセスのみ)。
pub const COURTESY_DELAY: Duration = Duration::from_millis(200);

pub fn courtesy_wait() {
    sleep(COURTESY_DELAY);
}

/// arXivは2022年2月以降、全投稿(過去分含め遡って)に`10.48550/arXiv.<id>`
/// 形式のDOIを発行済み——実測(2101.00001で確認)でOpenAlexはこのDOIで
/// 引ける。OpenAlexの`Work.ids`には`arxiv`という直接キーは無い
/// (実測で確認: `ids`は`openalex`/`doi`/`mag`/`pmid`/`pmcid`のみ)ため、
/// DOI経由の解決が唯一の直接ルート。
pub fn arxiv_doi(arxiv_id: &str) -> String {
    format!("10.48550/arXiv.{arxiv_id}")
}

/// OpenAlexのid/referenced_worksは`"https://openalex.org/W123"`という
/// フルURLで来る。スナップショットには裸のid(`"W123"`)だけを持たせる
/// ——保存量を減らし、`work_to_arxiv`の照合をURL文字列比較に依存させない。
pub fn bare_work_id(full_url_or_id: &str) -> &str {
    full_url_or_id.rsplit('/').next().unwrap_or(full_url_or_id)
}

#[derive(Debug, Clone, Deserialize)]
struct RawWork {
    id: String,
    doi: Option<String>,
    title: Option<String>,
    #[serde(default)]
    referenced_works: Vec<String>,
}

/// 取得結果からスナップショットへ書き出す最小レコード。`raw_content_hash`は
/// OpenAlex APIの生JSON全体ではなく、**実際に使うフィールドだけ**を正規化
/// して取ったハッシュ——OpenAlexが未使用フィールドを変更しただけで
/// リビジョンが変わってしまう(実際には何も変わっていないのに新しい
/// SourceRecordが増える)のを避ける、意図的な選択。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotEntry {
    pub arxiv_id: String,
    pub work_id: String,
    pub doi: Option<String>,
    pub title: Option<String>,
    pub referenced_work_ids: Vec<String>,
    pub retrieved_at_unix: i64,
}

impl SnapshotEntry {
    /// このエントリの内容だけから決定的に計算する——`entities`/
    /// `source_records`と同じ「無い情報を捏造しない」規律の一部として、
    /// フィールドの並びを固定した手書きの正規化(serde_jsonの
    /// キー順ではなく)で再現性を保証する。
    pub fn canonical_content_hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.arxiv_id.as_bytes());
        hasher.update([0u8]);
        hasher.update(self.work_id.as_bytes());
        hasher.update([0u8]);
        hasher.update(self.doi.as_deref().unwrap_or("").as_bytes());
        hasher.update([0u8]);
        hasher.update(self.title.as_deref().unwrap_or("").as_bytes());
        hasher.update([0u8]);
        let mut refs = self.referenced_work_ids.clone();
        refs.sort();
        for r in &refs {
            hasher.update(r.as_bytes());
            hasher.update([0u8]);
        }
        let digest = hasher.finalize();
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        format!("sha256:{hex}")
    }
}

/// 1論文のOpenAlex Workレコードを取得する。呼び出し側は`courtesy_wait()`で
/// 間を空けてから次を呼ぶこと(`fetch-openalex` CLI参照)。
///
/// `Ok(None)`はarXiv DOIがOpenAlexに存在しない場合(404) —— arXiv自身が
/// DOIを持たない極めて古い投稿、または取り下げ等。エラーとして扱わない
/// (`mathesis-fulltext::source::fetch_source`の`NoSource`と同じ方針)。
pub fn fetch_work_by_arxiv_id(arxiv_id: &str, retrieved_at_unix: i64) -> Result<Option<SnapshotEntry>> {
    let doi = arxiv_doi(arxiv_id);
    let url = format!("{OPENALEX_WORKS_BASE}/https://doi.org/{doi}");
    let resp = ureq::get(&url).set("User-Agent", USER_AGENT).timeout(Duration::from_secs(30)).call();

    let body = match resp {
        Ok(r) => r.into_string().context("OpenAlex応答本体の読み取りに失敗")?,
        Err(ureq::Error::Status(404, _)) => return Ok(None),
        Err(e) => return Err(anyhow!("{arxiv_id} のOpenAlex取得に失敗: {e}")),
    };
    let raw: RawWork = serde_json::from_str(&body).with_context(|| format!("{arxiv_id} のOpenAlex応答のパースに失敗"))?;

    Ok(Some(SnapshotEntry {
        arxiv_id: arxiv_id.to_string(),
        work_id: bare_work_id(&raw.id).to_string(),
        doi: raw.doi,
        title: raw.title,
        referenced_work_ids: raw.referenced_works.iter().map(|w| bare_work_id(w).to_string()).collect(),
        retrieved_at_unix,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arxiv_doi_uses_the_2022_retroactive_doi_prefix() {
        assert_eq!(arxiv_doi("2101.00001"), "10.48550/arXiv.2101.00001");
    }

    #[test]
    fn bare_work_id_strips_the_openalex_url_prefix() {
        assert_eq!(bare_work_id("https://openalex.org/W3120042730"), "W3120042730");
        assert_eq!(bare_work_id("W3120042730"), "W3120042730", "既に裸のidならそのまま");
    }

    fn entry(title: &str, refs: &[&str]) -> SnapshotEntry {
        SnapshotEntry {
            arxiv_id: "math/0001".into(),
            work_id: "W1".into(),
            doi: Some("10.48550/arXiv.math.0001".into()),
            title: Some(title.into()),
            referenced_work_ids: refs.iter().map(|s| s.to_string()).collect(),
            retrieved_at_unix: 0,
        }
    }

    #[test]
    fn canonical_hash_is_deterministic_and_order_independent_for_references() {
        let a = entry("Title", &["W2", "W3"]);
        let b = entry("Title", &["W3", "W2"]);
        assert_eq!(a.canonical_content_hash(), b.canonical_content_hash(), "参照リストの順序はハッシュに影響しない");
    }

    #[test]
    fn canonical_hash_changes_when_meaningful_content_changes() {
        let a = entry("Title", &["W2"]);
        let b = entry("Different Title", &["W2"]);
        let c = entry("Title", &["W2", "W4"]);
        assert_ne!(a.canonical_content_hash(), b.canonical_content_hash(), "タイトル変更はリビジョンを変えるべき");
        assert_ne!(a.canonical_content_hash(), c.canonical_content_hash(), "参照集合の変更はリビジョンを変えるべき");
    }

    #[test]
    fn retrieved_at_unix_does_not_affect_the_content_hash() {
        let mut a = entry("Title", &["W2"]);
        let mut b = a.clone();
        a.retrieved_at_unix = 100;
        b.retrieved_at_unix = 200;
        assert_eq!(a.canonical_content_hash(), b.canonical_content_hash(), "取得時刻はコンテンツの一部ではない");
    }
}

/// アーキテクチャ.txt 5.3 の `Paper` に対応。Concept抽出（層6のPhase 2以降）は
/// まだ行わないため `concepts` フィールドはここでは持たない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paper {
    pub arxiv_id: String,
    pub title: String,
    pub abstract_text: String,
    pub authors: Vec<String>,
    /// arXivの分類（"math.CT" 等）。MSC2020そのものではないが、Phase 5の
    /// alignment（アーキテクチャ.txt 5.8）で使う手がかりとして保持する。
    pub categories: Vec<String>,
    /// 著者が自己申告したMSC2020コード（例: "18D05"）。任意項目のため
    /// 空のことが多い。
    pub msc_codes: Vec<String>,
    /// 最初のバージョンの投稿日時（arXivRaw の生の日付文字列をそのまま保持）
    pub submitted: String,
}

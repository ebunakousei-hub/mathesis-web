//! arXivのe-print（LaTeXソース）取得（アーキテクチャ.txt 5.8 Phase 8:
//! 「full text・theorem・proof dependencyまで拡張」）。
//!
//! PDFのテキスト抽出ではなくLaTeXソースを使う——定理・証明環境は
//! `\begin{theorem}...\end{theorem}`のように構造化されたマークアップとして
//! 残っており、PDFのレイアウト後テキストから同じ情報を復元するより
//! はるかに正確に取れる。
//!
//! `https://arxiv.org/e-print/{id}` は実際に叩いて確認したところ
//! （2件のサンプルで実測）、次の3パターンがありうる:
//!
//!   1. gzip圧縮された単一の.texファイル（1つの.texファイルだけの投稿）
//!   2. gzip圧縮されたtarアーカイブ（複数ファイル・図版込みの投稿）
//!   3. PDFそのもの（LaTeXソースを提供していない投稿——`%PDF`で始まる）
//!
//! 3のケースは「ソース無し」として扱い、エラーにはしない（実際の投稿の
//! かなりの割合がこれに該当することを想定——正確な割合は`fetch-sources`
//! の実行結果で報告する）。

use anyhow::{anyhow, Context, Result};
use flate2::read::GzDecoder;
use std::io::Read;
use std::thread::sleep;
use std::time::Duration;

const EPRINT_BASE: &str = "https://arxiv.org/e-print";
// arXivの一括アクセスに関する利用規約は「短い間隔で連続アクセスしない」
// ことを求めている（OAI-PMHのresumptionTokenページングと同じ配慮）。
// 1論文ごとに固定で数秒空ける（`oai.rs`の503/Retry-After待機とは別の、
// 事前に決め打ちの礼儀的ウェイト——e-printエンドポイントは503を返さず
// 単に細い帯域で応答が遅くなるだけなのを実測で確認したため）。
pub const COURTESY_DELAY: Duration = Duration::from_secs(3);

pub enum FetchedSource {
    /// 展開済みの.texファイル群（ファイル名, 内容）。
    Tex(Vec<(String, String)>),
    /// PDFのみで、LaTeXソースの提供が無かった。
    NoSource,
}

/// 1論文のe-printを取得し、.texファイル群に展開する。呼び出し側は
/// `COURTESY_DELAY`だけ間を空けてから次を呼ぶこと（`fetch-sources` CLI参照）。
pub fn fetch_source(arxiv_id: &str) -> Result<FetchedSource> {
    let url = format!("{EPRINT_BASE}/{arxiv_id}");
    let resp = ureq::get(&url)
        .set("User-Agent", "Mathesis research bot (contact: ebunakousei@hotmail.com)")
        .timeout(Duration::from_secs(60))
        .call();

    let bytes = match resp {
        Ok(r) => {
            let mut buf = Vec::new();
            r.into_reader().read_to_end(&mut buf).context("応答本体の読み取りに失敗")?;
            buf
        }
        // 404 = そのIDのソースが存在しない（PDFのみ、または取り下げ）。
        Err(ureq::Error::Status(404, _)) => return Ok(FetchedSource::NoSource),
        Err(e) => return Err(anyhow!("{arxiv_id} のソース取得に失敗: {e}")),
    };

    unpack(&bytes)
}

fn unpack(bytes: &[u8]) -> Result<FetchedSource> {
    if bytes.starts_with(b"%PDF") {
        return Ok(FetchedSource::NoSource);
    }
    if bytes.len() < 2 || bytes[0] != 0x1f || bytes[1] != 0x8b {
        return Err(anyhow!("gzip/PDFのいずれでもない未知のフォーマット（先頭バイト: {:?}）", &bytes[..bytes.len().min(8)]));
    }

    let mut decoder = GzDecoder::new(bytes);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed).context("gzip展開に失敗")?;

    // tarアーカイブかどうかは、まず読んでみて中身のエントリが取れるかで
    // 判定する（マジックバイトの手打ち判定より`tar`クレートに任せる方が
    // 確実——ustarのマジックが無い古い形式のtarも稀に存在するため）。
    if let Some(files) = try_extract_tar(&decompressed) {
        if !files.is_empty() {
            return Ok(FetchedSource::Tex(files));
        }
    }

    // tarではなかった（か、.texファイルが1つも取れなかった）場合は、
    // 展開結果そのものを単一の.texファイルとして扱う（実データで確認した
    // 最も多いケース——1ファイルだけの投稿はgzip単体で来る）。
    let text = String::from_utf8_lossy(&decompressed).into_owned();
    Ok(FetchedSource::Tex(vec![("source.tex".to_string(), text)]))
}

fn try_extract_tar(bytes: &[u8]) -> Option<Vec<(String, String)>> {
    let mut archive = tar::Archive::new(bytes);
    let entries = archive.entries().ok()?;

    let mut files = Vec::new();
    for entry in entries {
        let mut entry = entry.ok()?;
        let path = entry.path().ok()?.to_string_lossy().into_owned();
        if !path.ends_with(".tex") {
            continue;
        }
        let mut content = String::new();
        if entry.read_to_string(&mut content).is_err() {
            continue; // バイナリ混入や非UTF-8ファイル名など、個別ファイルの失敗は無視して続行
        }
        files.push((path, content));
    }
    Some(files)
}

/// 呼び出し側（CLI）が礼儀的ウェイトを挟むためのヘルパー。
pub fn courtesy_wait() {
    sleep(COURTESY_DELAY);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn unpack_recognizes_a_bare_pdf_as_no_source() {
        let bytes = b"%PDF-1.5\n...";
        match unpack(bytes).unwrap() {
            FetchedSource::NoSource => {}
            FetchedSource::Tex(_) => panic!("PDFはNoSourceとして扱われるべき"),
        }
    }

    #[test]
    fn unpack_treats_a_gzipped_single_file_as_one_tex_document() {
        let tex = "\\documentclass{article}\n\\begin{document}Hi\\end{document}";
        let gz = gzip(tex.as_bytes());
        match unpack(&gz).unwrap() {
            FetchedSource::Tex(files) => {
                assert_eq!(files.len(), 1);
                assert_eq!(files[0].1, tex);
            }
            FetchedSource::NoSource => panic!("gzip単体は本文が取れるべき"),
        }
    }

    #[test]
    fn unpack_extracts_tex_files_from_a_gzipped_tar_archive() {
        let mut tar_bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_bytes);
            let tex_content = b"\\documentclass{article}\n";
            let mut header = tar::Header::new_gnu();
            header.set_size(tex_content.len() as u64);
            header.set_cksum();
            builder.append_data(&mut header, "main.tex", &tex_content[..]).unwrap();

            let fig_content = b"not text";
            let mut fig_header = tar::Header::new_gnu();
            fig_header.set_size(fig_content.len() as u64);
            fig_header.set_cksum();
            builder.append_data(&mut fig_header, "figure.eps", &fig_content[..]).unwrap();
            builder.finish().unwrap();
        }
        let gz = gzip(&tar_bytes);

        match unpack(&gz).unwrap() {
            FetchedSource::Tex(files) => {
                assert_eq!(files.len(), 1, "拡張子.tex以外(figure.eps)は除外されるべき");
                assert_eq!(files[0].0, "main.tex");
            }
            FetchedSource::NoSource => panic!("tar中の.texが取れるべき"),
        }
    }

    #[test]
    fn unpack_rejects_unknown_binary_formats() {
        let bytes = b"\x00\x01totally unknown format";
        assert!(unpack(bytes).is_err());
    }
}

//! `web/dist`（`npm run build`の成果物）を丸ごとバイナリへ埋め込み、
//! ローカルHTTPサーバとして配る単体exe。
//!
//! # なぜこれが要るか
//!
//! これまでMathesisを見るには「wasm-packでカーネルをビルド→`npm
//! install`→`npm run dev`」という3段階の手順が要り、動作確認のたびに
//! Node.jsの開発環境が要った。この`mathesis-server`は逆に、**一度
//! ビルドしてしまえば**Node.jsもRustツールチェインも無い環境でも
//! `mathesis-server.exe`を実行するだけでブラウザが開いて動く、配布用の
//! 出口を作るためのもの。
//!
//! `npm run dev`（HMR込みの開発サーバ）を置き換えるものではない——
//! 日常の開発・実装確認は引き続き`npm run dev`（`web/README.md`の
//! 「ビルド手順」参照）で行う。こちらは「ビルド済みの成果物を人に渡す・
//! 手元でNode無しに開く」ための別経路。
//!
//! # ビルド手順
//!
//! `web/dist`は`rust-embed`のマクロが**コンパイル時**に読み込むため、
//! 先に`npm run build`を終えてから`cargo build --release -p
//! mathesis-server`を実行すること（`web/dist`が古いままだと、古い内容が
//! そのままexeに焼き込まれる）。
//!
//! # 圧縮
//!
//! `web/README.md`「デプロイ時の必須確認事項」で明記した通り、
//! `taxonomy.related.json`等の大きな静的JSONは圧縮の有無で配信量が
//! 5倍変わる。この単体サーバでは配信先の設定に頼れないので、起動時に
//! 一定サイズ以上の埋め込みファイルへ`flate2`でgzip圧縮をかけておき
//! （`mathesis-fulltext::source`が既に使っている依存の再利用）、
//! `Accept-Encoding: gzip`を送るクライアント（実質全ブラウザ）へは
//! 圧縮済みバイト列をそのまま返す。

use flate2::write::GzEncoder;
use flate2::Compression;
use rust_embed::RustEmbed;
use std::collections::HashMap;
use std::io::Write;
use tiny_http::{Header, Response, Server};

#[derive(RustEmbed)]
#[folder = "../../web/dist"]
struct Assets;

const PORT: u16 = 8787;

/// これより小さいファイルは圧縮してもヘッダ分のオーバーヘッドの方が
/// 大きくなりうるので、そのまま返す。
const COMPRESS_THRESHOLD: usize = 1024;

fn main() {
    let gzip_cache = precompress_large_assets();
    println!(
        "{}件の埋め込みファイルのうち{}件を起動時にgzip圧縮しました。",
        Assets::iter().count(),
        gzip_cache.len()
    );

    let server = match Server::http(("127.0.0.1", PORT)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "ポート{PORT}でのサーバ起動に失敗しました: {e}\n\
                 （他のMathesisサーバが既に起動していないか確認してください）"
            );
            std::process::exit(1);
        }
    };

    let url = format!("http://127.0.0.1:{PORT}");
    println!("Mathesis を {url} で起動しました。Ctrl+Cで終了します。");
    open_browser(&url);

    for request in server.incoming_requests() {
        let requested_path = normalize_path(request.url());
        let accepts_gzip = client_accepts_gzip(&request);

        let response = match Assets::get(&requested_path) {
            Some(file) => build_response(&requested_path, &file.data, &gzip_cache, accepts_gzip),
            // SPA（ハッシュルーティング）なので、直接一致しないパスは
            // index.htmlへ委ねる——`/#q=...`のようなハッシュ付きURLを
            // 直接開いても404にならないようにする。
            None => match Assets::get("index.html") {
                Some(file) => build_response("index.html", &file.data, &gzip_cache, accepts_gzip),
                None => Response::from_string("404 Not Found").with_status_code(404).boxed(),
            },
        };

        if let Err(e) = request.respond(response) {
            eprintln!("応答の送信に失敗しました: {e}");
        }
    }
}

/// `Assets::get`に渡す形（先頭の`/`無し、クエリ・ハッシュ無し）へ揃える。
fn normalize_path(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() { "index.html".to_string() } else { trimmed.to_string() }
}

fn client_accepts_gzip(request: &tiny_http::Request) -> bool {
    request.headers().iter().any(|h| {
        h.field.as_str().as_str().eq_ignore_ascii_case("Accept-Encoding")
            && h.value.as_str().to_lowercase().contains("gzip")
    })
}

fn build_response(
    path: &str,
    raw: &[u8],
    gzip_cache: &HashMap<String, Vec<u8>>,
    accepts_gzip: bool,
) -> tiny_http::ResponseBox {
    let content_type = Header::from_bytes(&b"Content-Type"[..], content_type_for(path).as_bytes())
        .expect("content-typeの値はASCIIのみなので必ず成功する");

    if accepts_gzip {
        if let Some(compressed) = gzip_cache.get(path) {
            let encoding = Header::from_bytes(&b"Content-Encoding"[..], &b"gzip"[..])
                .expect("固定リテラルなので必ず成功する");
            return Response::from_data(compressed.clone())
                .with_header(content_type)
                .with_header(encoding)
                .boxed();
        }
    }
    Response::from_data(raw.to_vec()).with_header(content_type).boxed()
}

/// 拡張子からMIMEタイプを引く。埋め込み対象は`vite build`の出力だけなので
/// 網羅すべき拡張子は限られている——未知の拡張子は安全側の
/// `application/octet-stream`に倒す。
fn content_type_for(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "wasm" => "application/wasm",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
}

fn precompress_large_assets() -> HashMap<String, Vec<u8>> {
    let mut cache = HashMap::new();
    for path in Assets::iter() {
        let Some(file) = Assets::get(&path) else { continue };
        if file.data.len() < COMPRESS_THRESHOLD {
            continue;
        }
        cache.insert(path.to_string(), gzip(&file.data));
    }
    cache
}

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).expect("メモリ上のバッファへの書き込みは失敗しない");
    encoder.finish().expect("メモリ上のバッファへの書き込みは失敗しない")
}

/// OS既定のブラウザでURLを開く。失敗しても致命的ではない
/// （サーバ自体は動いているので、利用者が手動でURLを開けば済む）ので、
/// エラーはメッセージを出すだけでプロセスは続行する。
fn open_browser(url: &str) {
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn();
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = std::process::Command::new("xdg-open").arg(url).spawn();

    if let Err(e) = result {
        eprintln!("ブラウザを自動で開けませんでした（{e}）。手動で {url} を開いてください。");
    }
}

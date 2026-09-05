use anyhow::{anyhow, Context, Result};
use mathesis_fulltext::{bridge, citation, source, store::FulltextStore, theorem};
use mathesis_graph::GraphStore;
use std::path::PathBuf;
use std::time::Instant;

fn usage() -> ! {
    eprintln!(
        "Usage:\n\
         \x20 mathesis-fulltext fetch-sources    <papers.db> [--limit N]\n\
         \x20 mathesis-fulltext extract-theorems <papers.db>\n\
         \x20 mathesis-fulltext bridge-to-graph  <papers.db> <graph.db>\n\n\
         fetch-sources:    arXivのe-print（LaTeXソース）を1件ずつ取得し、\n\
         \x20                  同じpapers.dbの`paper_sources`テーブルに保存する。\n\
         \x20                  PDFのみでLaTeXソースの無い論文は「ソース無し」として\n\
         \x20                  記録し（再実行時にスキップされる）、取得失敗として\n\
         \x20                  扱わない。arXivへの配慮として1件ごとに数秒待機する\n\
         \x20                  ため、件数が多いと時間がかかる——`--limit`で件数を絞る\n\
         \x20                  ことを推奨。\n\
         extract-theorems: 取得済みの全ソースから定理系環境・証明・\n\
         \x20                  （同一論文内の）証明依存関係・文献引用キーを抽出し、\n\
         \x20                  `paper_theorems`/`theorem_dependencies`に保存する。\n\
         bridge-to-graph:  抽出済みの定理を`mathesis-graph`の判断ノードへ\n\
         \x20                  `parse_status: informal`で取り込む（診断⑥）。種別が\n\
         \x20                  `JudgmentKind`に対応しない定理は見送り、件数を報告する。\n\
         \x20                  同一論文の重複橋渡しは避ける（`graph.db`に既に\n\
         \x20                  papersとして登録済みの論文はスキップ、`bridge_paper`\n\
         \x20                  単体を直接2回呼んでも冪等）。続けて`\\cite`から\n\
         \x20                  解決できたarXiv IDが既に橋渡し済みの論文を指す場合、\n\
         \x20                  論文単位の引用辺（`paper_citations`）も張る——著者名・\n\
         \x20                  タイトルの一致には頼らず、明示的な\"arXiv:\"記載だけを\n\
         \x20                  対象にする。"
    );
    std::process::exit(1);
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("fetch-sources") => run_fetch_sources(&args[2..]),
        Some("extract-theorems") => run_extract_theorems(&args[2..]),
        Some("bridge-to-graph") => run_bridge_to_graph(&args[2..]),
        _ => usage(),
    }
}

fn flag_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

fn run_fetch_sources(args: &[String]) -> Result<()> {
    let db_path = PathBuf::from(args.first().unwrap_or_else(|| usage()));
    let limit: usize =
        flag_value(args, "--limit").map(str::parse).transpose().context("--limit の値が数値ではありません")?.unwrap_or(50);

    let mut store = FulltextStore::open(&db_path)?;
    let arxiv_ids = store.list_arxiv_ids_needing_source(limit)?;
    if arxiv_ids.is_empty() {
        println!("未取得のarxiv_idはありません（既に{limit}件の枠内は取得済みか、papers.dbに論文が無いかのいずれか）");
        return Ok(());
    }
    println!(
        "{}件のe-printを取得します（1件ごとに{:.0}秒待機、合計で概算{}秒程度かかります）…",
        arxiv_ids.len(),
        source::COURTESY_DELAY.as_secs_f64(),
        arxiv_ids.len() * source::COURTESY_DELAY.as_secs() as usize
    );

    let started = Instant::now();
    let mut with_source = 0usize;
    let mut failed = 0usize;
    for (i, arxiv_id) in arxiv_ids.iter().enumerate() {
        match source::fetch_source(arxiv_id) {
            Ok(source::FetchedSource::Tex(files)) => {
                let combined = files.iter().map(|(_, content)| content.as_str()).collect::<Vec<_>>().join("\n\n");
                store.save_source(arxiv_id, Some(&combined), files.len())?;
                with_source += 1;
            }
            Ok(source::FetchedSource::NoSource) => {
                store.save_source(arxiv_id, None, 0)?;
            }
            Err(e) => {
                // 1件の取得失敗でバッチ全体を止めない——arXiv側の一時的な
                // エラーや個別論文の異常フォーマットは十分にありうる。
                eprintln!("  ! {arxiv_id}: {e}");
                failed += 1;
            }
        }
        if (i + 1) % 10 == 0 {
            println!("  … {}/{}件（ソース有り{with_source} / 失敗{failed}）", i + 1, arxiv_ids.len());
        }
        if i + 1 < arxiv_ids.len() {
            source::courtesy_wait();
        }
    }

    println!(
        "{:.0}秒で{}件処理: ソース取得{with_source}件 / PDFのみ{}件 / 取得失敗{failed}件",
        started.elapsed().as_secs_f64(),
        arxiv_ids.len(),
        arxiv_ids.len() - with_source - failed,
    );
    Ok(())
}

fn run_extract_theorems(args: &[String]) -> Result<()> {
    let db_path = PathBuf::from(args.first().unwrap_or_else(|| usage()));

    let mut store = FulltextStore::open(&db_path)?;
    let sources = store.list_fetched_tex()?;
    if sources.is_empty() {
        return Err(anyhow!(
            "{} に取得済みのLaTeXソースが0件です（先に `mathesis-fulltext fetch-sources` を実行してください）",
            db_path.display()
        ));
    }
    println!("{}件のソースから定理・証明・依存関係を抽出します…", sources.len());

    let started = Instant::now();
    let mut per_paper_counts: Vec<(String, usize)> = Vec::with_capacity(sources.len());
    // 引用キーがどの程度`\bibitem`で解決できるかも参考として集計する。
    let mut total_cites = 0usize;
    let mut resolved_cites = 0usize;
    // 診断⑥拡張: `\bibitem`本文に著者自身が明記したarXiv IDだけを対象に、
    // 論文をまたぐ引用先を解決する（`citation.rs`冒頭参照——著者名・
    // タイトルの文字列一致には頼らない）。
    let mut total_bibitems = 0usize;
    let mut resolved_arxiv_ids = 0usize;
    for (arxiv_id, tex) in &sources {
        let records = theorem::extract_from_tex(tex);
        let bibitems = theorem::extract_bibitems(tex);
        for r in &records {
            for key in &r.cites {
                total_cites += 1;
                if bibitems.contains_key(key) {
                    resolved_cites += 1;
                }
            }
        }
        per_paper_counts.push((arxiv_id.clone(), records.len()));
        store.save_theorems(arxiv_id, &records)?;

        total_bibitems += bibitems.len();
        let resolved: Vec<(String, String)> = bibitems
            .iter()
            .filter_map(|(key, text)| citation::extract_arxiv_id(text).map(|id| (key.clone(), id)))
            .collect();
        resolved_arxiv_ids += resolved.len();
        store.save_bibitem_citations(arxiv_id, &resolved)?;
    }
    let elapsed = started.elapsed();

    let stats = store.theorem_stats()?;
    let papers_with_any_theorem = per_paper_counts.iter().filter(|(_, n)| *n > 0).count();
    println!(
        "{:.1}秒で{}論文を処理: 定理系環境{}件（うち証明つき{}件 = タイトル明示{}件+隣接推定{}件）・\n\
         同一論文内の依存関係{}本を {} に保存しました",
        elapsed.as_secs_f64(),
        sources.len(),
        stats.total,
        stats.with_proof,
        stats.titled_matches,
        stats.adjacent_matches,
        stats.dependency_edges,
        db_path.display()
    );
    println!(
        "定理系環境を1件以上抽出できた論文: {papers_with_any_theorem}/{}件（{:.0}%）",
        sources.len(),
        100.0 * papers_with_any_theorem as f64 / sources.len().max(1) as f64
    );
    println!(
        "証明中の文献引用キー: {total_cites}件、うち同一論文内の`\\bibitem`で本文が引けたもの{resolved_cites}件（{:.0}%）",
        100.0 * resolved_cites as f64 / total_cites.max(1) as f64
    );
    println!(
        "文献引用{total_bibitems}件のうち、明示的な`arXiv:`記載からarXiv IDを解決できたもの{resolved_arxiv_ids}件（{:.1}%、著者名・タイトルの一致には頼らない）",
        100.0 * resolved_arxiv_ids as f64 / total_bibitems.max(1) as f64
    );

    let mut top: Vec<&(String, usize)> = per_paper_counts.iter().collect();
    top.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    println!("\n定理系環境を最も多く含む論文上位10件:");
    for (arxiv_id, n) in top.iter().take(10) {
        println!("  [{n:>3}件] {arxiv_id}");
    }

    Ok(())
}

fn run_bridge_to_graph(args: &[String]) -> Result<()> {
    let db_path = PathBuf::from(args.first().unwrap_or_else(|| usage()));
    let graph_path = PathBuf::from(args.get(1).unwrap_or_else(|| usage()));

    let fulltext = FulltextStore::open(&db_path)?;
    let graph = GraphStore::open(&graph_path)?;

    let started = Instant::now();
    let mut papers_bridged = 0usize;
    let total = bridge::bridge_all_new_papers(&fulltext, &graph, |arxiv_id, stats| {
        papers_bridged += 1;
        if stats.judgments_inserted > 0 || !stats.kinds_skipped.is_empty() {
            println!(
                "  {arxiv_id}: 判断{}件・依存{}本（見送り{}件）",
                stats.judgments_inserted,
                stats.dependencies_created,
                stats.kinds_skipped.values().sum::<usize>()
            );
        }
    })?;

    println!(
        "\n{:.1}秒で{papers_bridged}論文を橋渡し: 判断ノード{}件・依存関係{}本を {} へ保存しました\n\
         （未解決の依存{}本、種別が対応せず見送った定理{}件）",
        started.elapsed().as_secs_f64(),
        total.judgments_inserted,
        total.dependencies_created,
        graph_path.display(),
        total.dependencies_unresolved,
        total.kinds_skipped.values().sum::<usize>(),
    );
    if !total.kinds_skipped.is_empty() {
        let mut kinds: Vec<(&String, &usize)> = total.kinds_skipped.iter().collect();
        kinds.sort_by_key(|(_, &n)| std::cmp::Reverse(n));
        println!("見送った種別の内訳:");
        for (kind, n) in kinds {
            println!("  [{n:>4}件] {kind}");
        }
    }

    // 診断⑥拡張: 論文単位の引用（`\cite`→arXiv ID→既に橋渡し済みの論文）。
    // `bridge_all_new_papers`とは別パスで、全論文が出揃った後に走らせる
    // （`bridge.rs::bridge_all_paper_citations`のdoc comment参照——走査
    // 順序によって引用先がまだ橋渡しされていないケースを取りこぼさない
    // ため）。
    let citation_started = Instant::now();
    let citation_stats = bridge::bridge_all_paper_citations(&fulltext, &graph)?;
    println!(
        "\n{:.1}秒で論文単位の引用を解決: {}本を保存（arXiv IDへの解決はできたが引用先が未橋渡しのため見送ったもの{}件）",
        citation_started.elapsed().as_secs_f64(),
        citation_stats.citations_created,
        citation_stats.citations_unresolved,
    );

    Ok(())
}

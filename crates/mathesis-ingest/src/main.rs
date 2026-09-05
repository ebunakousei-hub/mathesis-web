use anyhow::{Context, Result};
use mathesis_ingest::{oai, store::PaperStore};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!(
            "Usage: mathesis-ingest <oai-set> <output.db> [--max N] [--from YYYY-MM-DD]
                                 [--until YYYY-MM-DD] [--since-last] [--resume]
             例:   mathesis-ingest math:math:CT papers.db --max 500
                   mathesis-ingest math papers.db --since-last   # 前回以降の差分だけ
                   mathesis-ingest math papers.db --resume       # 中断した収集の続き
             OAI-PMHのセット一覧は https://export.arxiv.org/oai2?verb=ListSets で確認できる。
             --max を省略すると、そのセットの全件をresumptionTokenで辿り切るまで収集する。
             ページごとに保存するので、途中で中断しても収集済みの分は残る。"
        );
        std::process::exit(1);
    }

    let set = args[1].clone();
    let db_path = PathBuf::from(&args[2]);
    let flag = |name: &str| -> Option<String> {
        args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
    };
    let has = |name: &str| args.iter().any(|a| a == name);

    let max = flag("--max")
        .map(|s| s.parse::<usize>())
        .transpose()
        .context("--max の値が数値ではありません")?;

    let mut store = PaperStore::open(&db_path)?;

    let mut request = oai::HarvestRequest::new(set.clone());
    request.max = max;
    request.until = flag("--until");
    request.from = flag("--from");

    // --since-last: 既に収集済みの最新の投稿日を起点にする。全件を取り直さず、
    // その日以降に更新されたものだけを取る。
    if has("--since-last") && request.from.is_none() {
        match store.last_harvest_date()? {
            Some(day) => {
                println!("前回の収集日 {day} 以降に更新されたものだけを取得します");
                request.from = Some(day);
            }
            None => println!("まだ論文が1件も無いので、差分ではなく全件収集になります"),
        }
    }

    // --resume: 前回中断したところから続ける。
    if has("--resume") {
        match store.load_harvest_state(&request.key())? {
            Some((token, harvested, finished)) if !finished && token.is_some() => {
                println!("前回の続きから再開します（収集済み{harvested}件）");
                request.resume_token = token;
                request.already_harvested = harvested;
            }
            Some((_, harvested, true)) => {
                println!("この条件の収集は前回完了しています（{harvested}件）。最初から取り直します");
            }
            _ => println!("再開できる途中経過が無いので、最初から収集します"),
        }
    }

    println!(
        "arXiv OAI-PMH set={set} から収集します（max={:?}, from={:?}, until={:?}）…",
        request.max, request.from, request.until
    );

    // MSC照合の集計はページごとに積む。全論文をメモリに持たない形にした
    // 以上、最後にまとめて数え直すことはできない（そしてその必要も無い）。
    let mut total_codes = 0usize;
    let mut unknown_codes = 0usize;
    let mut top_level_hits: HashMap<String, usize> = HashMap::new();
    let mut with_msc = 0usize;

    let started = Instant::now();
    let key = request.key();
    // ページごとに保存する。ここで落ちても、保存済みの分とトークンは残る。
    let mut stored = 0usize;
    let total = oai::harvest_streaming(&request, |page, next_token, running_total, exhausted| {
        if !page.is_empty() {
            stored += store.upsert_all(page)?;
        }
        for paper in page {
            if !paper.msc_codes.is_empty() {
                with_msc += 1;
            }
            for code in &paper.msc_codes {
                total_codes += 1;
                match mathesis_msc::by_code(code) {
                    Some(msc) => {
                        if let Some(top) = mathesis_msc::ancestor_chain(&msc.code).first() {
                            *top_level_hits.entry(top.code.clone()).or_default() += 1;
                        }
                    }
                    None => unknown_codes += 1,
                }
            }
        }
        store.save_harvest_state(&key, next_token, running_total, exhausted)?;
        println!("  … {running_total}件取得済み（DB保存済み）");
        Ok(())
    })?;
    let elapsed = started.elapsed();

    if total == 0 {
        println!("新しい論文はありませんでした（{:.1}秒）", elapsed.as_secs_f64());
        return Ok(());
    }

    println!(
        "{total}件を{:.1}秒で収集し {} に保存しました（新規/更新 {stored}件、DB全体 {}件）",
        elapsed.as_secs_f64(),
        db_path.display(),
        store.count()?
    );

    println!(
        "自己申告MSCコードあり: {with_msc}件（{:.0}%）",
        100.0 * with_msc as f64 / total.max(1) as f64
    );
    // Phase 5（MSC2020とのalignment）の下準備として、収集できた自己申告MSC
    // コードが層6のseed ontology（mathesis-msc）に実在するかここで軽く
    // 照合しておく。フルのalignmentアルゴリズムはPhase 5で実装する。
    println!(
        "MSC2020照合: 自己申告コード{total_codes}件中、mathesis-msc（層6のseed ontology）に実在したのは{}件、未知の表記は{unknown_codes}件",
        total_codes - unknown_codes
    );

    let mut top_sorted: Vec<(String, usize)> = top_level_hits.into_iter().collect();
    top_sorted.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    println!("トップレベル分野への内訳（上位10件）:");
    for (code, count) in top_sorted.iter().take(10) {
        let name = mathesis_msc::by_code(code)
            .map(|c| c.name.as_str())
            .unwrap_or("?");
        println!("  {code} {name}: {count}件");
    }

    Ok(())
}

use anyhow::{anyhow, Context, Result};
use mathesis_graph::{
    intern_hypotheses, GraphStore, JudgmentKind, NewJudgment, PaperId, ParseStatus, SourceRef,
};
use mathesis_importer::dependencies::{self, InsertedJudgment};
use mathesis_lean_parse::{parse_lean_source, ParseOutcome, ParsedJudgment, ParsedKind};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("export-only") {
        return run_export_only(&args[2..]);
    }
    if args.len() < 3 {
        eprintln!(
            "Usage: mathesis-import <input.lean|input.v|dir> <output.db> [--propose-morphisms] [--validate] [--dump]\n\
             \x20      [--paper <arxiv_id>] [--paper-title <title>] [--export <output.json>]\n\
             \x20 mathesis-import export-only <db> <output.json>\n\
             入力にディレクトリを渡すと、配下の *.lean を再帰的に一括インポートする。\n\
             --paper: このインポート実行で読み込む全ての判断ノードを、指定した\n\
             \x20        arXiv論文（概念タクソノミー側、mathesis-taxonomy/mathesis-ingest）\n\
             \x20        に紐付ける（Phase 9: judgments.source_paper_id）。\n\
             --export: インポート後の判断グラフ全体（判断・依存関係・論文リンク）を\n\
             \x20        Web版Explorer用の静的JSONへ書き出す（Phase 10）。\n\
             export-only: 何もインポートせず、既存の<db>を読んで<output.json>へ\n\
             \x20        書き出すだけ（診断⑥: `mathesis-fulltext bridge-to-graph`で\n\
             \x20        判断を追加した後、再インポートせずに書き出し直すため）。"
        );
        std::process::exit(1);
    }

    let input_path = PathBuf::from(&args[1]);
    let db_path = PathBuf::from(&args[2]);

    if !input_path.exists() {
        return Err(anyhow!("Input file not found: {}", input_path.display()));
    }

    let propose = args.iter().any(|a| a == "--propose-morphisms");
    let validate = args.iter().any(|a| a == "--validate");
    let dump = args.iter().any(|a| a == "--dump");
    let paper_arxiv_id = flag_value(&args, "--paper");
    let paper_title = flag_value(&args, "--paper-title");
    let export_path = flag_value(&args, "--export");

    let files = collect_lean_files(&input_path);
    if files.is_empty() {
        return Err(anyhow!(
            "no .lean files found under {}",
            input_path.display()
        ));
    }

    let store = GraphStore::open(&db_path)?;

    let paper_id: Option<PaperId> = match paper_arxiv_id {
        Some(id) => Some(store.intern_paper(id, paper_title)?),
        None => None,
    };

    // 全ファイルぶんの挿入を1つの SQLite トランザクションにまとめる。
    // 1ファイルごと・1判断ごとに自動コミットさせると、行数分のfsync待ちが
    // 支配的になる（`GraphStore::transaction` のドキュメント参照）。
    // ただしファイル単位のパース失敗は従来どおり握りつぶして続行したいので、
    // 個々の `import_file` のエラーはここで catch し、トランザクション自体は
    // 失敗させない（ロールバックするのは致命的なDBエラー時のみ）。
    let import_started = Instant::now();
    let (inserted, failed_files) = store.transaction(|| -> Result<(Vec<InsertedJudgment>, usize)> {
        let mut inserted = Vec::new();
        let mut failed_files = 0usize;
        for f in &files {
            match import_file(&store, f, paper_id) {
                Ok(mut v) => inserted.append(&mut v),
                Err(e) => {
                    failed_files += 1;
                    eprintln!("  ✗ {}: {e}", f.display());
                }
            }
        }
        Ok((inserted, failed_files))
    })?;
    let import_elapsed = import_started.elapsed();
    let total = inserted.len();

    println!(
        "Imported {} judgments from {} file(s) ({} failed) in {:.3}s ({:.1} judgments/s)",
        total,
        files.len(),
        failed_files,
        import_elapsed.as_secs_f64(),
        total as f64 / import_elapsed.as_secs_f64().max(1e-9)
    );

    if let Some(arxiv_id) = paper_arxiv_id {
        println!("Linked all imported judgments to paper {arxiv_id}");
    }

    // Phase 9: このバッチ内の判断ノードどうしで、証明/定義本体が既存の
    // 判断名を参照している箇所を検出し、judgment_dependencies に記録する
    // （`crates/mathesis-fulltext` Phase 8 の theorem_dependencies のLean版）。
    let dep_started = Instant::now();
    let dep_count = store.transaction(|| -> Result<usize> {
        Ok(dependencies::record_dependencies(&store, &inserted)?)
    })?;
    println!(
        "Recorded {} judgment dependency edges in {:.3}s",
        dep_count,
        dep_started.elapsed().as_secs_f64()
    );

    if dump {
        for j in store.list_judgments()? {
            println!(
                "  [{:<10}] {:<40} parse={:<8} :: {}",
                j.kind.as_str(),
                j.name.as_deref().unwrap_or("<anon>"),
                j.parse_status.as_str(),
                j.statement
            );
        }
    }

    if propose {
        let propose_started = Instant::now();
        let ids = store.propose_morphisms()?;
        println!(
            "Proposed {} morphism candidates (status=proposed, not accepted) in {:.3}s",
            ids.len(),
            propose_started.elapsed().as_secs_f64()
        );
    }

    if validate {
        let validate_started = Instant::now();
        let report = store.validate()?;
        println!(
            "{} ({:.3}s)",
            report.summary(),
            validate_started.elapsed().as_secs_f64()
        );
        if !report.is_valid() {
            for issue in &report.broken_references {
                eprintln!("  ✗ {}", issue);
            }
            std::process::exit(1);
        }
    }

    if let Some(output_path) = export_path {
        let export_started = Instant::now();
        let export = mathesis_graph::build_export(&store)?;
        let json = serde_json::to_string_pretty(&export)?;
        std::fs::write(output_path, &json)
            .with_context(|| format!("{output_path} への書き込みに失敗しました"))?;
        println!(
            "Exported {} judgments / {} dependencies / {} morphisms / {} papers to {output_path} ({} bytes, {:.3}s)",
            export.judgment_count,
            export.dependency_count,
            export.morphism_count,
            export.papers.len(),
            json.len(),
            export_started.elapsed().as_secs_f64()
        );
    }

    println!("Import completed: {}", db_path.display());
    Ok(())
}

/// `mathesis-import export-only <db> <output.json>`。何もインポートせず、
/// 既存の判断グラフを読んで書き出すだけ——診断⑥のブリッジ
/// （`mathesis-fulltext bridge-to-graph`）が同じdbへJudgmentを追記した後、
/// Lean fixtureを再インポートして重複させずに書き出し直すために要る
/// （`--export`は常にインポート実行とセットで、インポート無しの単独
/// 書き出しが元々無かった）。
fn run_export_only(args: &[String]) -> Result<()> {
    let db_path = args.first().ok_or_else(|| anyhow!("Usage: mathesis-import export-only <db> <output.json>"))?;
    let output_path = args.get(1).ok_or_else(|| anyhow!("Usage: mathesis-import export-only <db> <output.json>"))?;

    let store = GraphStore::open(db_path)?;
    let export_started = Instant::now();
    let export = mathesis_graph::build_export(&store)?;
    let json = serde_json::to_string_pretty(&export)?;
    std::fs::write(output_path, &json).with_context(|| format!("{output_path} への書き込みに失敗しました"))?;
    println!(
        "Exported {} judgments / {} dependencies / {} morphisms / {} papers to {output_path} ({} bytes, {:.3}s)",
        export.judgment_count,
        export.dependency_count,
        export.morphism_count,
        export.papers.len(),
        json.len(),
        export_started.elapsed().as_secs_f64()
    );
    Ok(())
}

fn flag_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

/// `path` がファイルならそれ自身を、ディレクトリなら配下の `*.lean` を
/// 再帰的に集めて返す（多数の論文・リポジトリをまとめて取り込むことを
/// 見据えた一括インポート用）。
fn collect_lean_files(path: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_lean_files_into(path, &mut out);
    out.sort();
    out
}

fn collect_lean_files_into(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            collect_lean_files_into(&entry.path(), out);
        }
    } else if path.extension().is_some_and(|e| e == "lean") {
        out.push(path.to_path_buf());
    }
}

/// ファイルをインポートして、挿入した判断ノード（依存関係抽出用に
/// id・名前・生テキストを保持）の一覧を返す。
///
/// Lean構文の分割・命題/文脈の抽出そのものは `mathesis_lean_parse`
/// （ブラウザの`mathesis-wasm`とも共有する、`mathesis-graph`に依存しない
/// 純粋なパーサー）が行う。ここでの仕事は、その結果をDBへ挿入する
/// （束縛子の型・命題の式をインターンし、`NewJudgment`を組み立てる）
/// ことだけに絞られている。
fn import_file(store: &GraphStore, path: &Path, paper: Option<PaperId>) -> Result<Vec<InsertedJudgment>> {
    let content = fs::read_to_string(path)?;
    let judgments = parse_lean_source(&content);
    let mut out = Vec::with_capacity(judgments.len());

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    for parsed in judgments {
        let ParsedJudgment {
            kind,
            name,
            context,
            statement_expr,
            statement_status,
            definition_body_raw,
            raw_text,
            line,
            ..
        } = parsed;

        let hypotheses = intern_hypotheses(store, &context)?;
        let stmt_id = store.intern_expr(&statement_expr)?;
        let parse_status = match statement_status {
            ParseOutcome::Full => ParseStatus::Full,
            ParseOutcome::Partial => ParseStatus::Partial,
            ParseOutcome::Failed => ParseStatus::Failed,
        };

        let judgment = NewJudgment {
            kind: to_graph_kind(kind),
            name,
            context: hypotheses,
            statement: stmt_id,
            definition_body_raw,
            source: SourceRef { file: file_name.clone(), line },
            raw_text,
            parse_status,
            source_paper: paper,
        };
        let id = store.insert_judgment(&judgment)?;
        out.push(InsertedJudgment { id, name: judgment.name, raw_text: judgment.raw_text });
    }

    Ok(out)
}

fn to_graph_kind(kind: ParsedKind) -> JudgmentKind {
    match kind {
        ParsedKind::Theorem => JudgmentKind::Theorem,
        ParsedKind::Definition => JudgmentKind::Definition,
        ParsedKind::Axiom => JudgmentKind::Axiom,
        ParsedKind::Instance => JudgmentKind::Instance,
        ParsedKind::Example => JudgmentKind::Example,
    }
}

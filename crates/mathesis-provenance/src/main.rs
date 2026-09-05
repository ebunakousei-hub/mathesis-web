//! CLI。`mathesis-taxonomy`/`mathesis-annotate`と同じ「1バイナリ複数
//! サブコマンド、`clap`は使わず`flag_value`ヘルパーで自前パース」方式。

use anyhow::{Context, Result};
use mathesis_graph::GraphStore;
use mathesis_provenance::legacy_adapter::{import_graph, import_taxonomy_relations};
use mathesis_provenance::model::NewRelease;
use mathesis_provenance::reconcile::{reconcile_graph, reconcile_taxonomy};
use mathesis_provenance::{stats, ProvenanceStore};
use mathesis_taxonomy::store::TaxonomyStore;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn usage() -> ! {
    eprintln!(
        "mathesis-provenance <command>\n\n\
         Commands:\n\
         \x20 import-legacy --graph-db <path> --taxonomy-db <path> --release <tag> --out <path> [--git-commit <sha>]\n\
         \x20     mathesis-graphとmathesis-taxonomyの既存データをdocs/DATA_DICTIONARY.mdの\n\
         \x20     マッピングに従って証拠層(--out)へ写す。同じ(release, legacy_ref)は冪等\n\
         \x20     ——同じデータベースに同じリリースタグで再実行しても行は増えない。\n\
         \x20 stats --db <path>\n\
         \x20     証拠層DBの集計(assertion数、predicate/epistemic_state別内訳、\n\
         \x20     evidence行数のヒストグラム、review_decision数)を表示する。\n\
         \x20 reconcile --graph-db <path> --taxonomy-db <path> --provenance-db <path> --release <tag> --out-dir <path>\n\
         \x20     import-legacy済みの証拠層DBに対し、web/が今表示しているすべての辺\n\
         \x20     (judgment_dependencies/paper_citations/morphisms/concept_relations)が\n\
         \x20     assertionへ引けるかを検証し、judgments.provenance.json /\n\
         \x20     taxonomy.relations.provenance.json を書き出す。既存のexport.rsや\n\
         \x20     web/には一切触れない——追加のサイドカーファイルのみ。"
    );
    std::process::exit(1);
}

fn flag_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

fn require_flag<'a>(args: &'a [String], name: &str) -> Result<&'a str> {
    flag_value(args, name).with_context(|| format!("必須フラグ {name} がありません"))
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("import-legacy") => run_import_legacy(&args[2..]),
        Some("stats") => run_stats(&args[2..]),
        Some("reconcile") => run_reconcile(&args[2..]),
        _ => usage(),
    }
}

fn run_import_legacy(args: &[String]) -> Result<()> {
    let graph_db = PathBuf::from(require_flag(args, "--graph-db")?);
    let taxonomy_db = PathBuf::from(require_flag(args, "--taxonomy-db")?);
    let release_tag = require_flag(args, "--release")?.to_string();
    let out_db = PathBuf::from(require_flag(args, "--out")?);
    let git_commit = flag_value(args, "--git-commit").map(str::to_string);

    let graph = GraphStore::open(&graph_db).with_context(|| format!("{graph_db:?} を開けません"))?;
    let taxonomy = TaxonomyStore::open(&taxonomy_db).with_context(|| format!("{taxonomy_db:?} を開けません"))?;
    let prov = ProvenanceStore::open(&out_db).with_context(|| format!("{out_db:?} を開けません"))?;

    let generated_at_unix = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let release = prov.get_or_insert_release(&NewRelease {
        tag: release_tag.clone(),
        git_commit,
        generated_at_unix,
        notes: Some("mathesis-provenance import-legacy".into()),
    })?;

    // 数千行を素朴にループ挿入すると1行=1トランザクション(SQLite自動コミット)
    // でfsync待ちが支配的になる(`mathesis-graph::store`のdoc comment参照、
    // 実測9.8ms/行)。ここでは2回のインポート全体を1トランザクションに包み、
    // コミット回数を2回に減らす。
    let graph_stats = prov.transaction(|| import_graph(&graph, &prov, release, &release_tag))?;
    println!(
        "graph: dependencies +{} (skip {}), citations +{} (skip {}), morphisms +{} (skip {}), review_decisions +{}",
        graph_stats.dependencies_imported,
        graph_stats.dependencies_skipped_existing,
        graph_stats.citations_imported,
        graph_stats.citations_skipped_existing,
        graph_stats.morphisms_imported,
        graph_stats.morphisms_skipped_existing,
        graph_stats.review_decisions_created,
    );

    let taxonomy_stats = prov.transaction(|| import_taxonomy_relations(&taxonomy, &prov, release, &release_tag))?;
    println!(
        "taxonomy: relations +{} (skip {})",
        taxonomy_stats.relations_imported, taxonomy_stats.relations_skipped_existing,
    );

    Ok(())
}

fn run_stats(args: &[String]) -> Result<()> {
    let db = PathBuf::from(require_flag(args, "--db")?);
    let prov = ProvenanceStore::open(&db).with_context(|| format!("{db:?} を開けません"))?;
    stats::compute(&prov)?.print();
    Ok(())
}

fn run_reconcile(args: &[String]) -> Result<()> {
    let graph_db = PathBuf::from(require_flag(args, "--graph-db")?);
    let taxonomy_db = PathBuf::from(require_flag(args, "--taxonomy-db")?);
    let provenance_db = PathBuf::from(require_flag(args, "--provenance-db")?);
    let release_tag = require_flag(args, "--release")?.to_string();
    let out_dir = PathBuf::from(require_flag(args, "--out-dir")?);

    let graph = GraphStore::open(&graph_db).with_context(|| format!("{graph_db:?} を開けません"))?;
    let taxonomy = TaxonomyStore::open(&taxonomy_db).with_context(|| format!("{taxonomy_db:?} を開けません"))?;
    let prov = ProvenanceStore::open(&provenance_db).with_context(|| format!("{provenance_db:?} を開けません"))?;

    let (judgments_export, mut report) = reconcile_graph(&graph, &prov, &release_tag)?;
    let (relations_export, taxonomy_report) = reconcile_taxonomy(&taxonomy, &prov, &release_tag)?;
    report.relations_total = taxonomy_report.relations_total;
    report.relations_traced = taxonomy_report.relations_traced;
    report.print();

    std::fs::create_dir_all(&out_dir)?;
    std::fs::write(
        out_dir.join("judgments.provenance.json"),
        serde_json::to_string(&judgments_export)?,
    )?;
    std::fs::write(
        out_dir.join("taxonomy.relations.provenance.json"),
        serde_json::to_string(&relations_export)?,
    )?;
    println!("wrote {} and {}", out_dir.join("judgments.provenance.json").display(), out_dir.join("taxonomy.relations.provenance.json").display());

    if !report.is_fully_traced() {
        anyhow::bail!("reconciliation incomplete — see counts above");
    }
    Ok(())
}

//! CLI。`mathesis-taxonomy`/`mathesis-annotate`と同じ「1バイナリ複数
//! サブコマンド、`clap`は使わず`flag_value`ヘルパーで自前パース」方式。

use anyhow::{Context, Result};
use mathesis_graph::GraphStore;
use mathesis_provenance::assertion_export::export_assertion_details;
use mathesis_provenance::catalog_adapter::{build_concept_catalog, build_judgment_paper_catalog};
use mathesis_provenance::legacy_adapter::{import_graph, import_taxonomy_relations, ADAPTER_NAME, ADAPTER_VERSION};
use mathesis_provenance::manifest::{
    input_file_hash, ManifestCounts, ProvenanceManifest, WebExportCounts, WebExportManifest, SCHEMA_VERSION,
    WEB_EXPORT_SCHEMA_VERSION,
};
use mathesis_provenance::model::NewRelease;
use mathesis_provenance::reconcile::{reconcile_graph, reconcile_taxonomy, JudgmentsProvenanceExport, RelationsProvenanceExport};
use mathesis_provenance::release_gate::{print_web_export_failures, verify_web_export};
use mathesis_provenance::verify::{verify_release, VerifyInputs};
use mathesis_provenance::web_export::{build_web_export, WEB_EXPORT_VERSION};
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
         \x20     taxonomy.relations.provenance.json / provenance-manifest.json を書き出す。\n\
         \x20     既存のexport.rsやweb/には一切触れない——追加のサイドカーファイルのみ。\n\
         \x20 verify --manifest <path> --judgments-provenance <path> --relations-provenance <path>\n\
         \x20        --provenance-db <path> [--graph-db <path>] [--taxonomy-db <path>]\n\
         \x20     reconcileが書き出したサイドカー+マニフェストを、証拠層DB本体および\n\
         \x20     (指定すれば)元の入力DBと突き合わせる完全性ゲート。CI/リリースゲート\n\
         \x20     として繰り返し実行する想定——1件でも鎖が切れていれば非ゼロ終了する。\n\
         \x20 web-export --db <path> --release <tag> --out-dir <path>\n\
         \x20     P2: web/が実際に表示する辺(dependencies.json/morphisms.json/\n\
         \x20     relations.json)を、証拠層DB**だけ**を入口に生成する——\n\
         \x20     mathesis-graph/mathesis-taxonomyのSQLiteは一切開かない。\n\
         \x20     kind/origin/status/rationale/confidenceはすべてEvidence/\n\
         \x20     ReviewDecisionから再構成する（docs/P2_STATUS.md参照）。\n\
         \x20     同じディレクトリに web-export-manifest.json（自身の出力の\n\
         \x20     SHA-256・件数・schemaVersion）も書き出す。\n\
         \x20 verify-release --manifest <path> --judgments-provenance <path> --relations-provenance <path>\n\
         \x20                --provenance-db <path> --web-export-manifest <path> --web-export-dir <path>\n\
         \x20                --release <tag> [--graph-db <path>] [--taxonomy-db <path>]\n\
         \x20     P1のverify（サイドカーの完全性）とP2固有のweb-export検証\n\
         \x20     （出力ファイルのハッシュ一致、および今のProvenanceStoreから\n\
         \x20     再生成した内容との構造的一致——「古いコミット/リリースから\n\
         \x20     生成されたexportがそのまま残っている」を検出する）を1つに\n\
         \x20     まとめた、唯一の正式なリリースゲート。どちらか一方でも\n\
         \x20     失敗すれば非ゼロ終了する。\n\
         \x20 build-catalog --graph-db <path> --taxonomy-db <path> --db <path>\n\
         \x20     P3: 型付きエンティティカタログ(ARCHITECTURE_NEXT.md §5.2の\n\
         \x20     Paper/Statement/Conceptの最小版)を、mathesis-graph/\n\
         \x20     mathesis-taxonomyの既存データ**だけ**から冪等に作る——新しい\n\
         \x20     判断や捏造したラベルは増やさない。conceptはmathesis-taxonomy\n\
         \x20     自身のEntity Resolution(resolve.rs)が畳んだ表記ゆれをそのまま\n\
         \x20     alias群として使う。subject_ref/object_refの書式はまだ変えない\n\
         \x20     ——`stats`で「今のassertionのうち何件がカタログへ実際に\n\
         \x20     引けるか」を見られるようにするだけ(docs/P3_STATUS.md参照)。"
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
        Some("verify") => run_verify(&args[2..]),
        Some("web-export") => run_web_export(&args[2..]),
        Some("verify-release") => run_verify_release(&args[2..]),
        Some("build-catalog") => run_build_catalog(&args[2..]),
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
    let release = prov
        .get_release_by_tag(&release_tag)?
        .with_context(|| format!("release '{release_tag}' not found — run import-legacy first"))?;

    let (judgments_export, mut report) = reconcile_graph(&graph, &prov, &release_tag)?;
    let (relations_export, taxonomy_report) = reconcile_taxonomy(&taxonomy, &prov, &release_tag)?;
    report.relations_total = taxonomy_report.relations_total;
    report.relations_traced = taxonomy_report.relations_traced;
    report.print();

    std::fs::create_dir_all(&out_dir)?;
    std::fs::write(out_dir.join("judgments.provenance.json"), serde_json::to_string(&judgments_export)?)?;
    std::fs::write(out_dir.join("taxonomy.relations.provenance.json"), serde_json::to_string(&relations_export)?)?;

    // 外部レビュー(2026-09-05)提案4: ツールチップの1行で終わらせず、
    // assertion単位の全詳細(Evidence・ReviewDecision・既定トラバース対象か)
    // を1つの辞書にまとめて出す——フロントエンドのprovenanceパネルが
    // クリックのたびに個別リクエストを飛ばさずに済むようにする。
    let all_assertion_ids = judgments_export
        .dependencies
        .iter()
        .map(|d| d.assertion_id)
        .chain(judgments_export.citations.iter().map(|c| c.assertion_id))
        .chain(judgments_export.morphisms.iter().map(|m| m.assertion_id))
        .chain(relations_export.relations.iter().map(|r| r.assertion_id));
    let assertions = export_assertion_details(&prov, &release_tag, all_assertion_ids)?;
    std::fs::write(out_dir.join("assertions.json"), serde_json::to_string(&assertions)?)?;

    // 外部レビュー(2026-09-05)提案2: 機械可読マニフェスト。サイドカーが
    // 「どの入力・どのアダプタ版で作られたか」を自己申告することで、
    // 静的JSON/DBが後で入れ替わっても`verify`がズレを検出できるようにする。
    let manifest = ProvenanceManifest {
        release_tag: release_tag.clone(),
        release_id: release.id.0,
        release_git_commit: release.git_commit.clone(),
        source_database_schema: SCHEMA_VERSION,
        adapter_name: ADAPTER_NAME.to_string(),
        adapter_version: ADAPTER_VERSION.to_string(),
        input_files: vec![input_file_hash(&graph_db)?, input_file_hash(&taxonomy_db)?],
        generated_at_unix: SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0),
        counts: ManifestCounts {
            dependencies: judgments_export.dependencies.len(),
            citations: judgments_export.citations.len(),
            morphisms: judgments_export.morphisms.len(),
            relations: relations_export.relations.len(),
        },
    };
    std::fs::write(out_dir.join("provenance-manifest.json"), serde_json::to_string_pretty(&manifest)?)?;

    println!(
        "wrote {}, {}, {}, and {}",
        out_dir.join("judgments.provenance.json").display(),
        out_dir.join("taxonomy.relations.provenance.json").display(),
        out_dir.join("assertions.json").display(),
        out_dir.join("provenance-manifest.json").display(),
    );

    if !report.is_fully_traced() {
        anyhow::bail!("reconciliation incomplete — see counts above");
    }
    Ok(())
}

fn run_verify(args: &[String]) -> Result<()> {
    let manifest_path = PathBuf::from(require_flag(args, "--manifest")?);
    let judgments_path = PathBuf::from(require_flag(args, "--judgments-provenance")?);
    let relations_path = PathBuf::from(require_flag(args, "--relations-provenance")?);
    let provenance_db = PathBuf::from(require_flag(args, "--provenance-db")?);
    let graph_db = flag_value(args, "--graph-db").map(PathBuf::from);
    let taxonomy_db = flag_value(args, "--taxonomy-db").map(PathBuf::from);

    let manifest: ProvenanceManifest = serde_json::from_slice(
        &std::fs::read(&manifest_path).with_context(|| format!("{manifest_path:?} を開けません"))?,
    )
    .with_context(|| format!("{manifest_path:?} のパースに失敗"))?;
    let judgments_provenance: JudgmentsProvenanceExport = serde_json::from_slice(
        &std::fs::read(&judgments_path).with_context(|| format!("{judgments_path:?} を開けません"))?,
    )
    .with_context(|| format!("{judgments_path:?} のパースに失敗"))?;
    let relations_provenance: RelationsProvenanceExport = serde_json::from_slice(
        &std::fs::read(&relations_path).with_context(|| format!("{relations_path:?} を開けません"))?,
    )
    .with_context(|| format!("{relations_path:?} のパースに失敗"))?;
    let prov = ProvenanceStore::open(&provenance_db).with_context(|| format!("{provenance_db:?} を開けません"))?;

    let mut live_input_files = Vec::new();
    if let Some(p) = &graph_db {
        live_input_files.push(input_file_hash(p)?);
    }
    if let Some(p) = &taxonomy_db {
        live_input_files.push(input_file_hash(p)?);
    }
    if live_input_files.is_empty() {
        println!("note: --graph-db/--taxonomy-db not given, skipping input-file hash re-verification");
    }

    let report = verify_release(
        &prov,
        &VerifyInputs {
            manifest: &manifest,
            judgments_provenance: &judgments_provenance,
            relations_provenance: &relations_provenance,
            live_input_files: &live_input_files,
        },
    )?;
    report.print();

    if !report.is_ok() {
        anyhow::bail!("release verification failed — {} check(s) failed", report.failures.len());
    }
    Ok(())
}

fn run_web_export(args: &[String]) -> Result<()> {
    let db = PathBuf::from(require_flag(args, "--db")?);
    let release_tag = require_flag(args, "--release")?.to_string();
    let out_dir = PathBuf::from(require_flag(args, "--out-dir")?);

    let prov = ProvenanceStore::open(&db).with_context(|| format!("{db:?} を開けません"))?;
    let release = prov
        .get_release_by_tag(&release_tag)?
        .with_context(|| format!("release '{release_tag}' not found — run import-legacy first"))?;

    let export = build_web_export(&prov, release.id)?;

    std::fs::create_dir_all(&out_dir)?;
    let dependencies_path = out_dir.join("dependencies.json");
    let morphisms_path = out_dir.join("morphisms.json");
    let relations_path = out_dir.join("relations.json");
    std::fs::write(&dependencies_path, serde_json::to_string(&export.dependencies)?)?;
    std::fs::write(&morphisms_path, serde_json::to_string(&export.morphisms)?)?;
    std::fs::write(&relations_path, serde_json::to_string(&export.relations)?)?;

    // 出力ファイル自身のマニフェスト。書き終えた**後**にハッシュを取る
    // ——「このバイト列を後で誰かが書き換えていないか」を`verify-release`が
    // 確かめられるようにする。
    let web_export_manifest = WebExportManifest {
        schema_version: WEB_EXPORT_SCHEMA_VERSION,
        release_tag: release_tag.clone(),
        release_id: release.id.0,
        release_git_commit: release.git_commit.clone(),
        web_export_version: WEB_EXPORT_VERSION.to_string(),
        generated_at_unix: SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0),
        counts: WebExportCounts {
            dependencies: export.dependencies.len(),
            morphisms: export.morphisms.len(),
            relations: export.relations.len(),
        },
        output_files: vec![input_file_hash(&dependencies_path)?, input_file_hash(&morphisms_path)?, input_file_hash(&relations_path)?],
    };
    std::fs::write(out_dir.join("web-export-manifest.json"), serde_json::to_string_pretty(&web_export_manifest)?)?;

    println!(
        "wrote {} dependencies, {} morphisms, {} relations to {} (+ web-export-manifest.json)",
        export.dependencies.len(),
        export.morphisms.len(),
        export.relations.len(),
        out_dir.display(),
    );
    Ok(())
}

fn run_verify_release(args: &[String]) -> Result<()> {
    let manifest_path = PathBuf::from(require_flag(args, "--manifest")?);
    let judgments_path = PathBuf::from(require_flag(args, "--judgments-provenance")?);
    let relations_path = PathBuf::from(require_flag(args, "--relations-provenance")?);
    let provenance_db = PathBuf::from(require_flag(args, "--provenance-db")?);
    let web_export_manifest_path = PathBuf::from(require_flag(args, "--web-export-manifest")?);
    let web_export_dir = PathBuf::from(require_flag(args, "--web-export-dir")?);
    let release_tag = require_flag(args, "--release")?.to_string();
    let graph_db = flag_value(args, "--graph-db").map(PathBuf::from);
    let taxonomy_db = flag_value(args, "--taxonomy-db").map(PathBuf::from);

    let manifest: ProvenanceManifest = serde_json::from_slice(
        &std::fs::read(&manifest_path).with_context(|| format!("{manifest_path:?} を開けません"))?,
    )
    .with_context(|| format!("{manifest_path:?} のパースに失敗"))?;
    let judgments_provenance: JudgmentsProvenanceExport = serde_json::from_slice(
        &std::fs::read(&judgments_path).with_context(|| format!("{judgments_path:?} を開けません"))?,
    )
    .with_context(|| format!("{judgments_path:?} のパースに失敗"))?;
    let relations_provenance: RelationsProvenanceExport = serde_json::from_slice(
        &std::fs::read(&relations_path).with_context(|| format!("{relations_path:?} を開けません"))?,
    )
    .with_context(|| format!("{relations_path:?} のパースに失敗"))?;
    let web_export_manifest: WebExportManifest = serde_json::from_slice(
        &std::fs::read(&web_export_manifest_path).with_context(|| format!("{web_export_manifest_path:?} を開けません"))?,
    )
    .with_context(|| format!("{web_export_manifest_path:?} のパースに失敗"))?;
    let prov = ProvenanceStore::open(&provenance_db).with_context(|| format!("{provenance_db:?} を開けません"))?;

    let mut live_input_files = Vec::new();
    if let Some(p) = &graph_db {
        live_input_files.push(input_file_hash(p)?);
    }
    if let Some(p) = &taxonomy_db {
        live_input_files.push(input_file_hash(p)?);
    }
    if live_input_files.is_empty() {
        println!("note: --graph-db/--taxonomy-db not given, skipping input-file hash re-verification");
    }

    println!("--- P1: sidecar completeness (verify) ---");
    let verify_report = verify_release(
        &prov,
        &VerifyInputs {
            manifest: &manifest,
            judgments_provenance: &judgments_provenance,
            relations_provenance: &relations_provenance,
            live_input_files: &live_input_files,
        },
    )?;
    verify_report.print();

    println!("--- P2: web-export integrity ---");
    let web_export_failures = verify_web_export(&prov, &release_tag, &web_export_manifest, &web_export_dir)?;
    print_web_export_failures(&web_export_failures);

    if !verify_report.is_ok() || !web_export_failures.is_empty() {
        anyhow::bail!(
            "release verification failed — {} sidecar problem(s), {} web-export problem(s)",
            verify_report.failures.len(),
            web_export_failures.len()
        );
    }
    Ok(())
}

fn run_build_catalog(args: &[String]) -> Result<()> {
    let graph_db = PathBuf::from(require_flag(args, "--graph-db")?);
    let taxonomy_db = PathBuf::from(require_flag(args, "--taxonomy-db")?);
    let db = PathBuf::from(require_flag(args, "--db")?);

    let graph = GraphStore::open(&graph_db).with_context(|| format!("{graph_db:?} を開けません"))?;
    let taxonomy = TaxonomyStore::open(&taxonomy_db).with_context(|| format!("{taxonomy_db:?} を開けません"))?;
    let prov = ProvenanceStore::open(&db).with_context(|| format!("{db:?} を開けません"))?;

    let graph_stats = prov.transaction(|| build_judgment_paper_catalog(&graph, &prov))?;
    println!(
        "judgment/paper catalog: judgments +{} (skip {}), papers +{} (skip {})",
        graph_stats.judgments, graph_stats.judgments_skipped_existing, graph_stats.papers, graph_stats.papers_skipped_existing
    );

    let concept_stats = prov.transaction(|| build_concept_catalog(&taxonomy, &prov))?;
    println!(
        "concept catalog: concepts +{} (skip {}), {} alias references mapped this run",
        concept_stats.concepts, concept_stats.concepts_skipped_existing, concept_stats.concept_aliases
    );

    let coverage = mathesis_provenance::catalog_adapter::assertion_reference_coverage(&prov)?;
    coverage.print();
    Ok(())
}

//! CLI。`mathesis-taxonomy`/`mathesis-annotate`と同じ「1バイナリ複数
//! サブコマンド、`clap`は使わず`flag_value`ヘルパーで自前パース」方式。

use anyhow::{Context, Result};
use mathesis_graph::GraphStore;
use mathesis_provenance::assertion_export::export_assertion_details;
use mathesis_provenance::catalog_adapter::{build_concept_catalog, build_judgment_paper_catalog, catalog_metadata};
use mathesis_provenance::lean_manifest_adapter;
use mathesis_provenance::legacy_adapter::{import_graph, import_taxonomy_relations, ADAPTER_NAME, ADAPTER_VERSION};
use mathesis_provenance::discovery_export::build_discovery_export;
use mathesis_provenance::math_graph_adapter::{self, PilotEdge, PilotStatement};
use mathesis_provenance::msc_adapter;
use mathesis_provenance::msc_classification;
use mathesis_provenance::pilot_artifact::{self, ScopeReport};
use mathesis_provenance::openalex_adapter;
use mathesis_provenance::openalex_fetch::{self, SnapshotEntry};
use mathesis_provenance::manifest::{
    input_file_hash, CatalogManifest, ManifestCounts, ProvenanceManifest, WebExportCounts, WebExportManifest, SCHEMA_VERSION,
    WEB_EXPORT_SCHEMA_VERSION,
};
use mathesis_provenance::relation_policy::SOURCE_MAPPING_POLICY_VERSION;
use mathesis_provenance::model::{
    AssertionId, EpistemicState, EvidenceKind, NewEvidence, NewRelationAssertion, NewRelease, NewReviewDecision,
    NewSourceRecord, ReviewId, ReviewOutcome,
};
use mathesis_provenance::review::is_authenticated_accept;
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
         \x20     引けるか」を見られるようにするだけ(docs/P3_STATUS.md参照)。\n\
         \x20 fetch-openalex --graph-db <path> --out <snapshot.json>\n\
         \x20     P4(docs/P4_PLAN.md): mathesis-graphのpapersテーブルにある\n\
         \x20     論文(種)だけをOpenAlex APIから取得し、スナップショットJSONへ\n\
         \x20     書き出す——OpenAlex全体をクロールしない、雪だるま式収集。\n\
         \x20     ライブAPIを叩く唯一のコマンド。\n\
         \x20 import-openalex --db <path> --release <tag> --snapshot <snapshot.json>\n\
         \x20     fetch-openalexが書いたスナップショットを証拠層へ写す\n\
         \x20     (Paper entity + Cites assertion, epistemic_state: observed)。\n\
         \x20     ネットワークに一切触れない、冪等な純粋インポート。\n\
         \x20     --releaseはimport-legacyで既に作成済みのタグを指定する。\n\
         \x20 review --db <path> --assertion-id <id> --decision <accept|reject|supersede|revoke>\n\
         \x20        --reviewer-id <id> --authorization-level <level> --release <tag>\n\
         \x20        [--rationale <text>] [--scope <text>] [--supersedes <review-id>]\n\
         \x20        [--expires-at <unix>]\n\
         \x20     P6.3（docs/P6_3_STATUS.md）: 本人確認済みレビューを1件、追記専用の\n\
         \x20     review_decisionsログへ記録する——`mathesis-annotate`が層3の射を\n\
         \x20     人間に承認させるのと同じ役割を、証拠層の(型クラス階層・生成物などで\n\
         \x20     機械的には裏付けられない)意味的関係に対して果たす唯一の書き込み口。\n\
         \x20     --release は今のリリースタグをそのまま dataset_version として記録する\n\
         \x20     ——リリースゲート(verify-release)はこれが検証対象のリリースと一致し、\n\
         \x20     期限切れでなく、直近の実効判断であることまで確かめる。\n\
         \x20 promote-review --db <path> --assertion-id <id> --to-state <reviewed|verified> --release <tag>\n\
         \x20                --reviewer-id <id> --authorization-level <level> [--rationale <text>]\n\
         \x20     P6.3（docs/P6_3_STATUS.md）: `review`単独では、意味的関係が最初から\n\
         \x20     `reviewed`/`verified`で無ければ何も信頼されない(既定トラバース対象は\n\
         \x20     epistemic_state自体もチェックする)——だが`legacy_adapter`/`taxonomy`は\n\
         \x20     `extracted`/`proposed`しか作らない。このコマンドが両者の橋渡し:\n\
         \x20     既存assertionと同じsubject/predicate/objectで新しいepistemic_stateの\n\
         \x20     assertionを1件`supersedes_id`付きで積み(元の行は変更しない、追記のみ)、\n\
         \x20     手書きの根拠を`reviewer_note`のEvidenceとして添え、その新しいassertionに\n\
         \x20     対して`review`と同じ本人確認済みaccept決定を1件記録する——3つの書き込みを\n\
         \x20     1トランザクションにまとめた、唯一の「意味的関係を本人確認済みレビューで\n\
         \x20     昇格させる」経路。\n\
         \x20 import-lean-manifest --db <path> --graph-db <path> --release <tag>\n\
         \x20                      --arxiv-id <id> --manifest <manifest.json>\n\
         \x20                      [--project-commit <sha>]\n\
         \x20     Priority 2, step 1: crates/mathesis-lean-extractが書き出した\n\
         \x20     本物のLean elaborator依存マニフェストを取り込む\n\
         \x20     (depends_on assertion, epistemic_state: observed,\n\
         \x20     evidence_kind: formal_export)。既存のテキスト抽出\n\
         \x20     (judgment_dependency:...)とは別のlegacy_ref名前空間を使う\n\
         \x20     ので両方が共存する——置き換えではなく比較用の追加。\n\
         \x20 import-math-graph --db <path> --release <tag>\n\
         \x20                   --statements <pilot_statements.json> --edges <pilot_edges.json>\n\
         \x20                   --dataset-revision <content_hash_or_note>\n\
         \x20     P7（docs/P7_STATUS.md）: uw-math-ai/math-graph（CC BY 4.0）のLeanGraphを、\n\
         \x20     scratch/math_graph_pilot/scope_pilot.pyが絞り込んだスコープ(P6.2の\n\
         \x20     2つのMathlib名前空間と同じ)ぶんだけ取り込む。多GBの生CSVはこの\n\
         \x20     コマンド自身は一切開かない——絞り込み済みの2つの小さいJSONだけを読む。\n\
         \x20     epistemic_state: extracted（observedではない、独立検証していないため）、\n\
         \x20     evidence_kind: formal_export、subject_ref/object_refは\n\
         \x20     judgment:mathgraph:<uuid>という別名前空間——既定では\n\
         \x20     dependencies.jsonにも既定トラバース対象にもならない。\n\
         \x20 classify-msc --db <path> --taxonomy-db <path> --release <tag>\n\
         \x20     改善点.txt項目9（docs/PA_3_STATUS.md）: mathesis-taxonomy自身の\n\
         \x20     cluster_alignmentsを読み、classified/pending/unclassifiedの3状態へ\n\
         \x20     分類してmsc_classificationsへ書く（unavailable/outside_scope/rejectedは\n\
         \x20     型として予約済みだがこのコマンドでは1件も生成しない——理由は\n\
         \x20     msc_classification.rsのdocコメント参照）。(cluster_id, release)で\n\
         \x20     冪等——同じリリースへの再実行は上書きのみ。\n\
         \x20 export-discovery --db <path> --release <tag> --project-label <label> --out <path>\n\
         \x20     P7.4（docs/P7_4_STATUS.md）: 既存のdependencies.json/web-exportとは\n\
         \x20     完全に独立した、比較/発見モードUI専用の読み取りモデルを書き出す。\n\
         \x20     4種の出典(mathesis-checker/mathesis-text/math-graph-literal/\n\
         \x20     math-graph-hierarchy)を明示的に分類し、subject/objectは\n\
         \x20     entity_label_with_originの表示名で出す。本番のdependencies.json/\n\
         \x20     検索インデックス/既定の信頼グラフには一切書き込まない——追加専用の別ファイル。\n\
         \x20 export-pilot-manifest --db <path> --release <tag> --dataset-revision <hash>\n\
         \x20                       --retrieved-at <unix秒> --scope-report <path>\n\
         \x20                       [--raw-source-file <path>]... [--index-file <path>]... --out <path>\n\
         \x20     P8.1（docs/P8_1_STATUS.md）: オフラインTheoremGraph/Math-Graphコネクタ\n\
         \x20     の「artifact」——隔離パイロットDB専用の自己記述マニフェスト。データセット\n\
         \x20     URL/リビジョン・取得時刻・生ソースCSVのハッシュ・スキーマ/アダプタ版・\n\
         \x20     ライセンス・プロジェクト別内訳・DB内の実件数・未解決/重複件数・MSC分類\n\
         \x20     状態別件数（Lean宣言は分類対象外なので全件unavailableと正直に記録）・\n\
         \x20     読み取りモデル(export-discoveryの出力)のハッシュを1つにまとめる。"
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
        Some("import-msc") => run_import_msc(&args[2..]),
        Some("classify-msc") => run_classify_msc(&args[2..]),
        Some("fetch-openalex") => run_fetch_openalex(&args[2..]),
        Some("import-openalex") => run_import_openalex(&args[2..]),
        Some("import-lean-manifest") => run_import_lean_manifest(&args[2..]),
        Some("review") => run_review(&args[2..]),
        Some("promote-review") => run_promote_review(&args[2..]),
        Some("import-math-graph") => run_import_math_graph(&args[2..]),
        Some("export-discovery") => run_export_discovery(&args[2..]),
        Some("export-pilot-manifest") => run_export_pilot_manifest(&args[2..]),
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

fn run_import_msc(args: &[String]) -> Result<()> {
    let db = PathBuf::from(require_flag(args, "--db")?);
    let release_tag = require_flag(args, "--release")?.to_string();
    let prov = ProvenanceStore::open(&db).with_context(|| format!("{db:?} を開けません"))?;
    let release = prov
        .get_release_by_tag(&release_tag)?
        .with_context(|| format!("release '{release_tag}' not found — run import-legacy first"))?;
    let stats = prov.transaction(|| msc_adapter::import(&prov, release.id))?;
    println!(
        "MSC2020: concepts +{} (skip {}), hierarchy assertions +{} (skip {})",
        stats.concepts_imported, stats.concepts_skipped,
        stats.relations_imported, stats.relations_skipped
    );
    Ok(())
}

/// 改善点.txt項目9（`docs/PA_3_STATUS.md`）: `mathesis-taxonomy`の
/// `cluster_alignments`を読み、6状態モデルへ分類して`msc_classifications`
/// へ書く。`import-msc`（MSC2020オントロジー自体のimport）とは別コマンド
/// ——後者はコード階層、こちらはクラスタの分類結果で、対象も出力先の列も
/// 重ならない。
fn run_classify_msc(args: &[String]) -> Result<()> {
    let db = PathBuf::from(require_flag(args, "--db")?);
    let taxonomy_db = PathBuf::from(require_flag(args, "--taxonomy-db")?);
    let release_tag = require_flag(args, "--release")?.to_string();
    let prov = ProvenanceStore::open(&db).with_context(|| format!("{db:?} を開けません"))?;
    let release = prov
        .get_release_by_tag(&release_tag)?
        .with_context(|| format!("release '{release_tag}' not found — run import-legacy first"))?;
    let stats = prov.transaction(|| msc_classification::classify_from_taxonomy(&prov, &taxonomy_db, release.id))?;
    println!(
        "MSC classification status: {} classified, {} pending, {} unclassified ({} clusters total)",
        stats.classified, stats.pending, stats.unclassified, stats.total_clusters
    );
    Ok(())
}

/// P4: 種論文(mathesis-graphの`papers`)だけをOpenAlexから取得する。
/// ライブAPIに触れる唯一のコマンド——`import-openalex`はこの出力
/// (スナップショットJSON)だけを読み、ネットワークには触れない。
fn run_fetch_openalex(args: &[String]) -> Result<()> {
    let graph_db = PathBuf::from(require_flag(args, "--graph-db")?);
    let out_path = PathBuf::from(require_flag(args, "--out")?);
    let graph = GraphStore::open(&graph_db).with_context(|| format!("{graph_db:?} を開けません"))?;

    let papers = graph.list_papers()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let mut snapshot: Vec<SnapshotEntry> = Vec::new();
    let mut not_found = 0usize;
    for (i, p) in papers.iter().enumerate() {
        if i > 0 {
            openalex_fetch::courtesy_wait();
        }
        match openalex_fetch::fetch_work_by_arxiv_id(&p.arxiv_id, now) {
            Ok(Some(entry)) => snapshot.push(entry),
            Ok(None) => not_found += 1,
            Err(e) => eprintln!("警告: {} の取得に失敗、スキップ: {e}", p.arxiv_id),
        }
    }
    snapshot.sort_by(|a, b| a.arxiv_id.cmp(&b.arxiv_id));

    std::fs::write(&out_path, serde_json::to_string_pretty(&snapshot)?)
        .with_context(|| format!("{out_path:?} への書き込みに失敗"))?;
    println!(
        "fetched {}/{} papers from OpenAlex (not found: {}), wrote {}",
        snapshot.len(),
        papers.len(),
        not_found,
        out_path.display()
    );
    Ok(())
}

/// P4: `fetch-openalex`が書いたスナップショットを証拠層へ写す。純粋
/// インポート——ネットワークに一切触れない。
fn run_import_openalex(args: &[String]) -> Result<()> {
    let db = PathBuf::from(require_flag(args, "--db")?);
    let release_tag = require_flag(args, "--release")?.to_string();
    let snapshot_path = PathBuf::from(require_flag(args, "--snapshot")?);

    let prov = ProvenanceStore::open(&db).with_context(|| format!("{db:?} を開けません"))?;
    let release = prov
        .get_release_by_tag(&release_tag)?
        .with_context(|| format!("release '{release_tag}' not found — run import-legacy first"))?;
    let snapshot: Vec<SnapshotEntry> = serde_json::from_slice(
        &std::fs::read(&snapshot_path).with_context(|| format!("{snapshot_path:?} を開けません"))?,
    )
    .with_context(|| format!("{snapshot_path:?} のパースに失敗"))?;

    let stats = prov.transaction(|| openalex_adapter::import(&prov, release.id, &snapshot))?;
    println!(
        "OpenAlex: papers linked +{} (already linked {}), citations +{} (skip {}), {} references outside the catalog (ignored)",
        stats.papers_linked,
        stats.papers_already_linked,
        stats.citations_imported,
        stats.citations_skipped_existing,
        stats.references_outside_catalog,
    );
    openalex_adapter::citation_coverage(&prov)?.print();
    Ok(())
}

/// Priority 2, step 1（ユーザー指示 2026-09-08）: `crates/mathesis-lean-extract`
/// が書き出したLean elaborator由来の依存マニフェストを取り込む。
/// `--graph-db`はjudgment名の解決専用——書き込みはしない。
fn run_import_lean_manifest(args: &[String]) -> Result<()> {
    let db = PathBuf::from(require_flag(args, "--db")?);
    let graph_db = PathBuf::from(require_flag(args, "--graph-db")?);
    let release_tag = require_flag(args, "--release")?.to_string();
    let arxiv_id = require_flag(args, "--arxiv-id")?.to_string();
    let manifest_path = PathBuf::from(require_flag(args, "--manifest")?);
    // P6.1（`docs/LEAN_DEPENDENCY_POLICY.md`）: `import-legacy --git-commit`と
    // 同じ流儀——Leanはこの情報を知りようがないので、呼び出し元(このCLI)から
    // 渡す。無ければ捏造せず`None`のまま。
    let project_commit = flag_value(args, "--project-commit").map(str::to_string);

    let prov = ProvenanceStore::open(&db).with_context(|| format!("{db:?} を開けません"))?;
    let graph = GraphStore::open(&graph_db).with_context(|| format!("{graph_db:?} を開けません"))?;
    let release = prov
        .get_release_by_tag(&release_tag)?
        .with_context(|| format!("release '{release_tag}' not found — run import-legacy first"))?;
    let raw = std::fs::read_to_string(&manifest_path).with_context(|| format!("{manifest_path:?} を開けません"))?;
    let retrieved_at_unix = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);

    let stats = prov.transaction(|| {
        lean_manifest_adapter::import_lean_manifest(
            &prov,
            &graph,
            release.id,
            &arxiv_id,
            &raw,
            retrieved_at_unix,
            Some(&manifest_path.to_string_lossy()),
            project_commit.as_deref(),
        )
    })?;
    println!(
        "Lean manifest ({arxiv_id}): dependencies +{} (skip {}), declarations unmatched {}, dependency targets unmatched {}",
        stats.dependencies_imported, stats.dependencies_skipped_existing, stats.declarations_unmatched, stats.dependency_targets_unmatched,
    );
    lean_manifest_adapter::compare_dependency_sources(&prov, &graph, release.id, &arxiv_id, &raw)?.print();
    Ok(())
}

/// P7（`docs/P7_STATUS.md`）: `scope_pilot.py`が書き出した、スコープを
/// 絞ったJSON2つ(宣言・依存辺)だけを読む。生のMath-Graph CSV(計13.6GB)は
/// このバイナリのどのコードパスからも開かない——Pythonの前処理でだけ触れる、
/// `openalex_fetch.rs`(ネットワーク取得)と`openalex_adapter.rs`(純粋な
/// インポート)の分離と同じ設計。
fn run_import_math_graph(args: &[String]) -> Result<()> {
    let db = PathBuf::from(require_flag(args, "--db")?);
    let release_tag = require_flag(args, "--release")?.to_string();
    let statements_path = PathBuf::from(require_flag(args, "--statements")?);
    let edges_path = PathBuf::from(require_flag(args, "--edges")?);
    let dataset_revision = require_flag(args, "--dataset-revision")?.to_string();

    let prov = ProvenanceStore::open(&db).with_context(|| format!("{db:?} を開けません"))?;
    let release = prov
        .get_release_by_tag(&release_tag)?
        .with_context(|| format!("release '{release_tag}' not found — run import-legacy first"))?;

    let statements: Vec<PilotStatement> = serde_json::from_slice(
        &std::fs::read(&statements_path).with_context(|| format!("{statements_path:?} を開けません"))?,
    )
    .with_context(|| format!("{statements_path:?} のパースに失敗"))?;
    let edges: Vec<PilotEdge> = serde_json::from_slice(
        &std::fs::read(&edges_path).with_context(|| format!("{edges_path:?} を開けません"))?,
    )
    .with_context(|| format!("{edges_path:?} のパースに失敗"))?;

    let stats = prov.transaction(|| math_graph_adapter::import_pilot(&prov, release.id, &statements, &edges, &dataset_revision))?;
    println!(
        "Math-Graph pilot: declarations +{} (skip {}), dependencies +{} (skip {}), {} dependency targets outside the pilot scope (ignored)",
        stats.declarations_imported,
        stats.declarations_skipped_existing,
        stats.dependencies_imported,
        stats.dependencies_skipped_existing,
        stats.dependencies_outside_pilot_scope,
    );
    Ok(())
}

/// P7.4（`docs/P7_4_STATUS.md`）: 比較/発見モードUI専用の読み取りモデルを
/// 書き出す。`dependencies.json`/`web-export`の生成コードには一切触れない
/// ——別の関数(`discovery_export::build_discovery_export`)を通す独立した
/// 経路。
fn run_export_discovery(args: &[String]) -> Result<()> {
    let db = PathBuf::from(require_flag(args, "--db")?);
    let release_tag = require_flag(args, "--release")?.to_string();
    let project_label = require_flag(args, "--project-label")?.to_string();
    let out = PathBuf::from(require_flag(args, "--out")?);

    let prov = ProvenanceStore::open(&db).with_context(|| format!("{db:?} を開けません"))?;
    let release = prov
        .get_release_by_tag(&release_tag)?
        .with_context(|| format!("release '{release_tag}' not found"))?;
    let export = build_discovery_export(&prov, release.id, &release_tag, &project_label)?;
    std::fs::write(&out, serde_json::to_string(&export)?).with_context(|| format!("{out:?} へ書き込めません"))?;
    println!(
        "discovery export ({project_label}): {} edges — mathesis-checker {}, mathesis-text {}, math-graph-literal {}, math-graph-hierarchy {} -> {out:?}",
        export.edges.len(),
        export.counts.mathesis_checker,
        export.counts.mathesis_text,
        export.counts.math_graph_literal,
        export.counts.math_graph_hierarchy,
    );
    Ok(())
}

/// P8.1（`docs/P8_1_STATUS.md`）: the offline TheoremGraph/Math-Graph
/// connector artifact's self-describing release manifest. Reads real
/// counts from the isolated pilot DB itself (never production), the
/// Python-side scope/validation report, and hashes of both the raw
/// source CSVs and the sibling read-model export (`export-discovery`,
/// reused unchanged as this artifact's "small read-model prototype").
fn run_export_pilot_manifest(args: &[String]) -> Result<()> {
    let db = PathBuf::from(require_flag(args, "--db")?);
    let release_tag = require_flag(args, "--release")?.to_string();
    let dataset_revision = require_flag(args, "--dataset-revision")?.to_string();
    let retrieved_at_unix: i64 = require_flag(args, "--retrieved-at")?
        .parse()
        .context("--retrieved-at はUNIX秒の整数で指定してください")?;
    let scope_report_path = PathBuf::from(require_flag(args, "--scope-report")?);
    let out = PathBuf::from(require_flag(args, "--out")?);
    // 生ソースCSVは複数(--raw-source-file を繰り返し指定)。読み取りモデルの
    // 出力(export-discoveryの出力、複数プロジェクトぶんある)も同様。
    let raw_source_files: Vec<PathBuf> =
        args.iter().enumerate().filter(|(_, a)| *a == "--raw-source-file").map(|(i, _)| PathBuf::from(&args[i + 1])).collect();
    let index_files: Vec<PathBuf> =
        args.iter().enumerate().filter(|(_, a)| *a == "--index-file").map(|(i, _)| PathBuf::from(&args[i + 1])).collect();

    let prov = ProvenanceStore::open(&db).with_context(|| format!("{db:?} を開けません"))?;
    let release = prov
        .get_release_by_tag(&release_tag)?
        .with_context(|| format!("release '{release_tag}' not found"))?;

    let scope_report: ScopeReport = serde_json::from_slice(
        &std::fs::read(&scope_report_path).with_context(|| format!("{scope_report_path:?} を開けません"))?,
    )
    .with_context(|| format!("{scope_report_path:?} のパースに失敗"))?;

    let raw_source_file_hashes = raw_source_files.iter().map(|p| input_file_hash(p)).collect::<anyhow::Result<Vec<_>>>()?;
    let generated_index_hashes = index_files.iter().map(|p| input_file_hash(p)).collect::<anyhow::Result<Vec<_>>>()?;

    let generated_at_unix = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let manifest = pilot_artifact::build_manifest(
        &prov,
        &release_tag,
        release.id.0,
        &dataset_revision,
        retrieved_at_unix,
        raw_source_file_hashes,
        scope_report,
        generated_index_hashes,
        generated_at_unix,
    )?;
    std::fs::write(&out, serde_json::to_string_pretty(&manifest)?).with_context(|| format!("{out:?} へ書き込めません"))?;
    println!(
        "pilot artifact manifest: {} declarations ({} literal, {} typeclass-hierarchy, {} excluded), {} edges, {} db source records -> {out:?}",
        manifest.totals.declarations,
        manifest.totals.declarations_literal,
        manifest.totals.declarations_typeclass_hierarchy,
        manifest.totals.declarations_excluded,
        manifest.totals.edges_imported,
        manifest.source_record_count_in_db,
    );
    Ok(())
}

/// P6.3（`docs/P6_3_STATUS.md`）: 本人確認済みレビューを記録する唯一の書き込み口。
/// `--authorization-level`を必須にするのは意図的——「誰が」だけでなく
/// 「どんな資格で」を毎回明示させることで、リリースゲート
/// (`review::is_authenticated_accept`)が空文字列やNoneを本人確認済みと
/// 取り違えないようにする。`--release`はそのままdataset_versionへ入る
/// ——「このレビューはどのリリース時点の内容を見て判断したか」を自己申告
/// させ、後で別リリースへ確認なしに横流しされないようにする(ゲート側の
/// "no drift"チェック)。
fn run_review(args: &[String]) -> Result<()> {
    let db = PathBuf::from(require_flag(args, "--db")?);
    let assertion_id = require_flag(args, "--assertion-id")?
        .parse::<i64>()
        .context("--assertion-id は整数である必要があります")?;
    let decision_str = require_flag(args, "--decision")?;
    let decision = ReviewOutcome::from_str(decision_str)
        .with_context(|| format!("未知の --decision '{decision_str}' (accept/reject/supersede/revoke/split/merge/needs_expert のいずれか)"))?;
    let reviewer_id = require_flag(args, "--reviewer-id")?.to_string();
    let authorization_level = require_flag(args, "--authorization-level")?.to_string();
    let release_tag = require_flag(args, "--release")?.to_string();
    let rationale = flag_value(args, "--rationale").map(str::to_string);
    let scope = flag_value(args, "--scope").map(str::to_string);
    let supersedes_review_id = flag_value(args, "--supersedes")
        .map(|s| s.parse::<i64>().context("--supersedes は整数である必要があります"))
        .transpose()?
        .map(ReviewId);
    let expires_at_unix = flag_value(args, "--expires-at")
        .map(|s| s.parse::<i64>().context("--expires-at はUNIX秒である必要があります"))
        .transpose()?;

    let prov = ProvenanceStore::open(&db).with_context(|| format!("{db:?} を開けません"))?;
    let assertion = prov
        .try_get_assertion(AssertionId(assertion_id))?
        .with_context(|| format!("assertion #{assertion_id} が見つかりません"))?;
    let release = prov
        .get_release_by_tag(&release_tag)?
        .with_context(|| format!("release '{release_tag}' not found"))?;
    if assertion.release_id.0 != release.id.0 {
        eprintln!(
            "警告: assertion #{assertion_id} は release_id={} ですが --release '{release_tag}' は id={} を指します（別リリースのassertionをレビューしようとしていないか確認してください）",
            assertion.release_id.0, release.id.0
        );
    }

    let now_unix = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let review_id = prov.insert_review_decision(&NewReviewDecision {
        assertion_id: AssertionId(assertion_id),
        decision,
        reviewer_id: Some(reviewer_id.clone()),
        authorization_level: Some(authorization_level.clone()),
        scope,
        rationale,
        decided_at_unix: now_unix,
        dataset_version: Some(release_tag.clone()),
        expires_at_unix,
        supersedes_review_id,
    })?;

    println!(
        "review #{} recorded: assertion #{assertion_id} {} by {reviewer_id} ({authorization_level}), release '{release_tag}'",
        review_id.0,
        decision.as_str(),
    );

    let effective = prov.effective_review_decision(AssertionId(assertion_id))?;
    match effective {
        Some(d) if d.id == review_id => {
            let authenticated = is_authenticated_accept(&d, now_unix, &release_tag);
            println!(
                "this is now the effective decision for assertion #{assertion_id} — counts as an authenticated accept for the release gate: {authenticated}",
            );
        }
        Some(d) => println!(
            "note: assertion #{assertion_id}'s effective decision is still review #{} ({}) — this one was recorded but is not the most recent",
            d.id.0,
            d.decision.as_str()
        ),
        None => unreachable!("just inserted a decision for this assertion"),
    }
    Ok(())
}

/// P6.3（`docs/P6_3_STATUS.md`）: `review`だけでは意味的関係を信頼できない
/// ——`traversal_policy`はepistemic_stateも見るので、legacy_adapter/taxonomyが
/// 作る`extracted`/`proposed`のままではacceptが何件あろうと`default_traversal`に
/// 届かない。このコマンドが「既存assertionと同じ内容で新しいepistemic_stateの
/// assertionを積む(supersedes_id付き、元の行は不変) + reviewer_note証拠を添える +
/// 本人確認済みaccept決定を記録する」の3手順を1トランザクションでまとめる。
fn run_promote_review(args: &[String]) -> Result<()> {
    let db = PathBuf::from(require_flag(args, "--db")?);
    let old_assertion_id = require_flag(args, "--assertion-id")?
        .parse::<i64>()
        .context("--assertion-id は整数である必要があります")?;
    let to_state_str = require_flag(args, "--to-state")?;
    let to_state = EpistemicState::from_str(to_state_str)
        .with_context(|| format!("未知の --to-state '{to_state_str}' (reviewed/verified など)"))?;
    let release_tag = require_flag(args, "--release")?.to_string();
    let reviewer_id = require_flag(args, "--reviewer-id")?.to_string();
    let authorization_level = require_flag(args, "--authorization-level")?.to_string();
    let rationale = flag_value(args, "--rationale").map(str::to_string);
    let scope = flag_value(args, "--scope").map(str::to_string);
    let expires_at_unix = flag_value(args, "--expires-at")
        .map(|s| s.parse::<i64>().context("--expires-at はUNIX秒である必要があります"))
        .transpose()?;

    let prov = ProvenanceStore::open(&db).with_context(|| format!("{db:?} を開けません"))?;
    let old_assertion = prov
        .try_get_assertion(AssertionId(old_assertion_id))?
        .with_context(|| format!("assertion #{old_assertion_id} が見つかりません"))?;
    let release = prov
        .get_release_by_tag(&release_tag)?
        .with_context(|| format!("release '{release_tag}' not found"))?;
    if old_assertion.release_id.0 != release.id.0 {
        anyhow::bail!(
            "assertion #{old_assertion_id} は release_id={} ですが --release '{release_tag}' は id={} を指します",
            old_assertion.release_id.0,
            release.id.0
        );
    }

    let now_unix = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (new_assertion_id, review_id) = prov.transaction(|| -> anyhow::Result<(AssertionId, ReviewId)> {
        let new_assertion_id = prov.insert_assertion(&NewRelationAssertion {
            subject_ref: old_assertion.subject_ref.clone(),
            predicate: old_assertion.predicate,
            object_ref: old_assertion.object_ref.clone(),
            epistemic_state: to_state,
            score: old_assertion.score,
            policy_version: old_assertion.policy_version.clone(),
            created_by_run_id: Some(format!("mathesis-provenance promote-review by {reviewer_id}")),
            supersedes_id: Some(old_assertion.id),
            release_id: release.id,
            legacy_ref: None,
        })?;
        // 手書きの根拠を持つ人間のレビュー由来のsource_record。
        // `(provider, provider_id, provider_revision)`の一意制約により
        // 同じreviewer_idでの再実行は既存行を再利用する。
        let source_record_id = prov.get_or_insert_source_record(&NewSourceRecord {
            provider: "manual-review".into(),
            provider_id: reviewer_id.clone(),
            provider_revision: None,
            retrieved_at_unix: Some(now_unix),
            content_hash: None,
            licence: None,
            attribution: None,
            raw_payload_uri: None,
            adapter_name: "mathesis-provenance promote-review".into(),
            adapter_version: env!("CARGO_PKG_VERSION").to_string(),
            parser_version: None,
            reproducibility_json: None,
        })?;
        prov.insert_evidence(&NewEvidence {
            assertion_id: new_assertion_id,
            source_record_id,
            locator: rationale.clone(),
            evidence_kind: EvidenceKind::ReviewerNote,
            extractor_or_model: None,
            version: None,
            input_hash: None,
            output_hash: None,
            metric_name: None,
            metric_value: None,
            dependency_origin: None,
            external_classification: None,
        })?;
        let review_id = prov.insert_review_decision(&NewReviewDecision {
            assertion_id: new_assertion_id,
            decision: ReviewOutcome::Accept,
            reviewer_id: Some(reviewer_id.clone()),
            authorization_level: Some(authorization_level.clone()),
            scope,
            rationale,
            decided_at_unix: now_unix,
            dataset_version: Some(release_tag.clone()),
            expires_at_unix,
            supersedes_review_id: None,
        })?;
        Ok((new_assertion_id, review_id))
    })?;

    println!(
        "assertion #{old_assertion_id} promoted to '{}' as new assertion #{} (supersedes #{old_assertion_id}), review #{} recorded",
        to_state.as_str(),
        new_assertion_id.0,
        review_id.0,
    );
    let policy = mathesis_provenance::relation_policy::traversal_policy(old_assertion.predicate, to_state);
    println!("traversal_policy for the new assertion: {}", policy.as_str());
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
    //
    // P6.3（`docs/P6_3_STATUS.md`）: レガシーサイドカー由来のid集合だけでは
    // 足りないと実データで判明した——`promote-review`が作るassertionは
    // legacy_refを持たない(対応する旧mathesis-graph/taxonomy行が無い)ため、
    // reconcileの`legacy_ref`走査に一切引っかからない。結果、そのassertionは
    // `relations.json`には正しく現れるのに`assertions.json`には載らず、
    // provenanceパネルが「見つかりません」を返す——という食い違いを実際に
    // 起こしてから見つけた。`build_web_export`が実際に書き出すid集合
    // (=UIが実際にクリックしうる辺の全体)をここでも計算し、レガシー由来の
    // id集合と合わせて重複排除する——`web-export`は別コマンドとして独立に
    // 実行されるので、ここで計算し直す以外に整合を取る方法が無い。
    let live_web_export = build_web_export(&prov, release.id)?;
    let all_assertion_ids: std::collections::BTreeSet<i64> = judgments_export
        .dependencies
        .iter()
        .map(|d| d.assertion_id)
        .chain(judgments_export.citations.iter().map(|c| c.assertion_id))
        .chain(judgments_export.morphisms.iter().map(|m| m.assertion_id))
        .chain(relations_export.relations.iter().map(|r| r.assertion_id))
        .chain(live_web_export.dependencies.iter().map(|d| d.assertion_id))
        .chain(live_web_export.morphisms.iter().map(|m| m.id))
        .chain(live_web_export.relations.iter().map(|r| r.assertion_id))
        .collect();
    let now_unix = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let assertions = export_assertion_details(&prov, &release_tag, now_unix, all_assertion_ids)?;
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
        source_mapping_policy_version: SOURCE_MAPPING_POLICY_VERSION.to_string(),
        input_files: vec![input_file_hash(&graph_db)?, input_file_hash(&taxonomy_db)?],
        generated_at_unix: SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0),
        counts: ManifestCounts {
            dependencies: judgments_export.dependencies.len(),
            citations: judgments_export.citations.len(),
            morphisms: judgments_export.morphisms.len(),
            relations: relations_export.relations.len(),
        },
        catalog: prov.catalog_metadata()?.map(|c| CatalogManifest {
            schema_version: c.schema_version,
            build_version: c.build_version,
            entity_resolution_version: c.entity_resolution_version,
            graph_input_sha256: c.graph_input_sha256,
            taxonomy_input_sha256: c.taxonomy_input_sha256,
            entity_count: c.entity_count,
            alias_count: c.alias_count,
        }),
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

    let now_unix = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let report = verify_release(
        &prov,
        &VerifyInputs {
            manifest: &manifest,
            judgments_provenance: &judgments_provenance,
            relations_provenance: &relations_provenance,
            live_input_files: &live_input_files,
            now_unix,
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
    let now_unix = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let verify_report = verify_release(
        &prov,
        &VerifyInputs {
            manifest: &manifest,
            judgments_provenance: &judgments_provenance,
            relations_provenance: &relations_provenance,
            live_input_files: &live_input_files,
            now_unix,
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

    let metadata = catalog_metadata(
        &prov,
        input_file_hash(&graph_db)?.sha256,
        input_file_hash(&taxonomy_db)?.sha256,
    )?;
    prov.replace_catalog_metadata(&metadata)?;
    let backfill = prov.backfill_assertion_entity_ids()?;
    if backfill.unresolved > 0 {
        anyhow::bail!("catalog backfill left {} assertions with unresolved endpoints", backfill.unresolved);
    }
    println!(
        "assertion entity endpoints: +{} backfilled (already correct: {})",
        backfill.newly_backfilled, backfill.already_correct
    );
    println!(
        "catalog metadata: schema {}, build {}, resolution {}, entities {}, aliases {}",
        metadata.schema_version,
        metadata.build_version,
        metadata.entity_resolution_version,
        metadata.entity_count,
        metadata.alias_count
    );

    let coverage = mathesis_provenance::catalog_adapter::assertion_reference_coverage(&prov)?;
    coverage.print();
    Ok(())
}

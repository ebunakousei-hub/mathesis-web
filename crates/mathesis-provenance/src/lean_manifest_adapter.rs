//! Priority 2, step 1（ユーザー指示 2026-09-08）: 本物のLean elaboratorが
//! 出力した依存関係マニフェストを`depends_on`アサーションとして取り込む。
//! `epistemic_state: observed`の**最初の実使用**——`docs/DATA_DICTIONARY.md`
//! の当初の設計判断で「`observed`は本物のLean-exported manifestのために
//! 予約する（まだ作っていない）」とされていたもの、ARCHITECTURE_NEXT.md
//! §4.2が`observed`の代表例として名指ししている"a Lean-exported
//! dependency"そのもの。
//!
//! 追加的かつ比較用——既存のテキスト抽出`depends_on`
//! (`legacy_ref: "judgment_dependency:..."`, `epistemic_state: extracted`,
//! `crates/mathesis-importer/src/dependencies.rs`の識別子の名前一致)は
//! 一切変更しない。このアダプタのアサーションは別の`legacy_ref`名前空間
//! (`"lean-manifest:..."`)を使うので、同じ(subject,object)組に対して
//! 両方が共存しうる——`docs/P6_STATUS.md`の比較レポートが、
//! 2つの手法がどこで一致しどこで食い違うかを実データで測れるようにする。

use crate::model::{EpistemicState, EvidenceKind, NewEvidence, NewRelationAssertion, NewSourceRecord, RelationKind, ReleaseId};
use crate::relation_policy::SOURCE_MAPPING_POLICY_VERSION;
use crate::store::ProvenanceStore;
use mathesis_graph::GraphStore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

pub const ADAPTER_NAME: &str = "mathesis-provenance-lean-manifest-adapter";
pub const ADAPTER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// P6.1（`docs/LEAN_DEPENDENCY_POLICY.md`）: `crates/mathesis-lean-extract/
/// ExtractManifest.lean`の`filteringPolicyVersion`定数と手で同期させる
/// 文字列（Lean側からRust側の定数を直接参照する手段が無いための制約、
/// 同ドキュメント「Why the filter logic exists in two places」参照）。
/// `verify.rs`のリリースゲートが、取り込み時にmanifestへ埋め込んだ値と
/// この値の一致を検査する——フィルタの定義が変わったのに古いポリシーで
/// 取り込んだ証拠のまま`default_traversal`へ上げてしまう事態を防ぐ。
pub const FILTERING_POLICY_VERSION: &str = "mathesis-lean-dependency-filter-v1";

/// `crates/mathesis-lean-extract`が書き出すJSONの形。P6.1
/// （`docs/LEAN_DEPENDENCY_POLICY.md`）でスキーマを刷新——`dependsOn`の
/// 生の文字列配列は無くなり、raw/published分離・origin付きの形になった。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeanManifest {
    pub project: String,
    pub lean_toolchain: String,
    pub mathlib_rev: String,
    pub entry_module: String,
    pub extractor_version: String,
    pub filtering_policy_version: String,
    /// Leanは壁時計時刻を素直に取る標準APIを使っていない——生成時刻は
    /// マニフェスト自身には無い。`import_lean_manifest`が取り込み時刻を
    /// 代わりに`SourceRecord.retrieved_at_unix`へ記録する(偽の生成時刻を
    /// 捏造しない)。
    pub declarations: Vec<LeanDeclaration>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeanDeclaration {
    /// バレ名（namespace剥がした最後の構成要素）——`mathesis-importer`の
    /// テキスト抽出が使うjudgment名と揃える。
    pub name: String,
    /// 完全修飾名。マッチングには使わない（バレ名の食い違いを見せる
    /// evidence locator用）。
    pub qualified_name: String,
    pub module: String,
    /// P6.1: 型検査済みの型・値が実際に参照した全定数(完全修飾名、フィルタ前)
    /// ——自己参照・Mathlib・生成物・privateも含む、監査用の生信号。
    /// `import_lean_manifest`はこれを判断のためにも使わない
    /// (`published_dependencies`だけを取り込む)——`raw_manifest`自体を
    /// `raw_payload_uri`で指せるようにしてあるので、DBへ複製しない。
    #[allow(dead_code)]
    pub raw_constants: Vec<String>,
    /// P6.1: `docs/LEAN_DEPENDENCY_POLICY.md`のフィルタ済み、同じ
    /// プロジェクト名前空間内・自己参照でない・生成/private詳細でない
    /// 依存だけ。各要素に`origin`("type"|"body"|"both")付き。
    pub published_dependencies: Vec<PublishedDependency>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishedDependency {
    pub name: String,
    pub origin: String,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ImportStats {
    pub dependencies_imported: usize,
    pub dependencies_skipped_existing: usize,
    /// マニフェスト上の宣言名が、この論文の既存judgmentへ引けなかった件数
    /// （曖昧な同名が複数あった場合も含む——無い名前を捏造しない）。
    pub declarations_unmatched: usize,
    /// 依存先の宣言名が引けなかった件数（同上）。
    pub dependency_targets_unmatched: usize,
}

fn legacy_ref(subject_id: i64, object_id: i64) -> String {
    format!("lean-manifest:{subject_id}:{object_id}")
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{hex}")
}

fn manifest_hash(raw: &str) -> String {
    sha256_hex(raw.as_bytes())
}

/// P6.1「reproducibility metadata」（`docs/LEAN_DEPENDENCY_POLICY.md`）:
/// `manifest_hash`(受け取った生バイト列そのもののハッシュ)とは別に、
/// *パース後*のmanifestから宣言・依存・originだけを正規形に組み立てて
/// ハッシュする——JSONの体裁(フィールド順・空白)が変わっても、実際の
/// 依存関係グラフが変わらない限り同じ値になる。宣言・依存とも常に
/// 完全修飾名/バレ名で整列済み(`ExtractManifest.lean`のbyte-stable出力
/// 要件)なので、この文字列組み立て自体もmanifest内の順序に依存しない。
fn normalized_manifest_hash(manifest: &LeanManifest) -> String {
    let mut canonical = String::new();
    let mut decls: Vec<&LeanDeclaration> = manifest.declarations.iter().collect();
    decls.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
    for decl in decls {
        let mut deps: Vec<&PublishedDependency> = decl.published_dependencies.iter().collect();
        deps.sort_by(|a, b| a.name.cmp(&b.name));
        canonical.push_str(&decl.qualified_name);
        canonical.push('|');
        for dep in deps {
            canonical.push_str(&dep.name);
            canonical.push(':');
            canonical.push_str(&dep.origin);
            canonical.push(',');
        }
        canonical.push('\n');
    }
    sha256_hex(canonical.as_bytes())
}

/// 指定論文(arXiv id)が持つjudgmentだけを対象にバレ名→idを引く索引を作る。
/// `mathesis-importer::dependencies::record_dependencies`と同じ
/// 「1バッチ内でユニークな名前」という前提を踏襲——論文をまたいだ偶然の
/// 名前衝突を避けるために、対象をこの論文の判断だけに絞る。同じバレ名が
/// この論文内に複数あれば曖昧として除外する(黙って片方を選ばない)。
fn judgment_name_index(graph: &GraphStore, arxiv_id: &str) -> anyhow::Result<HashMap<String, i64>> {
    let paper = graph
        .find_paper_by_arxiv_id(arxiv_id)?
        .ok_or_else(|| anyhow::anyhow!("paper {arxiv_id} not found in mathesis-graph"))?;
    let judgment_ids = graph.judgments_of_paper(paper.id)?;
    let mut by_name: HashMap<String, Vec<i64>> = HashMap::new();
    for id in judgment_ids {
        let j = graph.get_judgment(id)?;
        if let Some(name) = j.name {
            by_name.entry(name).or_default().push(id.0);
        }
    }
    Ok(by_name.into_iter().filter_map(|(name, ids)| (ids.len() == 1).then(|| (name, ids[0]))).collect())
}

/// マニフェスト(生JSON文字列、ハッシュ計算のため生のまま受け取る)を
/// `depends_on`アサーション群として取り込む。呼び出し側が
/// `import-legacy`済みのreleaseを渡す——このアダプタはreleaseを新設しない。
///
/// `manifest_path`は`SourceRecord.raw_payload_uri`へそのまま記録する
/// (監査者が生マニフェスト——`rawConstants`を含む——を実際に開けるように)。
/// `project_commit`はLean側が知りようがない情報なので(`docs/
/// LEAN_DEPENDENCY_POLICY.md`「projectCommit is the only nullable...」)、
/// `import-legacy --git-commit`と同じ流儀でCLI呼び出し元から渡す——無ければ
/// 捏造せず`None`のまま。
pub fn import_lean_manifest(
    prov: &ProvenanceStore,
    graph: &GraphStore,
    release: ReleaseId,
    arxiv_id: &str,
    raw_manifest: &str,
    retrieved_at_unix: i64,
    manifest_path: Option<&str>,
    project_commit: Option<&str>,
) -> anyhow::Result<ImportStats> {
    let manifest: LeanManifest = serde_json::from_str(raw_manifest)?;
    let name_index = judgment_name_index(graph, arxiv_id)?;
    let raw_hash = manifest_hash(raw_manifest);
    let normalized_hash = normalized_manifest_hash(&manifest);

    let reproducibility_json = serde_json::json!({
        "leanToolchain": manifest.lean_toolchain,
        "mathlibRev": manifest.mathlib_rev,
        "projectCommit": project_commit,
        "extractorVersion": manifest.extractor_version,
        "filteringPolicyVersion": manifest.filtering_policy_version,
        "rawManifestHash": raw_hash,
        "normalizedManifestHash": normalized_hash,
    })
    .to_string();

    let source_id = prov.get_or_insert_source_record(&NewSourceRecord {
        provider: "lean-elaborator".into(),
        provider_id: manifest.entry_module.clone(),
        // `raw_hash`を使う(トルーチェイン文字列固定にしない)——`source_record.rs`
        // の設計判断(外部レビュー2026-09-05)そのままの理由: 同じ
        // (provider, provider_id, provider_revision)キーは同じSourceRecordに
        // 集約されるので、内容が変わったのにrevisionが変わらなければ
        // 再取り込み時に古い`reproducibility_json`/`content_hash`が黙って
        // 使い回されてしまう。マニフェストの中身が変わるたびに別行になる
        // ことを保証するのは、この生ハッシュだけ。
        provider_revision: Some(raw_hash.clone()),
        retrieved_at_unix: Some(retrieved_at_unix),
        content_hash: Some(raw_hash.clone()),
        // 第三者データの再配布ではなく、このプロジェクト自身が実行して
        // 得たLean elaboratorの出力——ライセンス欄は「再配布ライセンス」の
        // 意味では該当なし(licensing.rsのゲートは外部ソース向け)。
        licence: None,
        attribution: Some(format!(
            "{} (lean-toolchain {}, mathlib {})",
            manifest.project, manifest.lean_toolchain, manifest.mathlib_rev
        )),
        raw_payload_uri: manifest_path.map(str::to_string),
        adapter_name: ADAPTER_NAME.into(),
        adapter_version: ADAPTER_VERSION.into(),
        parser_version: Some(manifest.extractor_version.clone()),
        reproducibility_json: Some(reproducibility_json),
    })?;

    let mut stats = ImportStats::default();
    for decl in &manifest.declarations {
        let Some(&subject_id) = name_index.get(&decl.name) else {
            stats.declarations_unmatched += 1;
            continue;
        };
        for dep in &decl.published_dependencies {
            let Some(&object_id) = name_index.get(&dep.name) else {
                stats.dependency_targets_unmatched += 1;
                continue;
            };
            let lref = legacy_ref(subject_id, object_id);
            if prov.get_assertion_by_legacy_ref(release, &lref)?.is_some() {
                stats.dependencies_skipped_existing += 1;
                continue;
            }
            let assertion_id = prov.insert_assertion(&NewRelationAssertion {
                subject_ref: format!("judgment:{subject_id}"),
                predicate: RelationKind::DependsOn,
                object_ref: format!("judgment:{object_id}"),
                epistemic_state: EpistemicState::Observed,
                score: None,
                policy_version: Some(SOURCE_MAPPING_POLICY_VERSION.into()),
                created_by_run_id: Some(ADAPTER_NAME.into()),
                supersedes_id: None,
                release_id: release,
                legacy_ref: Some(lref),
            })?;
            prov.insert_evidence(&NewEvidence {
                assertion_id,
                source_record_id: source_id,
                locator: Some(format!("{} -> {}", decl.qualified_name, dep.name)),
                evidence_kind: EvidenceKind::FormalExport,
                extractor_or_model: Some(ADAPTER_NAME.into()),
                version: Some(manifest.lean_toolchain.clone()),
                input_hash: Some(raw_hash.clone()),
                output_hash: None,
                metric_name: None,
                metric_value: None,
                dependency_origin: Some(dep.origin.clone()),
            })?;
            stats.dependencies_imported += 1;
        }
    }
    Ok(stats)
}

/// Priority 2, step 3（ユーザー指示 2026-09-08）: 「false positive・見落とし・
/// 食い違いを測る」ための素朴な集計。`depends_on`述語の
/// (subject_ref, object_ref)組を認識状態ごとに集めて突き合わせるだけ——
/// どちらの技法が「正しい」かをここで判定しない(それは人間の仕事)。
#[derive(Debug, Default)]
pub struct ComparisonReport {
    pub text_extracted_total: usize,
    pub checker_derived_total: usize,
    /// 両方の技法が独立に見つけた組——最も信頼できる部分集合。
    pub agree: usize,
    /// テキスト抽出だけが見つけた組——識別子が本文中に現れたが、
    /// 実際の型検査済み証明項は参照していない可能性がある
    /// (`docs/DATA_DICTIONARY.md`が最初から警告していた性質そのもの)。
    pub text_only: usize,
    /// Lean manifestだけが見つけた組——証明項は参照しているのに、
    /// 名前一致では検出できなかった(例: 別名・`open`経由・暗黙引数)。
    pub checker_only: usize,
    pub text_only_examples: Vec<(String, String)>,
    pub checker_only_examples: Vec<(String, String)>,
}

impl ComparisonReport {
    pub fn print(&self) {
        println!(
            "dependency source comparison: text-extracted {}, checker-derived {} — agree {}, text-only {}, checker-only {}",
            self.text_extracted_total, self.checker_derived_total, self.agree, self.text_only, self.checker_only
        );
        if !self.text_only_examples.is_empty() {
            println!("  text-only examples (up to 10, possible false positives in name-matching):");
            for (s, o) in &self.text_only_examples {
                println!("    {s} -> {o}");
            }
        }
        if !self.checker_only_examples.is_empty() {
            println!("  checker-only examples (up to 10, missed by name-matching):");
            for (s, o) in &self.checker_only_examples {
                println!("    {s} -> {o}");
            }
        }
    }
}

/// `release`全体ではなく、**このマニフェストが実際に解析した宣言だけ**に
/// 比較を絞る——スコープを絞らないと、Lean側が一度も見ていない
/// (=別ファイル/別論文の)`text-only`ペアが大量に混ざり、あたかも
/// テキスト抽出が的外れに多いかのような誤った印象を与える。「Leanが
/// この宣言を見たのに、この依存を報告しなかった」ときだけを本当の
/// text-only(食い違い)として数える。
pub fn compare_dependency_sources(
    prov: &ProvenanceStore,
    graph: &GraphStore,
    release: ReleaseId,
    arxiv_id: &str,
    raw_manifest: &str,
) -> anyhow::Result<ComparisonReport> {
    use std::collections::HashSet;

    let manifest: LeanManifest = serde_json::from_str(raw_manifest)?;
    let name_index = judgment_name_index(graph, arxiv_id)?;
    let scope: HashSet<String> = manifest
        .declarations
        .iter()
        .filter_map(|d| name_index.get(&d.name).map(|id| format!("judgment:{id}")))
        .collect();

    let assertions = prov.list_assertions_for_release(release)?;
    let mut text_pairs: HashSet<(String, String)> = HashSet::new();
    let mut checker_pairs: HashSet<(String, String)> = HashSet::new();
    for a in &assertions {
        if a.predicate != RelationKind::DependsOn || !scope.contains(&a.subject_ref) {
            continue;
        }
        let pair = (a.subject_ref.clone(), a.object_ref.clone());
        match a.epistemic_state {
            EpistemicState::Extracted => {
                text_pairs.insert(pair);
            }
            EpistemicState::Observed => {
                checker_pairs.insert(pair);
            }
            _ => {}
        }
    }

    let mut report = ComparisonReport {
        text_extracted_total: text_pairs.len(),
        checker_derived_total: checker_pairs.len(),
        ..Default::default()
    };
    for pair in text_pairs.difference(&checker_pairs) {
        report.text_only += 1;
        if report.text_only_examples.len() < 10 {
            report.text_only_examples.push(pair.clone());
        }
    }
    for pair in checker_pairs.difference(&text_pairs) {
        report.checker_only += 1;
        if report.checker_only_examples.len() < 10 {
            report.checker_only_examples.push(pair.clone());
        }
    }
    report.agree = text_pairs.intersection(&checker_pairs).count();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EntityKind, NewEntity, NewRelease};
    use crate::store::ProvenanceStore;
    use mathesis_ast::parse_expr;
    use mathesis_graph::model::{JudgmentKind, NewJudgment, ParseStatus, SourceRef};

    fn seed_graph_judgment(graph: &GraphStore, name: &str, paper: mathesis_graph::PaperId) -> i64 {
        let expr = parse_expr("True").unwrap().expr;
        let statement = graph.intern_expr(&expr).unwrap();
        graph
            .insert_judgment(&NewJudgment {
                kind: JudgmentKind::Lemma,
                name: Some(name.into()),
                context: vec![],
                statement,
                definition_body_raw: None,
                source: SourceRef { file: "Test.lean".into(), line: 1 },
                raw_text: format!("lemma {name} : True := trivial"),
                parse_status: ParseStatus::Full,
                source_paper: Some(paper),
            })
            .unwrap()
            .0
    }

    /// P6.1でスキーマが変わった(`dependsOn: [String]` →
    /// `publishedDependencies: [{name, origin}]` + `rawConstants`)。
    /// テストは依存を`(name, origin)`のペアで渡す——originを明示的に
    /// 書かせることで、`import_lean_manifest`が実際にそれを
    /// `Evidence.dependency_origin`まで運んでいることをテストできる。
    fn manifest_json(entries: &[(&str, &str, &[(&str, &str)])]) -> String {
        let decls: Vec<_> = entries
            .iter()
            .map(|(name, qualified, deps)| {
                let published: Vec<_> = deps.iter().map(|(dep_name, origin)| serde_json::json!({"name": dep_name, "origin": origin})).collect();
                let raw: Vec<_> = deps.iter().map(|(dep_name, _)| *dep_name).collect();
                serde_json::json!({
                    "name": name,
                    "qualifiedName": qualified,
                    "module": "Test.Module",
                    "rawConstants": raw,
                    "publishedDependencies": published,
                })
            })
            .collect();
        serde_json::json!({
            "project": "TestProject",
            "leanToolchain": "leanprover/lean4:v4.29.0-rc6",
            "mathlibRev": "abc123",
            "entryModule": "Test.Module",
            "extractorVersion": "mathesis-lean-extract-v2",
            "filteringPolicyVersion": FILTERING_POLICY_VERSION,
            "declarations": decls,
        })
        .to_string()
    }

    #[test]
    fn imports_a_dependency_edge_as_observed_with_formal_export_evidence() {
        let graph = GraphStore::open_in_memory().unwrap();
        let paper = graph.intern_paper("test/0001", None).unwrap();
        let a = seed_graph_judgment(&graph, "thm_a", paper);
        let b = seed_graph_judgment(&graph, "thm_b", paper);

        let prov = ProvenanceStore::open_in_memory().unwrap();
        let release = prov.get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None }).unwrap();
        prov.get_or_insert_entity(&NewEntity { kind: EntityKind::Judgment, display_label: "thm_a".into(), source_record_id: None }, &format!("judgment:{a}")).unwrap();
        prov.get_or_insert_entity(&NewEntity { kind: EntityKind::Judgment, display_label: "thm_b".into(), source_record_id: None }, &format!("judgment:{b}")).unwrap();

        let raw = manifest_json(&[("thm_a", "Test.Module.thm_a", &[("thm_b", "body")]), ("thm_b", "Test.Module.thm_b", &[])]);
        let stats =
            prov.transaction(|| import_lean_manifest(&prov, &graph, release, "test/0001", &raw, 0, Some("scratch/manifest.json"), Some("deadbeef"))).unwrap();
        assert_eq!(stats.dependencies_imported, 1);
        assert_eq!(stats.declarations_unmatched, 0);
        assert_eq!(stats.dependency_targets_unmatched, 0);

        let assertion_id = prov.get_assertion_by_legacy_ref(release, &legacy_ref(a, b)).unwrap().unwrap();
        let assertion = prov.get_assertion(assertion_id).unwrap();
        assert_eq!(assertion.epistemic_state, EpistemicState::Observed, "本物のLean elaborator manifest由来はobservedであるべき");
        assert_eq!(assertion.predicate, RelationKind::DependsOn);
        assert_eq!(assertion.subject_entity_id.is_some() && assertion.object_entity_id.is_some(), true, "既存カタログに両端があればFKも即座に埋まるはず");

        let evidence = prov.evidence_for(assertion_id).unwrap();
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].evidence_kind, EvidenceKind::FormalExport, "テキスト抽出のsource_spanとは区別する");
        assert_eq!(evidence[0].dependency_origin.as_deref(), Some("body"), "P6.1: origin(type/body/both)がEvidenceまで運ばれる");

        let source = prov.get_source_record(evidence[0].source_record_id).unwrap();
        assert_eq!(source.raw_payload_uri.as_deref(), Some("scratch/manifest.json"), "監査用に生マニフェストの場所を記録する");
        let repro: serde_json::Value = serde_json::from_str(source.reproducibility_json.as_deref().unwrap()).unwrap();
        assert_eq!(repro["projectCommit"], "deadbeef");
        assert_eq!(repro["filteringPolicyVersion"], FILTERING_POLICY_VERSION);
        assert!(repro["rawManifestHash"].as_str().unwrap().starts_with("sha256:"));
        assert!(repro["normalizedManifestHash"].as_str().unwrap().starts_with("sha256:"));
    }

    #[test]
    fn rerunning_the_same_manifest_is_idempotent() {
        let graph = GraphStore::open_in_memory().unwrap();
        let paper = graph.intern_paper("test/0002", None).unwrap();
        let a = seed_graph_judgment(&graph, "thm_a", paper);
        let b = seed_graph_judgment(&graph, "thm_b", paper);
        let _ = (a, b);

        let prov = ProvenanceStore::open_in_memory().unwrap();
        let release = prov.get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None }).unwrap();
        prov.get_or_insert_entity(&NewEntity { kind: EntityKind::Judgment, display_label: "thm_a".into(), source_record_id: None }, &format!("judgment:{a}")).unwrap();
        prov.get_or_insert_entity(&NewEntity { kind: EntityKind::Judgment, display_label: "thm_b".into(), source_record_id: None }, &format!("judgment:{b}")).unwrap();

        let raw = manifest_json(&[("thm_a", "Test.Module.thm_a", &[("thm_b", "body")])]);
        let first = prov.transaction(|| import_lean_manifest(&prov, &graph, release, "test/0002", &raw, 0, None, None)).unwrap();
        let second = prov.transaction(|| import_lean_manifest(&prov, &graph, release, "test/0002", &raw, 0, None, None)).unwrap();
        assert_eq!(first.dependencies_imported, 1);
        assert_eq!(second.dependencies_imported, 0);
        assert_eq!(second.dependencies_skipped_existing, 1);
        assert_eq!(prov.assertion_count().unwrap(), 1);
    }

    #[test]
    fn a_dependency_naming_an_unknown_declaration_is_counted_not_fabricated() {
        let graph = GraphStore::open_in_memory().unwrap();
        let paper = graph.intern_paper("test/0003", None).unwrap();
        let a = seed_graph_judgment(&graph, "thm_a", paper);
        let _ = a;

        let prov = ProvenanceStore::open_in_memory().unwrap();
        let release = prov.get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None }).unwrap();
        prov.get_or_insert_entity(&NewEntity { kind: EntityKind::Judgment, display_label: "thm_a".into(), source_record_id: None }, &format!("judgment:{a}")).unwrap();

        // "thm_b"はmathesis-graph側に存在しない(例えばsorryや自動生成された
        // 補助補題で、テキスト抽出側のjudgmentノードには無い)想定。
        let raw = manifest_json(&[("thm_a", "Test.Module.thm_a", &[("thm_b", "body")])]);
        let stats = prov.transaction(|| import_lean_manifest(&prov, &graph, release, "test/0003", &raw, 0, None, None)).unwrap();
        assert_eq!(stats.dependencies_imported, 0);
        assert_eq!(stats.dependency_targets_unmatched, 1);
        assert_eq!(prov.assertion_count().unwrap(), 0, "存在しない依存先をでっち上げない");
    }

    #[test]
    fn an_ambiguous_bare_name_within_the_same_paper_is_skipped_not_guessed() {
        let graph = GraphStore::open_in_memory().unwrap();
        let paper = graph.intern_paper("test/0004", None).unwrap();
        // 同じ論文内に同名のjudgmentが2件——バレ名だけでは一意に決まらない。
        let _a1 = seed_graph_judgment(&graph, "dup_name", paper);
        let _a2 = seed_graph_judgment(&graph, "dup_name", paper);

        let prov = ProvenanceStore::open_in_memory().unwrap();
        let release = prov.get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None }).unwrap();

        let raw = manifest_json(&[("dup_name", "Test.Module.dup_name", &[])]);
        let stats = prov.transaction(|| import_lean_manifest(&prov, &graph, release, "test/0004", &raw, 0, None, None)).unwrap();
        assert_eq!(stats.declarations_unmatched, 1, "曖昧な同名は解決せず未マッチとして数える");
        assert_eq!(prov.assertion_count().unwrap(), 0);
    }

    /// Priority 2, step 3: マニフェストが実際に見ていない宣言についての
    /// テキスト抽出済み依存は、text-onlyの「食い違い」として数えては
    /// いけない——Leanがそもそも解析していない(=判定不能な)宣言だから。
    /// スコープを絞らないと、コーパス全体のtext-extracted depends_onが
    /// ほぼ全部text-onlyとして混入し、比較結果が実質的に無意味になる。
    #[test]
    fn comparison_only_counts_pairs_the_manifest_actually_analyzed() {
        use crate::model::{EvidenceKind, NewEvidence, NewSourceRecord};

        let graph = GraphStore::open_in_memory().unwrap();
        let paper = graph.intern_paper("test/0005", None).unwrap();
        let a = seed_graph_judgment(&graph, "thm_a", paper); // マニフェストが見る
        let b = seed_graph_judgment(&graph, "thm_b", paper); // マニフェストが見る
        let outside = seed_graph_judgment(&graph, "unrelated", paper); // マニフェストの外

        let prov = ProvenanceStore::open_in_memory().unwrap();
        let release = prov.get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None }).unwrap();
        let source = prov
            .get_or_insert_source_record(&NewSourceRecord {
                provider: "test".into(), provider_id: "t".into(), provider_revision: None, retrieved_at_unix: None,
                content_hash: None, licence: None, attribution: None, raw_payload_uri: None,
                adapter_name: "test".into(), adapter_version: "0".into(), parser_version: None,
                reproducibility_json: None,
            })
            .unwrap();
        let insert_extracted = |subject: i64, object: i64, lref: &str| {
            let assertion_id = prov
                .insert_assertion(&NewRelationAssertion {
                    subject_ref: format!("judgment:{subject}"),
                    predicate: RelationKind::DependsOn,
                    object_ref: format!("judgment:{object}"),
                    epistemic_state: EpistemicState::Extracted,
                    score: None, policy_version: None, created_by_run_id: None, supersedes_id: None,
                    release_id: release, legacy_ref: Some(lref.into()),
                })
                .unwrap();
            prov.insert_evidence(&NewEvidence {
                assertion_id, source_record_id: source, locator: None, evidence_kind: EvidenceKind::SourceSpan,
                extractor_or_model: None, version: None, input_hash: None, output_hash: None, metric_name: None, metric_value: None,
                dependency_origin: None,
            })
            .unwrap();
        };
        // マニフェストの範囲内: text抽出とLean双方が同意する組。
        insert_extracted(a, b, "judgment_dependency:a:b");
        // マニフェストの範囲**外**: `outside`はこのマニフェストが一度も
        // 見ていない宣言——比較から除外されるべき。
        insert_extracted(outside, a, "judgment_dependency:outside:a");

        let raw = manifest_json(&[("thm_a", "Test.Module.thm_a", &[("thm_b", "body")]), ("thm_b", "Test.Module.thm_b", &[])]);
        prov.transaction(|| import_lean_manifest(&prov, &graph, release, "test/0005", &raw, 0, None, None)).unwrap();

        let report = compare_dependency_sources(&prov, &graph, release, "test/0005", &raw).unwrap();
        assert_eq!(report.agree, 1, "thm_a -> thm_bは両方が同意");
        assert_eq!(report.text_only, 0, "outside -> thm_aはマニフェスト範囲外なので数えない");
        assert_eq!(report.checker_only, 0);
    }
}

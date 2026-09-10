//! P7（`docs/P7_STATUS.md`）: Math-Graph（`uw-math-ai/math-graph`、CC BY 4.0、
//! TheoremGraph論文 arXiv:2606.25363のデータセット本体）の`LeanGraph`部分
//! ——25個のLean 4プロジェクトに対するelaborator-level宣言依存抽出——を、
//! スコープを絞ったオフラインパイロットとして取り込む。
//!
//! **意図的に取り込まないもの**: `paper_arxiv.csv`/`statement_informal.csv`/
//! `informal_dependency.csv`/`slogan.csv`(非形式・テキスト側、計12.4GB)、
//! および形式側でも`body`/`proof`/`docstring`(宣言そのものの本文テキスト)。
//! ユーザー指示の「テキストより先にグラフ構造を」——このパイロットは
//! `statement_id`/`decl_name`/`module`/`kind`/`file_path`(ロケータ、コード
//! 本体ではない)と`formal_dependency.csv`の型付き依存辺だけを扱う。
//! `paper_lean_repo.csv`にプロジェクト単位のライセンス列が無いため、本文を
//! 取り込まない設計はこの欠落を実質的に無害化する——無いライセンス情報を
//! 捏造して本文を配布することはしない。
//!
//! スコープの絞り込み(`scratch/math_graph_pilot/scope_pilot.py`)は、
//! P6.2のパイロット2/3と**同じ2つのMathlib名前空間**
//! (`Mathlib.Algebra.Order.Group`/`Mathlib.CategoryTheory.Category`)、
//! 最も近いLeanツールチェイン版(`Mathlib_v429`)に絞る——「何と比べれば
//! 意味があるか」を先に決めてから取り込む、P6.2と同じ規律。フィルタ済みの
//! `pilot_statements.json`/`pilot_edges.json`(小さい)だけをこのモジュールが
//! 読む——多GBの生CSVは`mathesis-provenance`自身が開くことはない
//! （`openalex_fetch.rs`/`openalex_adapter.rs`の分離と同じ設計）。
//!
//! **`epistemic_state: extracted`、`observed`ではない意図的な選択**:
//! `docs/DATA_DICTIONARY.md`§4.2の文字どおりの定義では、
//! elaborator検証済みの依存は`observed`にあたる——Math-GraphのLeanGraphは
//! 方法論として本当にelaborator-levelの抽出である。だがそれは「Math-Graph
//! 自身が自分のパイプラインを検証した」という意味でしかなく、Mathesis
//! 自身が独立に再現・監査したわけではない(P6.1の`mathesis-lean-extract`は
//! 自分でLean/mathlib版を固定し、フィルタポリシーをテスト付きで文書化し、
//! P6.2でbyte-stabilityまで確認した——Math-Graphのパイプラインに対しては
//! そのどれもしていない)。ユーザー指示の「外部の辺が、データセットが
//! dependencyと呼んでいるというだけの理由でdefault_traversalにならない
//! ようにする」を、`relation_policy::traversal_policy`という既存の共有
//! コードに手を入れずに満たす最小の方法が、この1点の選択——
//! `extracted`は`DependsOn`に対して`visible_only`にしかならない
//! （`relation_policy.rs`参照）。
//!
//! **`evidence_kind: formal_export`は維持**——`extracted`にしたのは信頼の
//! ポリシー判断であって、証拠の性質の誤記ではない。実際に
//! elaborator由来の型付き依存であることに変わりはなく、テキスト一致
//! (`source_span`)やLLM出力(`model_output`)と呼ぶ方がむしろ不正確になる。
//! `docs/DATA_DICTIONARY.md`「Evidence multiplicity, not epistemic-state
//! inflation」——evidence_kindは「どんな検査が行われたか」、
//! epistemic_stateは「それをどれだけ信頼するか」の別の軸。
//!
//! **`subject_ref`/`object_ref`は`"judgment:mathgraph:<statement_id>"`
//! 名前空間**——Mathesis自身の`"judgment:<数値id>"`とは衝突しない別の
//! タグ付き文字列。`EntityKind::Judgment`として登録するので
//! `relation_policy::valid_entity_kinds`は素通しするが、
//! `entity.rs::judgment_id_for_entity`は`"judgment:"`の**残り全体を
//! i64としてパース**するため(`mathesis-graph`側の数値主キーへの逆引き
//! 専用)、`"mathgraph:<uuid>"`はパースに失敗し`None`を返す——結果、
//! `web_export::build_dependency_edges`はこのpilotが作るassertionを
//! 静かにスキップし続ける(既存の「FKが無ければ出さない」ガードそのもの、
//! 新しいコードを足さなくても安全側に倒れる)。**これは意図した、
//! このパイロットの明示的なスコープ境界**であって未発見のバグではない
//! ——`dependencies.json`/lineage viewへ本当に載せるには、Mathesis自身の
//! `judgments`テーブルに対応する行を作る(=Math-Graphの宣言を独自の
//! judgmentとしてカタログ化する)か、`EntityKind`に第4の種別を足すかの
//! どちらかが要る。どちらも「オフラインでデータセットを取り込めるか」
//! というこのpilotの問いには関係ないので、今回はやらない
//! (`docs/P7_STATUS.md`「What this pilot deliberately does not do」)。

use crate::model::{
    EntityKind, EpistemicState, EvidenceKind, NewEntity, NewEvidence, NewRelationAssertion, NewSourceRecord,
    RelationKind, ReleaseId,
};
use crate::relation_policy::SOURCE_MAPPING_POLICY_VERSION;
use crate::store::ProvenanceStore;
use serde::Deserialize;
use std::collections::HashMap;

pub const ADAPTER_NAME: &str = "mathesis-provenance-math-graph-adapter";
pub const ADAPTER_VERSION: &str = env!("CARGO_PKG_VERSION");
/// HuggingFaceの`uw-math-ai/math-graph`データセットカードの
/// `cardData.license`欄で確認済み(2026-09-08、HF API `/api/datasets/...`)。
pub const MATH_GRAPH_LICENSE: &str = "CC-BY-4.0";
pub const MATH_GRAPH_ATTRIBUTION: &str =
    "Math-Graph (uw-math-ai), https://huggingface.co/datasets/uw-math-ai/math-graph — dataset for \"TheoremGraph: Bridging Formal and Informal Mathematics\" (arXiv:2606.25363)";
pub const MATH_GRAPH_SOURCE_URL: &str = "https://huggingface.co/datasets/uw-math-ai/math-graph";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalClassification {
    ExternalLiteralDependency,
    ExternalTypeclassHierarchy,
}

impl ExternalClassification {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExternalLiteralDependency => "external_literal_dependency",
            Self::ExternalTypeclassHierarchy => "external_typeclass_hierarchy",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PilotStatement {
    pub statement_id: String,
    pub decl_name: String,
    pub module: String,
    pub kind: String,
    pub file_path: String,
    pub is_instance: bool,
    pub repo_slug: String,
    pub lean_toolchain: Option<String>,
    pub mathlib_rev: Option<String>,
    pub git_commit: Option<String>,
    /// P7.4（`docs/P7_4_STATUS.md`）: P7.3のスキーマ+内容証拠による分類——
    /// `scope_pilot_p7_4.py`が事前に計算して埋める(このRustコードでは
    /// 推測しない)。この安全部分集合には、この2値のどちらかに分類できた
    /// 61件だけを含む——未解決だった2件はPythonの前処理段階で除外済み。
    pub classification: ExternalClassification,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PilotEdge {
    pub src_id: String,
    pub dep_id: String,
    pub edge_type: String,
    pub role: Option<String>,
    pub via_proj: bool,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ImportStats {
    pub declarations_imported: usize,
    pub declarations_skipped_existing: usize,
    pub dependencies_imported: usize,
    pub dependencies_skipped_existing: usize,
    /// `dep_id`がこのpilotのスコープ外(=`statements`に無い)だった辺の数。
    /// `openalex_adapter::ImportStats::references_outside_catalog`と同じ
    /// 「雪だるま式収集の境界」——0でも異常ではない。
    pub dependencies_outside_pilot_scope: usize,
    /// P7.4（`docs/P7_4_STATUS.md`）: `edge_type: proof`を持つ辺は、その
    /// 「意味」(literal宣言どうしの証明項参照なのか、階層合成ノードの
    /// 一部として何を表すのか)を別途定義するまで取り込まない、という
    /// ユーザー指示による意図的な除外——`dependencies_outside_pilot_scope`
    /// (スコープ外)とは別の理由による除外なので、別カウンタで数える。
    pub dependencies_excluded_proof_edge: usize,
}

fn mathgraph_ref(statement_id: &str) -> String {
    format!("judgment:mathgraph:{statement_id}")
}

fn legacy_ref(edge: &PilotEdge) -> String {
    format!("math-graph:{}:{}:{}", edge.src_id, edge.dep_id, edge.edge_type)
}

/// 宣言1件ぶんのsource_recordを作り、`EntityKind::Judgment`として登録する。
/// `reproducibility_json`はP6.1のキー(`leanToolchain`/`mathlibRev`/...)を
/// 意図的に使わない——別の語彙にすることで、`verify_formal_evidence_has_
/// reproducibility_metadata`(P6.1、Mathesis自身の`filteringPolicyVersion`
/// と一致するかを見る)がこのpilotのevidenceを誤って対象にしないことを
/// 型ではなくJSON形状のレベルでも明示する（もっとも実際には
/// `epistemic_state: extracted`のためdefault_traversalに届かず、この
/// チェック自体がそもそも走らない——二重の安全）。
fn import_statement(
    prov: &ProvenanceStore,
    stmt: &PilotStatement,
    dataset_revision: &str,
) -> anyhow::Result<crate::model::SourceRecordId> {
    let reproducibility_json = serde_json::json!({
        "source": "math-graph",
        "datasetRevision": dataset_revision,
        "repoSlug": stmt.repo_slug,
        "leanToolchain": stmt.lean_toolchain,
        "mathlibRev": stmt.mathlib_rev,
        "gitCommit": stmt.git_commit,
        "declKind": stmt.kind,
        "isInstance": stmt.is_instance,
    })
    .to_string();
    let source_id = prov.get_or_insert_source_record(&NewSourceRecord {
        provider: "math-graph".into(),
        provider_id: stmt.statement_id.clone(),
        provider_revision: Some(dataset_revision.to_string()),
        retrieved_at_unix: None,
        content_hash: None,
        licence: Some(MATH_GRAPH_LICENSE.into()),
        attribution: Some(MATH_GRAPH_ATTRIBUTION.into()),
        raw_payload_uri: Some(MATH_GRAPH_SOURCE_URL.into()),
        adapter_name: ADAPTER_NAME.into(),
        adapter_version: ADAPTER_VERSION.into(),
        parser_version: Some("mathesis-provenance::math_graph_adapter".into()),
        reproducibility_json: Some(reproducibility_json),
    })?;
    // `file_path`はロケータ(どのファイルの何という宣言か)であって、そのファイル
    // の中身(body/proof/docstring)は一切保持しない——`docs/P7_STATUS.md`
    // 「テキストより先にグラフ構造を」。
    prov.get_or_insert_entity(
        &NewEntity {
            kind: EntityKind::Judgment,
            display_label: format!("{} ({})", stmt.decl_name, stmt.module),
            source_record_id: Some(source_id),
        },
        &mathgraph_ref(&stmt.statement_id),
    )?;
    Ok(source_id)
}

/// `statements`/`edges`は`scope_pilot.py`が書き出した、スコープを絞った
/// JSON(P6.2のMathlib部分木2件と同じ名前空間、`Mathlib_v429`のみ)。
/// `release`は呼び出し側が事前に作成済みのものを渡す(このアダプタは
/// リリースを新設しない、`openalex_adapter::import`と同じ流儀)。
pub fn import_pilot(
    prov: &ProvenanceStore,
    release: ReleaseId,
    statements: &[PilotStatement],
    edges: &[PilotEdge],
    dataset_revision: &str,
) -> anyhow::Result<ImportStats> {
    let mut stats = ImportStats::default();
    let mut in_scope: HashMap<&str, &PilotStatement> = HashMap::new();

    for stmt in statements {
        let already = prov.resolve_entity_ref(&mathgraph_ref(&stmt.statement_id))?.is_some();
        import_statement(prov, stmt, dataset_revision)?;
        if already {
            stats.declarations_skipped_existing += 1;
        } else {
            stats.declarations_imported += 1;
        }
        in_scope.insert(stmt.statement_id.as_str(), stmt);
    }

    for edge in edges {
        let Some(src) = in_scope.get(edge.src_id.as_str()) else {
            // scope_pilot.pyはsrc_idが常にスコープ内であることを保証する
            // ——これが起きたら呼び出し側の前提が崩れている。
            anyhow::bail!("edge src_id {} is not among the given statements — pilot_edges.json/pilot_statements.json out of sync?", edge.src_id);
        };
        let Some(dep) = in_scope.get(edge.dep_id.as_str()) else {
            // openalex_adapter::importの「カタログ外への参照は雪だるま式
            // 収集の境界として静かに無視する」と同じ扱い——このpilotは
            // Mathlibの一角も網羅しないと決めている以上、正常系。
            stats.dependencies_outside_pilot_scope += 1;
            continue;
        };
        if edge.edge_type == "proof" {
            stats.dependencies_excluded_proof_edge += 1;
            continue;
        }

        let ref_ = legacy_ref(edge);
        if prov.get_assertion_by_legacy_ref(release, &ref_)?.is_some() {
            stats.dependencies_skipped_existing += 1;
            continue;
        }

        let assertion_id = prov.insert_assertion(&NewRelationAssertion {
            subject_ref: mathgraph_ref(&src.statement_id),
            predicate: RelationKind::DependsOn,
            object_ref: mathgraph_ref(&dep.statement_id),
            // 上のモジュールdocを参照——elaborator由来ではあるが、独立に
            // 再現・監査していないため`observed`ではなく`extracted`。
            epistemic_state: EpistemicState::Extracted,
            score: None,
            policy_version: Some(SOURCE_MAPPING_POLICY_VERSION.into()),
            created_by_run_id: Some(ADAPTER_NAME.into()),
            supersedes_id: None,
            release_id: release,
            legacy_ref: Some(ref_),
        })?;

        let source_id = prov.get_or_insert_source_record(&NewSourceRecord {
            provider: "math-graph".into(),
            provider_id: src.statement_id.clone(),
            provider_revision: Some(dataset_revision.to_string()),
            retrieved_at_unix: None,
            content_hash: None,
            licence: Some(MATH_GRAPH_LICENSE.into()),
            attribution: Some(MATH_GRAPH_ATTRIBUTION.into()),
            raw_payload_uri: Some(MATH_GRAPH_SOURCE_URL.into()),
            adapter_name: ADAPTER_NAME.into(),
            adapter_version: ADAPTER_VERSION.into(),
            parser_version: Some("mathesis-provenance::math_graph_adapter".into()),
            reproducibility_json: None,
        })?;
        prov.insert_evidence(&NewEvidence {
            assertion_id,
            source_record_id: source_id,
            locator: Some(format!(
                "Math-Graph formal_dependency.csv: {} -> {} (edge_type={}{})",
                src.decl_name,
                dep.decl_name,
                edge.edge_type,
                edge.role.as_deref().map(|r| format!(", role={r}")).unwrap_or_default()
            )),
            evidence_kind: EvidenceKind::FormalExport,
            extractor_or_model: Some("uw-math-ai/math-graph LeanGraph".into()),
            version: Some(dataset_revision.to_string()),
            input_hash: None,
            output_hash: None,
            metric_name: None,
            metric_value: None,
            // 生の`edge_type`をそのまま保持する(sig/def/extends/field/docref
            // ——`proof`は上で既に除外済み)——Mathesis自身のtype/body/both
            // 語彙に無理に押し込めると、Math-Graph自身が実際に区別した
            // 情報を握り潰すことになる(P6.2の教訓「実測しないまま丸めない」)。
            dependency_origin: Some(edge.edge_type.clone()),
            // P7.4: この辺の起点(`src`)がP7.3でどう分類されたか
            // (external_literal_dependency / external_typeclass_hierarchy)。
            external_classification: Some(src.classification.as_str().to_string()),
        })?;
        stats.dependencies_imported += 1;
    }

    Ok(stats)
}

/// P8.4（`docs/P8_4_STATUS.md`、ディレクティブ Stage 5 "Each project should
/// be independently importable and removable"）: `repo_slug`ぶんの宣言・辺を
/// すべて取り消す。`import_pilot`は既に冪等（同じ`(statements, edges)`を
/// 何度渡しても増えない）で「independently importable」の側は元から
/// 満たしていた——ここで足すのは「removable」の側。
///
/// Math-Graphの辺は必ず同一プロジェクト内に閉じる（`scope_pilot_*.py`が
/// `src_id`のスコープでしか辺を残さない設計、`docs/P8_1_STATUS.md`の
/// "dependency targets outside scope"参照）ため、対象entityの集合だけを
/// 集めて`retract::retract_entity`を1件ずつ呼べば、他プロジェクトの宣言・
/// 辺を一切巻き込まずに完結する——複数プロジェクトが混在するDB
/// （P8.1以降の`scratch/p8_1/pilot_provenance.db`）でもこの前提は崩れない。
pub fn remove_project(prov: &ProvenanceStore, repo_slug: &str) -> anyhow::Result<RemoveProjectStats> {
    let candidate_ids = prov.entity_ids_with_ref_prefix("judgment:mathgraph:")?;
    let mut target_ids = Vec::new();
    for id in candidate_ids {
        if crate::discovery_export::repo_slug_for_entity(prov, Some(id))?.as_deref() == Some(repo_slug) {
            target_ids.push(id);
        }
    }

    let mut stats = RemoveProjectStats { declarations_removed: 0, dependencies_removed: 0 };
    for id in target_ids {
        let r = crate::retract::retract_entity(prov, id)?;
        stats.declarations_removed += 1;
        stats.dependencies_removed += r.assertions_removed;
    }
    Ok(stats)
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RemoveProjectStats {
    pub declarations_removed: usize,
    /// 各entityは`target_ids`の中でちょうど1回だけ`retract_entity`に渡される
    /// ため、あるentityの取り消しで既に削除された辺（assertion）は、もう
    /// 片方の端点のentityを取り消すときには`assertion_ids_touching_entity`
    /// にもう出てこない——重複カウントはしない。行き着く総和は実際に
    /// 削除された辺の総数と一致する。
    pub dependencies_removed: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::NewRelease;

    fn store_with_release() -> (ProvenanceStore, ReleaseId) {
        let prov = ProvenanceStore::open_in_memory().unwrap();
        let release = prov
            .get_or_insert_release(&NewRelease { tag: "t".into(), git_commit: None, generated_at_unix: 0, notes: None })
            .unwrap();
        (prov, release)
    }

    fn stmt(id: &str, decl_name: &str, module: &str, classification: ExternalClassification) -> PilotStatement {
        PilotStatement {
            statement_id: id.into(),
            decl_name: decl_name.into(),
            module: module.into(),
            kind: "theorem".into(),
            file_path: format!("{module}.lean"),
            is_instance: false,
            repo_slug: "Mathlib_v429".into(),
            lean_toolchain: Some("v4.2.9".into()),
            mathlib_rev: None,
            git_commit: None,
            classification,
        }
    }

    /// P8.4: like `stmt`, but with a caller-chosen `repo_slug` — `stmt` itself
    /// hardcodes `"Mathlib_v429"`, which the `remove_project` tests below
    /// need to vary (that's the entire property under test).
    fn stmt_in_project(id: &str, decl_name: &str, repo_slug: &str, classification: ExternalClassification) -> PilotStatement {
        PilotStatement { repo_slug: repo_slug.into(), ..stmt(id, decl_name, "Mod", classification) }
    }

    #[test]
    fn imports_declarations_and_a_dependency_edge_as_extracted_not_observed() {
        let (prov, release) = store_with_release();
        let statements = vec![
            stmt("s1", "Foo.bar", "Mathlib.Foo", ExternalClassification::ExternalLiteralDependency),
            stmt("s2", "Foo.baz", "Mathlib.Foo", ExternalClassification::ExternalLiteralDependency),
        ];
        let edges = vec![PilotEdge { src_id: "s1".into(), dep_id: "s2".into(), edge_type: "sig".into(), role: None, via_proj: false }];

        let stats = prov.transaction(|| import_pilot(&prov, release, &statements, &edges, "rev-1")).unwrap();
        assert_eq!(stats.declarations_imported, 2);
        assert_eq!(stats.dependencies_imported, 1);
        assert_eq!(stats.dependencies_outside_pilot_scope, 0);
        assert_eq!(stats.dependencies_excluded_proof_edge, 0);

        let assertions = prov.list_assertions_for_release(release).unwrap();
        assert_eq!(assertions.len(), 1);
        let a = &assertions[0];
        assert_eq!(a.predicate, RelationKind::DependsOn);
        assert_eq!(a.epistemic_state, EpistemicState::Extracted, "must not be Observed — not independently reproduced");
        assert_eq!(
            crate::relation_policy::traversal_policy(a.predicate, a.epistemic_state),
            crate::relation_policy::TraversalPolicy::VisibleOnly,
            "an external dependency claim must not reach default_traversal on its own"
        );

        let evidence = prov.evidence_for(a.id).unwrap();
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].evidence_kind, EvidenceKind::FormalExport);
        assert_eq!(evidence[0].dependency_origin.as_deref(), Some("sig"));
        assert_eq!(evidence[0].external_classification.as_deref(), Some("external_literal_dependency"));
    }

    #[test]
    fn proof_type_edges_are_excluded_pending_a_separate_definition_of_their_meaning() {
        let (prov, release) = store_with_release();
        let statements = vec![
            stmt("s1", "Foo.bar", "Mathlib.Foo", ExternalClassification::ExternalLiteralDependency),
            stmt("s2", "Foo.baz", "Mathlib.Foo", ExternalClassification::ExternalLiteralDependency),
        ];
        let edges = vec![PilotEdge { src_id: "s1".into(), dep_id: "s2".into(), edge_type: "proof".into(), role: None, via_proj: false }];

        let stats = prov.transaction(|| import_pilot(&prov, release, &statements, &edges, "rev-1")).unwrap();
        assert_eq!(stats.dependencies_imported, 0);
        assert_eq!(stats.dependencies_excluded_proof_edge, 1);
        assert_eq!(prov.list_assertions_for_release(release).unwrap().len(), 0, "declarations import even though their proof edge is excluded");
    }

    #[test]
    fn typeclass_hierarchy_classification_round_trips() {
        let (prov, release) = store_with_release();
        let statements = vec![
            stmt("s1", "OrderDual.instMonoid", "Mathlib.Algebra.Order.Group.Synonym", ExternalClassification::ExternalTypeclassHierarchy),
            stmt("s2", "OrderDual.instSemigroup", "Mathlib.Algebra.Order.Group.Synonym", ExternalClassification::ExternalTypeclassHierarchy),
        ];
        let edges = vec![PilotEdge { src_id: "s1".into(), dep_id: "s2".into(), edge_type: "def".into(), role: None, via_proj: false }];

        prov.transaction(|| import_pilot(&prov, release, &statements, &edges, "rev-1")).unwrap();
        let a = &prov.list_assertions_for_release(release).unwrap()[0];
        let evidence = &prov.evidence_for(a.id).unwrap()[0];
        assert_eq!(evidence.external_classification.as_deref(), Some("external_typeclass_hierarchy"));
    }

    #[test]
    fn edges_pointing_outside_the_pilot_scope_are_counted_not_fabricated() {
        let (prov, release) = store_with_release();
        let statements = vec![stmt("s1", "Foo.bar", "Mathlib.Foo", ExternalClassification::ExternalLiteralDependency)];
        let edges = vec![PilotEdge { src_id: "s1".into(), dep_id: "outside-the-scope".into(), edge_type: "sig".into(), role: None, via_proj: false }];

        let stats = prov.transaction(|| import_pilot(&prov, release, &statements, &edges, "rev-1")).unwrap();
        assert_eq!(stats.dependencies_imported, 0);
        assert_eq!(stats.dependencies_outside_pilot_scope, 1);
        assert_eq!(prov.list_assertions_for_release(release).unwrap().len(), 0);
    }

    #[test]
    fn self_referencing_edges_from_the_source_are_the_caller_s_job_to_drop() {
        // scope_pilot.pyがself-loopを既に落とす前提——ここではアダプタが
        // src==depを渡された場合に単に2つの別assertion扱いしないことだけ確認
        // (self-loop除去そのものはPython前処理の責務、Rust側は信頼して素通し)。
        let (prov, release) = store_with_release();
        let statements = vec![stmt("s1", "Foo.bar", "Mathlib.Foo", ExternalClassification::ExternalLiteralDependency)];
        let edges = vec![PilotEdge { src_id: "s1".into(), dep_id: "s1".into(), edge_type: "def".into(), role: None, via_proj: false }];
        let stats = prov.transaction(|| import_pilot(&prov, release, &statements, &edges, "rev-1")).unwrap();
        assert_eq!(stats.dependencies_imported, 1, "adapter itself does not filter self-loops — scope_pilot.py already did");
    }

    #[test]
    fn rerunning_the_same_pilot_is_idempotent() {
        let (prov, release) = store_with_release();
        let statements = vec![
            stmt("s1", "Foo.bar", "Mathlib.Foo", ExternalClassification::ExternalLiteralDependency),
            stmt("s2", "Foo.baz", "Mathlib.Foo", ExternalClassification::ExternalLiteralDependency),
        ];
        let edges = vec![PilotEdge { src_id: "s1".into(), dep_id: "s2".into(), edge_type: "sig".into(), role: None, via_proj: false }];

        prov.transaction(|| import_pilot(&prov, release, &statements, &edges, "rev-1")).unwrap();
        let second = prov.transaction(|| import_pilot(&prov, release, &statements, &edges, "rev-1")).unwrap();
        assert_eq!(second.declarations_imported, 0);
        assert_eq!(second.declarations_skipped_existing, 2);
        assert_eq!(second.dependencies_imported, 0);
        assert_eq!(second.dependencies_skipped_existing, 1);
        assert_eq!(prov.assertion_count().unwrap(), 1);
    }

    /// P8.4（`docs/P8_4_STATUS.md`）: the actual "independently importable and
    /// removable" contract — two projects in the same DB, remove one by
    /// repo_slug, the other must be completely untouched (not just "still
    /// present" but byte-identical entity/assertion/source_record counts).
    #[test]
    fn remove_project_removes_only_the_named_project_and_leaves_the_other_intact() {
        let (prov, release) = store_with_release();
        let statements = vec![
            stmt_in_project("a1", "ProjA.one", "ProjectA", ExternalClassification::ExternalTypeclassHierarchy),
            stmt_in_project("a2", "ProjA.two", "ProjectA", ExternalClassification::ExternalTypeclassHierarchy),
            stmt_in_project("b1", "ProjB.one", "ProjectB", ExternalClassification::ExternalTypeclassHierarchy),
            stmt_in_project("b2", "ProjB.two", "ProjectB", ExternalClassification::ExternalTypeclassHierarchy),
        ];
        let edges = vec![
            PilotEdge { src_id: "a1".into(), dep_id: "a2".into(), edge_type: "def".into(), role: None, via_proj: false },
            PilotEdge { src_id: "b1".into(), dep_id: "b2".into(), edge_type: "def".into(), role: None, via_proj: false },
        ];
        prov.transaction(|| import_pilot(&prov, release, &statements, &edges, "rev-1")).unwrap();
        assert_eq!(prov.entity_count().unwrap(), 4);
        assert_eq!(prov.assertion_count().unwrap(), 2);

        let stats = prov.transaction(|| remove_project(&prov, "ProjectA")).unwrap();
        assert_eq!(stats.declarations_removed, 2);
        assert_eq!(stats.dependencies_removed, 1);

        assert_eq!(prov.entity_count().unwrap(), 2, "ProjectB's 2 declarations must remain");
        assert_eq!(prov.assertion_count().unwrap(), 1, "ProjectB's 1 edge must remain");
        assert!(prov.resolve_entity_ref("judgment:mathgraph:a1").unwrap().is_none());
        assert!(prov.resolve_entity_ref("judgment:mathgraph:a2").unwrap().is_none());
        assert!(prov.resolve_entity_ref("judgment:mathgraph:b1").unwrap().is_some(), "ProjectB must be untouched");
        assert!(prov.resolve_entity_ref("judgment:mathgraph:b2").unwrap().is_some(), "ProjectB must be untouched");
    }

    #[test]
    fn remove_project_reports_zero_for_an_unknown_repo_slug_without_touching_anything() {
        let (prov, release) = store_with_release();
        let statements = vec![stmt_in_project("a1", "ProjA.one", "ProjectA", ExternalClassification::ExternalTypeclassHierarchy)];
        prov.transaction(|| import_pilot(&prov, release, &statements, &[], "rev-1")).unwrap();

        let stats = prov.transaction(|| remove_project(&prov, "NoSuchProject")).unwrap();
        assert_eq!(stats.declarations_removed, 0);
        assert_eq!(prov.entity_count().unwrap(), 1, "the real project must be untouched by a no-match removal");
    }

    /// A project can be removed and then re-imported to reach the exact same
    /// state — the concrete guarantee "removable" needs to be worth anything
    /// (otherwise "removable" could mean "removable but not re-addable").
    #[test]
    fn a_removed_project_can_be_reimported_to_identical_counts() {
        let (prov, release) = store_with_release();
        let statements = vec![
            stmt_in_project("a1", "ProjA.one", "ProjectA", ExternalClassification::ExternalTypeclassHierarchy),
            stmt_in_project("a2", "ProjA.two", "ProjectA", ExternalClassification::ExternalTypeclassHierarchy),
        ];
        let edges = vec![PilotEdge { src_id: "a1".into(), dep_id: "a2".into(), edge_type: "def".into(), role: None, via_proj: false }];
        prov.transaction(|| import_pilot(&prov, release, &statements, &edges, "rev-1")).unwrap();
        let before = (prov.entity_count().unwrap(), prov.assertion_count().unwrap(), prov.source_record_count().unwrap());

        prov.transaction(|| remove_project(&prov, "ProjectA")).unwrap();
        assert_eq!(prov.entity_count().unwrap(), 0);

        prov.transaction(|| import_pilot(&prov, release, &statements, &edges, "rev-1")).unwrap();
        let after = (prov.entity_count().unwrap(), prov.assertion_count().unwrap(), prov.source_record_count().unwrap());
        assert_eq!(before, after, "remove-then-reimport must reach the exact same counts");
    }
}

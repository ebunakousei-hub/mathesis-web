export interface JudgmentView {
  id: number;
  kind: string;
  name?: string;
  statement: string;
  canonical_hash: string;
  parse_status: string;
  context: string[];
  /** `fields.ts` の `FieldNode`/`BridgeNode` の `id` と対応する分野タグ */
  fields: string[];
}

/**
 * `mathesis-taxonomy export` が書き出す `taxonomy.json` の型
 * （Rust側は crates/mathesis-taxonomy/src/export.rs、Phase 1〜5の
 * パイプライン出力そのもの。手書きの静的taxonomyではない）。
 */
export interface ExportedMember {
  phrase: string;
  docFreq: number;
  mscCode: string | null;
}

export interface ExportedCluster {
  id: number;
  size: number;
  confidence: number;
  dominantCode: string | null;
  dominantName: string | null;
  members: ExportedMember[];
}

export interface ExportedField {
  code: string;
  name: string;
  clusterCount: number;
  conceptCount: number;
  clusters: ExportedCluster[];
}

/** Phase 7: hybrid search用の索引の1件ぶん（`SearchIndexColumns` を展開したもの）。 */
export interface SearchEntry {
  phrase: string;
  docFreq: number;
  mscCode: string | null;
  clusterId: number | null;
  /**
   * 分野集中度（Rust側 `crates/mathesis-taxonomy/src/concentration.rs`）。
   * この語を含む論文の分野分布が、コーパス全体の分野分布からどれだけ
   * 離れているか。高いほど特定分野に固有の概念、低いほど分野を問わず
   * 使われる語。分野ラベルの付いた論文が少なく判定できなかった候補は
   * `null`（コーパスの大半はこちら——実データ80,727件中3,822件のみ
   * スコアを持つ）。
   */
  fieldConcentration: number | null;
}

/**
 * 検索索引の配信形式。1件ごとのオブジェクトではなく列で届く
 * （Rust側 `export.rs::SearchIndexColumns`）——キー名が候補の数だけ
 * 繰り返されるのを避けるため。実データ80,727件で7.81MB→2.88MB。
 * 各配列は同じ長さで、同じ添字が同じ候補を指す。
 */
export interface SearchIndexColumns {
  phrase: string[];
  docFreq: number[];
  mscCode: (string | null)[];
  clusterId: (number | null)[];
  fieldConcentration: (number | null)[];
}

export interface RelatedEdge {
  phrase: string;
  score: number;
}

/**
 * embedding近傍の辺リスト。**別ファイル**（`taxonomy.related.json`）で
 * 配信され、利用者が実際に検索したときに初めて読み込まれる。
 * 実データで10.57MB——これを同梱していた頃は、ページを開いただけの
 * 利用者もこの62%を待たされてから概念エクスプローラーが動き始めていた。
 */
export interface RelatedEdgesExport {
  source: string[];
  targets: string[][];
  scores: number[][];
}

/** 検索結果に出す論文1件。 */
export interface ConceptPaper {
  arxivId: string;
  title: string;
  year: number | null;
  primaryCategory: string | null;
}

/**
 * 概念ごとの出典論文（`taxonomy.papers.json`、別ファイル）。
 *
 * 以前この構造は `arxivIds: string[][]` ——概念ごとに生のarXiv ID最大3件
 * だけ——だった。つまりこのアプリのバンドルには**論文オブジェクトが1件も
 * 入っていなかった**: 題名も年も無く、利用者は "0905.3137" という文字列を
 * 見せられてサイトの外へ出るしかなかった。検索結果の単位が「文書」では
 * なく「フレーズ」だったということで、検索としての体裁との最大の距離が
 * ここだった。
 *
 * 論文本体は列で1回だけ持ち、概念からは添字で参照する（同じ論文が
 * 多数の概念に現れるので、題名を概念ごとに繰り返すと配信量が跳ね上がる）。
 * `source[i]` の論文が `papers[i]`（`arxivId`等の配列への添字）。
 * `RelatedEdgesExport` と同じく検索時の遅延読み込み。
 */
export interface PapersExport {
  arxivId: string[];
  title: string[];
  year: (number | null)[];
  primaryCategory: (string | null)[];
  source: string[];
  papers: number[][];
}

/**
 * 表記ゆれ（`taxonomy.aliases.json`、別ファイル。実データで0.75MBと小さい）。
 * `representative[i]` に畳まれた別表記が `aliases[i]`。
 *
 * 畳んだ事実を画面に出すために使う——黙って畳むと、利用者は自分が打った
 * 表記が結果に出てこない理由が分からない（綴り訂正を必ず表示しているのと
 * 同じ理由）。
 */
export interface AliasExport {
  representative: string[];
  aliases: string[][];
}

/**
 * 型付き関係（`relations.json`、別ファイル。アーキテクチャ.txt 5.3の
 * `Relation`——特殊化/同値）。P2（`docs/P2_STATUS.md`）以降、
 * `mathesis-taxonomy`の`RelationStatus`を直接読むのではなく、
 * `mathesis-provenance web-export`が証拠層（`RelationAssertion`+
 * `Evidence`）から1件ずつ再構成して書き出す——`assertionId`が最初から
 * 埋め込まれているので、`assertions.json`との突き合わせなしにこの1件だけで
 * 出典まで辿れる。`kind`は"specialization_of"（subjectはobjectの特殊化）
 * か"equivalent_to"（同一概念の異なる定式化）。
 *
 * **根拠文を持つものだけがここに来る**——統計のみで根拠文の無いProposedは
 * 含まれない。`status`は"confirmed"（分布統計と本文の一文が一致）か
 * "grounded"（本文の一文のみ）か、P6.3で加わった"reviewed"（本人確認済み
 * レビューで昇格した意味的関係——`mathesis-provenance promote-review`、
 * `docs/P6_3_STATUS.md`）。`confidence`はConfirmedにしか無い実測値
 * （invCLメトリック）——Groundedは`null`（旧`taxonomy.relations.json`が
 * 出していた固定1.0のプレースホルダは、ここでは捏造しない）。手動サンプルで
 * 確認した実測精度は約50%（当初36%、3回の的を絞った修正後、
 * `relations.rs`冒頭コメント参照）——だからこそ`evidenceSentence`を必ず
 * 一緒に見せ、読者がその場で自分の目で判断できるようにする。
 */
export interface ProvenanceRelationEdge {
  assertionId: number;
  subject: string;
  object: string;
  kind: "specialization_of" | "equivalent_to";
  status: "confirmed" | "grounded" | "reviewed";
  confidence: number | null;
  evidenceSentence: string;
  evidenceArxivId: string;
  /**
   * P6.3: `DependencyEdge`/`MorphismEdge`と同じ語彙。現時点でこの画面
   * (`dynamicTaxonomy.ts`の概念詳細)自体には「trusted only」トグルは
   * 無い(リネージビューだけが持つ、`docs/P5_PLAN.md`)——このフィールドは
   * 将来そのトグルを追加する際に他の2種と揃えるためのデータで、今回の
   * 増分ではまだUIから参照しない。
   */
  traversalPolicy: "excluded" | "visible_only" | "default_traversal" | "formal_only";
}

export interface TaxonomyExport {
  /** このJSONが書き出された時刻（UNIX秒）。外部レビュー（2026-09-05）で
   * 「データの生成日時が見えない」と指摘され追加された。 */
  generatedAtUnix: number;
  paperCount: number;
  candidateCount: number;
  /** Entity Resolution（`resolve.rs`）で表記ゆれを畳んだ後の概念数。 */
  resolvedConceptCount: number;
  clusterCount: number;
  ambiguousClusterCount: number;
  fields: ExportedField[];
  novelClusters: ExportedCluster[];
  searchIndex: SearchIndexColumns;
}

/**
 * `TaxonomyExport`から`searchIndex`（実データ9.0MB）を除いた残り。
 *
 * 診断④「配信の不可分性」への対応（`searchWorker.ts`参照）: `searchIndex`
 * のJSON.parseと索引構築は合わせて実測632msかかりメインスレッドを
 * 塞いでいたため、Worker上で行うようにした。Workerが返す「殻」
 * （分野カード一覧・統計等、検索とは無関係な表示用データ）はこの型で、
 * フル索引そのもの（`ConceptSearchIndex`、`queryIndex.ts`）は別途
 * `DynamicTaxonomyExplorer.searchIndex`として保持する。
 */
export type TaxonomyShell = Omit<TaxonomyExport, "searchIndex">;

/**
 * Phase 10: `mathesis-import --export` が書き出す `judgments.json` の型
 * （Rust側は crates/mathesis-graph/src/export.rs）。Phase 9で取り込んだ
 * Lean判断グラフ（`mathesis-graph`の実データ、静的デモの`KernelStore`とは別物）。
 */
export interface ExportedJudgment {
  id: number;
  kind: string;
  name: string | null;
  statement: string;
  context: string[];
  parseStatus: string;
  sourceFile: string;
  sourceLine: number;
  paperArxivId: string | null;
}

/**
 * `dependencies.json`（`mathesis-provenance web-export`が証拠層から直接
 * 生成、P2 `docs/P2_STATUS.md`）1件ぶん。以前は`judgments.json`
 * （`GraphExport.dependencies`）に載っていたが、その場では常に
 * `epistemic_state: extracted`という中身の解釈まで`mathesis-graph`が
 * 決めていた——今はその解釈を証拠層のassertionから読む、別ファイルに移した。
 */
export interface ExportedGraphDependency {
  assertionId: number;
  from: number;
  to: number;
  /**
   * P5, Item 1（`docs/P5_PLAN.md`）: `mathesis-provenance web-export`が
   * `relation_policy::traversal_policy`から直接書き出す、この辺の信頼度
   * ("excluded"|"visible_only"|"default_traversal"|"formal_only")。
   * 実データでは`depends_on`は全件`extracted`（Lean elaboratorの正式exportで
   * はなく名前一致抽出のため`observed`ではない）——よって現状は全件
   * `visible_only`。0件になることも含めて正直に表示する
   * （`docs/P5_STATUS.md`参照）。
   */
  traversalPolicy: "excluded" | "visible_only" | "default_traversal" | "formal_only";
  /**
   * Priority 2, step 1（ユーザー指示 2026-09-08）: `"checker-derived"`
   * (Lean elaboratorの実行結果、`crates/mathesis-lean-extract`)か
   * `"text-extracted"`(`mathesis-importer`の識別子名一致)か。
   */
  origin: "checker-derived" | "text-extracted";
}

export interface ExportedGraphPaper {
  arxivId: string;
  title: string | null;
  judgmentCount: number;
  /** 診断⑥拡張: この論文が引用している他の論文のarXiv id（解決できた分だけ）。 */
  cites: string[];
}

/**
 * 層3の射（`morphisms.json`、`mathesis-provenance web-export`が証拠層から
 * 直接生成、P2 `docs/P2_STATUS.md`）。`ExportedGraphDependency`
 * （証明本体が参照する判断、Phase 9）とは別物——含意・特殊化・一般化・
 * 同値という論理的な関係を表す。現状は`mathesis-importer --propose-morphisms`
 * がヒューリスティックで機械的に提案した`status: "proposed"`（未承認）候補
 * のみで、人間によるレビュー・承認はまだ行っていない。
 *
 * `id`は旧来の`mathesis-graph`側の射idではなく、このassertion自身のid——
 * 1射につきassertionが必ず1件なので識別子として完全に代用でき、
 * `assertions.json`（詳細パネル用）を`id`でそのまま引ける。`kind`/`origin`/
 * `status`/`rationale`は`morphisms`テーブルの生の値ではなく、この
 * assertionのEvidence/ReviewDecisionから再構成されたもの。
 */
export interface ExportedMorphism {
  id: number;
  src: number;
  dst: number;
  kind: "implication" | "specialization" | "generalization" | "equivalence";
  origin: "manual" | "heuristic";
  status: "proposed" | "accepted" | "rejected";
  rationale: string | null;
  /** `ExportedGraphDependency.traversalPolicy`と同じ語彙。 */
  traversalPolicy: "excluded" | "visible_only" | "default_traversal" | "formal_only";
}

/**
 * `assertions.json`（`mathesis-provenance::assertion_export`）1件ぶんの
 * 全詳細。id文字列をキーにした辞書として配信される
 * （`Record<string, AssertionDetail>`）。
 */
export interface EvidenceDetail {
  evidenceKind: string;
  locator: string | null;
  /**
   * `assertion_export.rs::evidence_details_for`が`evidenceKind`(+`locator`
   * の有無)から導く、位置特定の精度("formal_artifact"|"model_output"|
   * "reviewer_note"|"approximate_location"|"source_only")。実データ以前は
   * 型に無かった実在フィールド——`locatorPrecision`が欠けているとRust側と
   * TS側の形がずれる。
   */
  locatorPrecision: string;
  extractorOrModel: string | null;
  metricName: string | null;
  metricValue: number | null;
  sourceProvider: string;
  sourceProviderId: string;
  /**
   * P6.1（`docs/LEAN_DEPENDENCY_POLICY.md`）: `evidenceKind ===
   * "formal_export"`の依存辺だけが持つ、`"type"`|`"body"`|`"both"`——
   * 対象宣言の型・値(証明項)のどちらから見つかった参照か。それ以外は`null`。
   */
  dependencyOrigin: "type" | "body" | "both" | null;
  /**
   * P6.1: `evidenceKind === "formal_export"`だけが持つ、Lean/mathlib版・
   * フィルタポリシー版をまとめた短い1行("leanprover/lean4:v4.29.0-rc6,
   * mathlib 5c8398df, filter policy mathesis-lean-dependency-filter-v1")。
   */
  formalRevision: string | null;
}

export interface ReviewDecisionDetail {
  decision: string;
  reviewerId: string | null;
  /**
   * P6.3（`docs/P6_3_STATUS.md`）: 承認者が「どんな資格で」承認したか。
   * `null`はリリースゲートが本人確認済みと認めない（審査済みの身元だけでは
   * 足りない——資格の表明も必須）。
   */
  authorizationLevel: string | null;
  scope: string | null;
  rationale: string | null;
  decidedAtUnix: number;
  /**
   * P6.3: このレビューが「見た」リリースタグ。`AssertionDetail.releaseTag`と
   * 食い違えば、別リリース時点のレビューがそのまま今のリリースへ横流し
   * されていないかを読者自身が確かめられる。
   */
  datasetVersion: string | null;
  expiresAtUnix: number | null;
  /**
   * P6.3: サーバ側(`review::is_authenticated_accept`)が判定済みの
   * 「今、これがリリースゲートを通す本人確認済みaccept/supersedeか」。
   * 同じ判断ログのうち最新の1件だけが`true`になりうる——古いacceptが
   * 後からrevoke/rejectで効力を失っていても行自体は残る(追記専用ログ)。
   */
  isCurrentAuthenticatedAccept: boolean;
}

export interface AssertionDetail {
  id: number;
  subjectRef: string;
  predicate: string;
  objectRef: string;
  epistemicState: string;
  score: number | null;
  releaseTag: string;
  evidence: EvidenceDetail[];
  reviewDecisions: ReviewDecisionDetail[];
  eligibleForDefaultTraversal: boolean;
  /**
   * P5, Item 1（`docs/P5_PLAN.md`）: `eligibleForDefaultTraversal`の元に
   * なった4値そのもの("excluded"|"visible_only"|"default_traversal"|
   * "formal_only")——真偽値だけでは「なぜ既定トラバース対象外か」
   * （却下されたのか、まだ根拠が弱いだけなのか、形式的な文脈でのみ通用する
   * のか）が伝わらない。
   */
  traversalPolicy: "excluded" | "visible_only" | "default_traversal" | "formal_only";
  /**
   * P3, Increment 1（`docs/P3_STATUS.md`、`mathesis-provenance
   * build-catalog`が作る型付きエンティティカタログ）: `subjectRef`/
   * `objectRef`の人間可読な表示名。カタログ未構築、またはその参照がまだ
   * カタログに載っていなければ`null`——その場合は`subjectRef`のタグ付き
   * 文字列をそのまま見せる（無いラベルを捏造しない）。
   */
  subjectLabel: string | null;
  objectLabel: string | null;
  /** ラベルの由来("source_provided"|"canonicalized"|"derived"|"fallback_identifier")。ラベルが`null`なら同じく`null`。 */
  subjectLabelOrigin: string | null;
  objectLabelOrigin: string | null;
}

/**
 * `judgments.json`。P2（`docs/P2_STATUS.md`）以降、辺そのもの
 * （`dependencies`/`morphisms`）はここには無い——別途`dependencies.json`/
 * `morphisms.json`（`mathesis-provenance web-export`が証拠層から直接生成）
 * を読む。ここに残るのはノード側のデータ（判断・論文）と件数だけ。
 */
export interface GraphExport {
  /** このJSONが書き出された時刻（UNIX秒）。TaxonomyExportと同型の対応。 */
  generatedAtUnix: number;
  judgmentCount: number;
  dependencyCount: number;
  morphismCount: number;
  papers: ExportedGraphPaper[];
  judgments: ExportedJudgment[];
}

/**
 * `mathesis-wasm::parseLeanSource` の戻り値
 * （Rust側は `crates/mathesis-wasm/src/lean.rs::LeanParseView`）。
 * 貼り付けたLeanソースをブラウザ内でその場でパースした結果——
 * `GraphExport`（CLIで事前にインポート・書き出した静的データ）の
 * その場版。`sourceFile`・`paperArxivId`を持たない（貼り付けにファイル名も
 * 出典論文も無いため）以外は`ExportedJudgment`/`ExportedGraphDependency`
 * と同じ形にしてある——`web/src/lineage.ts`の`LineageGraph`をそのまま
 * 組み立てられるように。
 */
export interface LeanParsedJudgment {
  /** この貼り付け内だけで意味を持つ、1始まりの通し番号。 */
  id: number;
  kind: string;
  name: string | null;
  statement: string;
  context: string[];
  parseStatus: string;
  sourceLine: number;
}

export interface LeanParsedDependency {
  from: number;
  to: number;
}

export interface LeanParseResult {
  judgments: LeanParsedJudgment[];
  dependencies: LeanParsedDependency[];
}

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
 * 型付き関係（`taxonomy.relations.json`、別ファイル。アーキテクチャ.txt
 * 5.3の`Relation`——特殊化/同値）。`subject[i]`が`object[i]`の
 * `kind[i]`（"specialization_of" = subjectはobjectの特殊化、
 * "equivalent_to" = 同一概念の異なる定式化）。
 *
 * **根拠文を持つものだけがここに来る**（Rust側`export.rs::
 * RelationsExport`が実装、統計のみで根拠文の無いProposedは含まれない）。
 * `status`は "confirmed"（分布統計と本文の一文が一致）か
 * "grounded"（本文の一文のみ）——どちらも「両経路が一致した／実際に
 * その一文がある」以上の確実性は主張しない。手動サンプルで確認した
 * 実測精度は約50%（当初36%、3回の的を絞った修正後、`relations.rs`
 * 冒頭コメント参照）——だからこそ`evidenceSentence`を必ず一緒に見せ、
 * 読者がその場で自分の目で判断できるようにする。
 */
export interface RelationsExport {
  subject: string[];
  object: string[];
  kind: ("specialization_of" | "equivalent_to")[];
  status: ("confirmed" | "grounded")[];
  confidence: number[];
  evidenceSentence: string[];
  evidenceArxivId: string[];
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

export interface ExportedGraphDependency {
  from: number;
  to: number;
}

export interface ExportedGraphPaper {
  arxivId: string;
  title: string | null;
  judgmentCount: number;
  /** 診断⑥拡張: この論文が引用している他の論文のarXiv id（解決できた分だけ）。 */
  cites: string[];
}

/**
 * 層3の射（`morphisms`テーブル）。`ExportedGraphDependency`
 * （証明本体が参照する判断、Phase 9）とは別物——含意・特殊化・一般化・
 * 同値という論理的な関係を表す。現状は`mathesis-importer --propose-morphisms`
 * がヒューリスティックで機械的に提案した`status: "proposed"`（未承認）候補
 * のみで、人間によるレビュー・承認はまだ行っていない。
 */
export interface ExportedMorphism {
  id: number;
  src: number;
  dst: number;
  kind: "implication" | "specialization" | "generalization" | "equivalence";
  origin: "manual" | "heuristic";
  status: "proposed" | "accepted" | "rejected";
  rationale: string | null;
}

/**
 * Phase 1 (ARCHITECTURE_NEXT.md, `mathesis-provenance`)の証拠層への追跡情報。
 * `judgments.json`/`taxonomy.relations.json`本体には無い追加のサイドカー
 * ファイルで、既存のexportの形は一切変えていない——`mathesis-provenance
 * reconcile`が生成し、`RelationAssertion`のidとリリースタグだけを持つ薄い
 * 索引。存在しなくても（フェッチに失敗しても）既存の画面は今までどおり
 * 動く前提で、あれば追加のツールチップ情報として使う。
 */
export interface JudgmentsProvenanceExport {
  releaseTag: string;
  releaseGitCommit: string | null;
  dependencies: { from: number; to: number; assertionId: number }[];
  citations: { from: string; to: string; assertionId: number }[];
  morphisms: { morphismId: number; assertionId: number }[];
}

export interface RelationsProvenanceExport {
  releaseTag: string;
  releaseGitCommit: string | null;
  relations: { subject: string; object: string; kind: string; assertionId: number }[];
}

/**
 * `assertions.json`（`mathesis-provenance::assertion_export`）1件ぶんの
 * 全詳細。id文字列をキーにした辞書として配信される
 * （`Record<string, AssertionDetail>`）。
 */
export interface EvidenceDetail {
  evidenceKind: string;
  locator: string | null;
  extractorOrModel: string | null;
  metricName: string | null;
  metricValue: number | null;
  sourceProvider: string;
  sourceProviderId: string;
}

export interface ReviewDecisionDetail {
  decision: string;
  reviewerId: string | null;
  scope: string | null;
  rationale: string | null;
  decidedAtUnix: number;
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
}

export interface GraphExport {
  /** このJSONが書き出された時刻（UNIX秒）。TaxonomyExportと同型の対応。 */
  generatedAtUnix: number;
  judgmentCount: number;
  dependencyCount: number;
  morphismCount: number;
  papers: ExportedGraphPaper[];
  judgments: ExportedJudgment[];
  dependencies: ExportedGraphDependency[];
  morphisms: ExportedMorphism[];
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

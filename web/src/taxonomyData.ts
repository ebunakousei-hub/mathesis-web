import type { RelatedEdge, RelatedEdgesExport, SearchEntry, SearchIndexColumns } from "./types";

/**
 * `taxonomy.json` / `taxonomy.related.json` の列形式（`SearchIndexColumns`・
 * `RelatedEdgesExport`）を、検索が実際に使う形へ展開する純粋な変換関数。
 *
 * DOM に一切触れない——`dynamicTaxonomy.ts`（描画クラス）と
 * `searchWorker.ts`（Workerスレッド、DOM自体が存在しない）の**両方**から
 * 同じ関数を呼ぶために切り出した。以前はこの2関数が`dynamicTaxonomy.ts`
 * 内にプライベート関数として置かれていたが、フル索引の構築をWorkerへ
 * 移す際（診断④「配信の不可分性」への対応、`searchWorker.ts`参照）に
 * Workerからも同じ変換が要るようになったため、共有モジュールへ出した。
 */
export function expandSearchIndex(columns: SearchIndexColumns): SearchEntry[] {
  const entries = new Array<SearchEntry>(columns.phrase.length);
  for (let i = 0; i < columns.phrase.length; i++) {
    entries[i] = {
      phrase: columns.phrase[i],
      docFreq: columns.docFreq[i],
      mscCode: columns.mscCode[i],
      clusterId: columns.clusterId[i],
      fieldConcentration: columns.fieldConcentration[i],
    };
  }
  return entries;
}

/** 列形式で届いた近傍の辺を、フレーズ引きのマップへ展開する。 */
export function expandRelatedEdges(columns: RelatedEdgesExport): Record<string, RelatedEdge[]> {
  const map: Record<string, RelatedEdge[]> = {};
  for (let i = 0; i < columns.source.length; i++) {
    const targets = columns.targets[i];
    const scores = columns.scores[i];
    const edges = new Array<RelatedEdge>(targets.length);
    for (let j = 0; j < targets.length; j++) edges[j] = { phrase: targets[j], score: scores[j] };
    map[columns.source[i]] = edges;
  }
  return map;
}

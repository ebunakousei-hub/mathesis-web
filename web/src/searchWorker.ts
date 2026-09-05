/// <reference lib="webworker" />
import { buildConceptSearchIndex, type ConceptSearchIndex } from "./queryIndex";
import { expandSearchIndex } from "./taxonomyData";
import type { TaxonomyExport, TaxonomyShell } from "./types";

/**
 * `taxonomy.json`（実データ9.0MB）のfetch・JSON.parse・索引構築を
 * メインスレッドの外で行う（診断④「配信の不可分性」への対応）。
 *
 * # なぜ要るか
 *
 * `web/README.md`「今後の課題」で測った実データの数字: `taxonomy.json`
 * のJSON.parseが378ms、`queryIndex.ts::buildConceptSearchIndex`
 * （転置索引・IDF計算）が254ms——合わせて632msがメインスレッドを
 * **不可分に**塞いでいた。壁はバイト数（gzip後1.6MBは軽い）ではなく
 * この不可分性そのもので、コーパスが今後さらに大きくなるほど悪化する。
 * Web Workerへ丸ごと移せば、この632msはメインスレッドの外で起きるため、
 * ページは読み込み中もクリック・入力に応答し続けられる。
 *
 * `taxonomy.related.json`（related段階用、10.57MB）は意図的にここへ
 * 含めていない——`dynamicTaxonomy.ts::ensureRelatedEdges`が既に
 * 「利用者が実際に検索するまで取りに行かない」という遅延読み込みを
 * しており、ここで一緒に取ってしまうとページを開いただけの利用者にも
 * 常時ダウンロードさせることになり、既存の意図的な設計を壊す。
 *
 * `ConceptSearchIndex`の中身（`Map`・`Int32Array`・`Float64Array`・
 * 配列・文字列・数値のみ）はいずれも構造化複製（structured clone）に
 * 対応しているため、`postMessage`でメインスレッドへそのまま渡せる
 * ——出来上がった索引を再度メインスレッドで構築し直す必要は無い。
 */

export interface SearchWorkerReadyMessage {
  type: "ready";
  shell: TaxonomyShell;
  searchIndex: ConceptSearchIndex;
}

export interface SearchWorkerErrorMessage {
  type: "error";
  message: string;
}

export type SearchWorkerMessage = SearchWorkerReadyMessage | SearchWorkerErrorMessage;

async function run(): Promise<void> {
  try {
    const resp = await fetch(`${import.meta.env.BASE_URL}taxonomy.json`);
    if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
    const data = (await resp.json()) as TaxonomyExport;

    const searchIndex = buildConceptSearchIndex(expandSearchIndex(data.searchIndex));
    const shell: TaxonomyShell = {
      generatedAtUnix: data.generatedAtUnix,
      paperCount: data.paperCount,
      candidateCount: data.candidateCount,
      resolvedConceptCount: data.resolvedConceptCount,
      clusterCount: data.clusterCount,
      ambiguousClusterCount: data.ambiguousClusterCount,
      fields: data.fields,
      novelClusters: data.novelClusters,
    };

    const message: SearchWorkerReadyMessage = { type: "ready", shell, searchIndex };
    postMessage(message);
  } catch (err) {
    const message: SearchWorkerErrorMessage = { type: "error", message: String(err) };
    postMessage(message);
  }
}

void run();

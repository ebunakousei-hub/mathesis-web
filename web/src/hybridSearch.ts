import {
  buildConceptSearchIndex,
  collectMatches,
  queryIdfMass,
  resolveQuery,
  splitWords,
  type ConceptSearchIndex,
  type Correction,
} from "./queryIndex";
import type { RelatedEdge, SearchEntry } from "./types";

/**
 * Phase 7のhybrid search（アーキテクチャ.txt 5.7）をブラウザ側で行う。
 * 4段階（exact / same concept / specialization / related）という
 * 意味づけは `crates/mathesis-taxonomy/src/search.rs::hybrid_search` と
 * 同じだが、**クエリの受け取り方**はブラウザ側で作り直してある。
 *
 * 旧実装は各段階が「クエリ文字列との完全一致」「クエリ語列が候補に連続して
 * 含まれるか」だけで出来ており、正解のフレーズを既に知っている人にしか
 * 使えなかった（実データ80,727候補で、現実的なクエリの67%が全段階0件）。
 * 現在は `queryIndex.ts` の解釈層を通し、
 *   - 大小文字・ダイアクリティカルマーク・単複を正規化する
 *     （"Kähler Manifolds" と "kahler manifold" は同じ）
 *   - "the" / "of" / "what is" のような機能語を落とす
 *   - 語順を問わない（"manifold kahler" でも "kahler manifold" に着地）
 *   - 索引に無い語は綴りを訂正する（"reimann" → "riemann"）
 *   - 全語が一致しなくても、一致した語数（被覆率）で順位を付けて返す
 * ようにした。これにより「意図に最も近い概念に必ず着地する」。
 *
 * Ollamaへの通信は従来どおり行わない（taxonomy.jsonは静的スナップショット
 * という方針のまま）。ただし related 段階は、旧実装が「完全一致した候補が
 * あるときだけ」引いていたのを、**その検索で最も確からしい候補**の近傍を
 * 引くように変えた——完全一致しないクエリでも関連概念が出るようになる。
 */

export interface SearchHit {
  phrase: string;
  docFreq: number;
  mscCode: string | null;
  score: number;
  /** 分野集中度（`SearchEntry.fieldConcentration`）。判定できなかった候補は null。 */
  fieldConcentration: number | null;
}

export interface HybridSearchResult {
  exact: SearchHit[];
  sameConcept: SearchHit[];
  specialization: SearchHit[];
  related: SearchHit[];
  /**
   * クエリの一部の語だけが一致した候補。旧実装ではこれらは1件も返らず
   * 「該当なし」になっていた——利用者から見れば、綴りが違うのか、
   * 概念が存在しないのか、言い方が違うのかの区別が付かない行き止まり
   * だった。被覆率の高い順に返す。
   */
  closest: SearchHit[];
  /** 綴りを訂正した語（「◯◯として検索しました」と提示するため） */
  corrections: Correction[];
  /** 解釈の結果、実際に検索に使った語列 */
  interpretedTokens: string[];
}

export type { ConceptSearchIndex, Correction };
export { buildConceptSearchIndex };

const EMPTY_RESULT: HybridSearchResult = {
  exact: [],
  sameConcept: [],
  specialization: [],
  related: [],
  closest: [],
  corrections: [],
  interpretedTokens: [],
};

function toHit(e: SearchEntry, score: number): SearchHit {
  return {
    phrase: e.phrase,
    docFreq: e.docFreq,
    mscCode: e.mscCode,
    score,
    fieldConcentration: e.fieldConcentration ?? null,
  };
}

/** 文書頻度（コーパスでの人気）をどれだけ効かせるか。 */
const POPULARITY_WEIGHT = 0.15;

/**
 * 再現率をどれだけ適合率より重く見るか（F値の β）。
 *
 * 2.0 は「再現率を適合率の2倍重く見る」。この用途では、利用者が打った語を
 * 満たすこと（再現率）の方が、候補に余計な語が付いていないこと（適合率）
 * より大事だから。β=1（対称なF1）で実データを測ると、クエリ
 * "kahler space" に対して裸の1語 "kähler" が "kähler moduli space" を
 * 0.82 対 0.80 で押しのけた——1語の候補は適合率が必ず1.0になるので、
 * 対称なF値では構造的に有利になりすぎる。β=2 だと 0.745 対 0.910 と
 * 順序が入れ替わり、複合語が正しく上に来る。
 */
const RECALL_BETA = 2.0;

/**
 * 順位付けに使う 0〜1 のスコア。IDF重み付きの再現率と適合率のF値。
 *
 *   再現率 r = 一致した語のIDF合計 / クエリ語のIDF合計
 *             （クエリの「絞り込みに効く部分」をどれだけ満たしたか）
 *   適合率 p = 一致した語のIDF合計 / 候補の語のIDF合計
 *             （候補がクエリでどれだけ説明され切っているか）
 *   score  = F_β(r, p) を人気で少しだけ補正
 *
 * IDF重みが要る理由: これが無かった頃は、クエリの語をすべて同じ重みで
 * 数えていた。"kähler manifold" というクエリで、80,727概念のうち数千件に
 * 現れる "manifold" と数十件にしか現れない "kähler" が同格に扱われ、
 * どちらの語が絞り込みに効くかという情報が捨てられていた。
 *
 * F値の形にした理由（教科書どおりのBM25にしなかった理由）: BM25を実装して
 * 実データで測ったところ、その長さ正規化が**1語の候補を不当に優遇した**。
 * クエリ "kahler space" に対して "kähler"・"kahler" という裸の1語が
 * "kähler moduli space" のような複合語を押しのけて2〜3位に入る
 * （"stability conditions ..." でも同様に裸の "stability" が上がった）。
 * 概念タクソノミーでは1語の候補はたいてい複合語より情報が少ないので、
 * これは望ましくない。再現率だけでなく適合率も掛けると、クエリを
 * 説明し切れていない短い候補は適合率では得をしても再現率で落ちるため、
 * この病理が消える。
 */
function relevance(matchedIdf: number, queryIdfMass: number, entryIdfMass: number, docFreq: number): number {
  if (queryIdfMass <= 0 || entryIdfMass <= 0) return 0;
  const recall = Math.min(1, matchedIdf / queryIdfMass);
  const precision = Math.min(1, matchedIdf / entryIdfMass);
  const b2 = RECALL_BETA * RECALL_BETA;
  const denominator = b2 * precision + recall;
  const f = denominator > 0 ? ((1 + b2) * precision * recall) / denominator : 0;
  const popularity = Math.min(1, Math.log10(1 + docFreq) / 4);
  return (1 - POPULARITY_WEIGHT) * f + POPULARITY_WEIGHT * popularity;
}

export function hybridSearch(
  query: string,
  index: ConceptSearchIndex,
  relatedEdges: Record<string, RelatedEdge[]>,
  topK: number,
): HybridSearchResult {
  const { tokens, corrections } = resolveQuery(query, index);
  if (tokens.length === 0) return EMPTY_RESULT;

  const entries = index.entries;
  const normalizedQuery = tokens.join(" ");
  const distinctQueryTokens = new Set(tokens).size;

  const exactIdx: number[] = [];
  const specializationIdx: number[] = [];
  const closestIdx: number[] = [];
  const scoreOf = new Map<number, number>();

  const idfMass = queryIdfMass(tokens, index);
  for (const { entry, matched, matchedIdf } of collectMatches(tokens, index)) {
    const entryTokens = index.tokenCount[entry];
    scoreOf.set(entry, relevance(matchedIdf, idfMass, index.entryIdfMass[entry], entries[entry].docFreq));

    if (matched < distinctQueryTokens) {
      closestIdx.push(entry);
    } else if (entryTokens === distinctQueryTokens) {
      // クエリの全語を、余計な語なしで持つ候補。語順が違っていても
      // （"manifold kahler" → "kahler manifold"）ここに入れる。
      exactIdx.push(entry);
    } else {
      specializationIdx.push(entry);
    }
  }

  // 正規化フレーズがそのまま一致する候補は、語順一致より確実な完全一致
  // なので必ず先頭に置く。
  const verbatim = new Set(index.byPhrase.get(normalizedQuery) ?? []);
  const byScore = (a: number, b: number): number => {
    const verbatimDelta = Number(verbatim.has(b)) - Number(verbatim.has(a));
    if (verbatimDelta !== 0) return verbatimDelta;
    return (scoreOf.get(b) ?? 0) - (scoreOf.get(a) ?? 0);
  };
  exactIdx.sort(byScore);
  specializationIdx.sort(byScore);
  closestIdx.sort(byScore);

  const shown = new Set<number>();
  const take = (ids: number[], limit: number): SearchHit[] => {
    const hits: SearchHit[] = [];
    for (const id of ids) {
      if (hits.length >= limit) break;
      if (shown.has(id)) continue;
      shown.add(id);
      hits.push(toHit(entries[id], scoreOf.get(id) ?? 0));
    }
    return hits;
  };

  const exact = take(exactIdx, topK);

  // same concept: 完全一致した候補と同じクラスタに属する概念。
  const exactClusters = new Set(
    exactIdx
      .slice(0, topK)
      .map((id) => entries[id].clusterId)
      .filter((c): c is number => c !== null),
  );
  const sameConceptIdx: number[] = [];
  for (const clusterId of exactClusters) {
    for (const id of index.byCluster.get(clusterId) ?? []) {
      if (!shown.has(id)) sameConceptIdx.push(id);
    }
  }
  sameConceptIdx.sort((a, b) => entries[b].docFreq - entries[a].docFreq);
  const sameConcept = sameConceptIdx.slice(0, topK).map((id) => {
    shown.add(id);
    return toHit(entries[id], 1);
  });

  const specialization = take(specializationIdx, topK);
  const closest = take(closestIdx, topK);

  // related: 「この検索で最も確からしい候補」の事前計算済み近傍を引く。
  // 旧実装は完全一致があるときしか引かなかったので、少しでも言い方が
  // 違うクエリでは常に空だった。
  const anchor = exactIdx[0] ?? specializationIdx[0] ?? closestIdx[0];
  let related: SearchHit[] = [];
  if (anchor !== undefined) {
    const shownPhrases = new Set<string>();
    for (const id of shown) shownPhrases.add(entries[id].phrase);
    related = (relatedEdges[entries[anchor].phrase] ?? [])
      .filter((edge) => !shownPhrases.has(edge.phrase))
      .slice(0, topK)
      .map((edge) => {
        const found = index.byPhrase.get(splitWords(edge.phrase).join(" "));
        const entry = found === undefined ? undefined : entries[found[0]];
        return entry !== undefined
          ? toHit(entry, edge.score)
          : { phrase: edge.phrase, docFreq: 0, mscCode: null, score: edge.score, fieldConcentration: null };
      });
  }

  return { exact, sameConcept, specialization, related, closest, corrections, interpretedTokens: tokens };
}

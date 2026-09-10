/**
 * P8.3（`docs/P8_3_STATUS.md`）: Math-Graphパイロットの依存辺から、1件の
 * 宣言を中心にした**局所近傍**（直接の依存先・直接の依存元、各1段だけ）を
 * 組み立てる純粋なグラフ演算層——描画は`mathGraphLineageView.ts`の仕事
 * （`lineage.ts`/`lineageView.ts`の分離と同じ方針）。
 *
 * `lineage.ts`が持つ多段の系譜アルゴリズム（背骨検出・層別配置・重心法に
 * よる交差低減）は意図的に再利用しない——あれはMathesis自身の判断
 * （`ExportedJudgment`、命題本文・射・信頼度つき）専用に深く結び付いており、
 * Math-Graphの宣言（本文テキストなし、P7の意図的なスコープ）を無理に
 * その型へ押し込むと、ディレクティブ自身が禁じている「ordinary Lean
 * declaration nodesとして表示する」ことになってしまう
 * （`docs/P8_3_STATUS.md`の設計判断を参照）。ここではその代わりに、
 * 1宣言を中心とした3段（依存元・本人・依存先）だけの、意図的にずっと
 * 単純な近傍ビューを組む——「有界で中断可能な展開」（ディレクティブの
 * 受け入れ基準）を、複雑な省略ロジックではなく設計そのもので満たす。
 */
import type { DiscoveryEdge } from "./types";

/** 片側（依存先／依存元）に表示する上限件数。 */
export const MAX_NEIGHBORS = 12;

export interface MathGraphNeighborNode {
  /** 隣接する宣言のラベル（`DiscoveryEdge.subject`/`.object`と同じ文字列）。 */
  label: string;
  /** 焦点ノードとこの隣接ノードを結ぶ辺そのもの——由来・ライセンス・
   * locatorはここから引く("inspect why an edge exists"要求への対応)。 */
  edge: DiscoveryEdge;
}

export interface MathGraphNeighborhood {
  focus: string;
  /** 焦点が依存している宣言（焦点がedge.subject側）。 */
  dependsOn: MathGraphNeighborNode[];
  dependsOnOmitted: number;
  /** 焦点に依存している宣言（焦点がedge.object側）。 */
  usedBy: MathGraphNeighborNode[];
  usedByOmitted: number;
}

/**
 * `edges`（1プロジェクトぶん、`sourceProject`で既に絞り込み済みのものを
 * 渡す想定）から、`focus`を中心にした1段ぶんの近傍を切り出す。
 * `MAX_NEIGHBORS`を超える分は畳んで件数だけ返す——全件を一度に展開しない
 * ことが、「有界で中断可能」を実装として保証する。
 */
export function buildNeighborhood(edges: DiscoveryEdge[], focus: string): MathGraphNeighborhood {
  const dependsOnAll: MathGraphNeighborNode[] = [];
  const usedByAll: MathGraphNeighborNode[] = [];
  for (const edge of edges) {
    if (edge.subject === focus && edge.object !== focus) dependsOnAll.push({ label: edge.object, edge });
    else if (edge.object === focus && edge.subject !== focus) usedByAll.push({ label: edge.subject, edge });
  }
  return {
    focus,
    dependsOn: dependsOnAll.slice(0, MAX_NEIGHBORS),
    dependsOnOmitted: Math.max(0, dependsOnAll.length - MAX_NEIGHBORS),
    usedBy: usedByAll.slice(0, MAX_NEIGHBORS),
    usedByOmitted: Math.max(0, usedByAll.length - MAX_NEIGHBORS),
  };
}

/** 全ての辺に現れる宣言ラベルの集合（両端点）——「どれがグラフ表示可能な
 * 焦点候補か」を`mathGraphDiscovery.ts`が判定するのに使う。 */
export function neighborhoodCandidates(edges: DiscoveryEdge[]): Set<string> {
  const set = new Set<string>();
  for (const e of edges) {
    set.add(e.subject);
    set.add(e.object);
  }
  return set;
}

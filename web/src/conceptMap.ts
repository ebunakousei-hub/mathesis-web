import type { RelatedEdge } from "./types";
import type { TypedRelationView } from "./dynamicTaxonomy";

/**
 * 概念の局所的な「地図」（診断⑤への対応）。
 *
 * # なぜ要るか
 *
 * 2026-09-03のアーキテクチャ批評: 「実際の可視化はLineageView1つだけで、
 * 他は`<section>`の縦積み。ただし今グラフを描いても綴りの星座しか
 * 出ない——距離も向きも無い空間の地図は描けないので、①②の後でなければ
 * 意味が無い」。①（文脈ベクトル）と②（型付き関係）が実装された今、
 * 検索結果の各概念について、実際に意味のある**距離**（コサイン類似度、
 * `taxonomy.related.json`）と**向き**（特殊化/同値、
 * `taxonomy.relations.json`）を持つ小さな近傍グラフを描ける。
 *
 * 全概念72,632件を一度に描く「銀河」は意図的に作らない——大半の辺が
 * 描画されないまま重なるだけの「毛玉」になることが視覚化の定石として
 * 知られており、かつ72,632件規模のforce-directedレイアウトは
 * ブラウザで現実的な時間に収まらない。代わりに、検索で辿り着いた
 * 1概念を中心に、その意味的近傍（1〜2ホップ、たかだか数十件）だけを
 * その場でレイアウトする——クリックして中心を移すことで、地図全体を
 * 少しずつ探索できる（Google マップのパン・ズームに近い体験）。
 * MSC分野→クラスタ→概念という既存のドリルダウン（`renderFieldGrid`等）
 * が「広い→狭い」の俯瞰を既に提供しているので、この地図はその末端に
 * 「意味のある実データの詳細」を足す形になる。
 *
 * レイアウトはFruchterman-Reingold（総当たりの反発力＋辺に沿った
 * ばね引力、冷却スケジュール付き反復）。t-SNE/UMAPのような専用の
 * 次元圧縮アルゴリズムは実装しない——ノード数がたかだか数十件の
 * グラフ描画には過剰で、既に持っている「辺（似ている/特殊化）」を
 * そのままばねの強さに使う方が素直だし、依存も増えない。
 */

export type ConceptMapEdgeKind = "neighbor" | "specialization" | "equivalent";

export interface ConceptMapNode {
  phrase: string;
  isFocus: boolean;
  docFreq: number;
  x: number;
  y: number;
}

export interface ConceptMapEdge {
  /** specializationの場合、sourceがtargetの特殊化（source ⊂ target）。 */
  source: string;
  target: string;
  kind: ConceptMapEdgeKind;
  /** ばねの強さ。neighborはコサイン類似度、typed relationは信頼度。 */
  weight: number;
}

export interface ConceptMap {
  nodes: ConceptMapNode[];
  edges: ConceptMapEdge[];
  /** レイアウトが使った論理座標系の一辺（正方形、原点中心）。 */
  extent: number;
}

export interface BuildConceptMapParams {
  focus: string;
  relatedEdges: Record<string, RelatedEdge[]>;
  relations: Record<string, TypedRelationView[]>;
  docFreqOf: (phrase: string) => number;
  /** 1ノードあたり、近傍として取り込む最大件数（次数の上限）。 */
  maxNeighborsPerNode?: number;
  /** グラフ全体のノード数の上限（超えたら遠い方から間引く）。 */
  maxNodes?: number;
}

const DEFAULT_MAX_NEIGHBORS_PER_NODE = 6;
const DEFAULT_MAX_NODES = 36;

/** 決定的な疑似乱数（mulberry32）。同じクエリなら毎回同じ初期配置になる
 * ようにする——再描画のたびにレイアウトが飛ぶと探索の手がかりを失う。 */
function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function hashSeed(text: string): number {
  let h = 2166136261;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

/**
 * `focus`から1〜2ホップの近傍を集め、辺つきグラフを作る（座標はまだ
 * 割り当てない——`layoutForceDirected`が別途行う）。
 */
function collectSubgraph(params: BuildConceptMapParams): { nodes: Set<string>; edges: ConceptMapEdge[] } {
  const maxPerNode = params.maxNeighborsPerNode ?? DEFAULT_MAX_NEIGHBORS_PER_NODE;
  const nodes = new Set<string>([params.focus]);
  const edgeKey = (kind: ConceptMapEdgeKind, a: string, b: string): string => `${kind}:${a < b ? a : b}:${a < b ? b : a}`;
  const edgesByKey = new Map<string, ConceptMapEdge>();

  const addEdge = (source: string, target: string, kind: ConceptMapEdgeKind, weight: number) => {
    if (source === target) return;
    const key = edgeKey(kind, source, target);
    const existing = edgesByKey.get(key);
    if (existing === undefined || weight > existing.weight) {
      edgesByKey.set(key, { source, target, kind, weight });
    }
  };

  const expand = (phrase: string) => {
    const neighbors = (params.relatedEdges[phrase] ?? []).slice(0, maxPerNode);
    for (const n of neighbors) {
      nodes.add(n.phrase);
      addEdge(phrase, n.phrase, "neighbor", n.score);
    }
    for (const rel of params.relations[phrase] ?? []) {
      nodes.add(rel.other);
      if (rel.relation === "equivalent") {
        addEdge(phrase, rel.other, "equivalent", rel.confidence);
      } else if (rel.relation === "broader") {
        // phraseはrel.otherの特殊化（phrase ⊂ other）。
        addEdge(phrase, rel.other, "specialization", rel.confidence);
      } else {
        // rel.relation === "narrower": otherがphraseの特殊化。
        addEdge(rel.other, phrase, "specialization", rel.confidence);
      }
    }
  };

  // 1ホップ: 中心の近傍。
  expand(params.focus);
  // 2ホップ: 1ホップで見つかった各ノードの、さらにその近傍
  // （中心そのものへ戻る辺は上のaddEdgeがsource===targetで弾く）。
  for (const phrase of [...nodes]) {
    if (phrase !== params.focus) expand(phrase);
  }

  const maxNodes = params.maxNodes ?? DEFAULT_MAX_NODES;
  if (nodes.size > maxNodes) {
    // 中心に一番近い辺（重みの大きい順）から優先してノードを残す。
    const keep = new Set<string>([params.focus]);
    const sortedEdges = [...edgesByKey.values()].sort((a, b) => b.weight - a.weight);
    for (const e of sortedEdges) {
      if (keep.size >= maxNodes) break;
      keep.add(e.source);
      keep.add(e.target);
    }
    for (const phrase of nodes) {
      if (!keep.has(phrase)) nodes.delete(phrase);
    }
  }

  const edges = [...edgesByKey.values()].filter((e) => nodes.has(e.source) && nodes.has(e.target));
  return { nodes, edges };
}

/**
 * Fruchterman-Reingold法による2次元配置。全ノード対の反発力＋辺に
 * 沿ったばね引力を、冷却しながら反復する——ノード数が数十件規模の
 * グラフに対する標準的な手法（Gephi・Cytoscape等が使うのと同じ原理）。
 */
function layoutForceDirected(nodeIds: string[], edges: ConceptMapEdge[], seed: number): Map<string, { x: number; y: number }> {
  const n = nodeIds.length;
  const extent = 500;
  const rng = mulberry32(seed);
  const pos = new Map<string, { x: number; y: number }>();
  for (const id of nodeIds) {
    pos.set(id, { x: (rng() - 0.5) * extent, y: (rng() - 0.5) * extent });
  }
  if (n <= 1) return pos;

  const area = extent * extent;
  const k = Math.sqrt(area / n);
  const indexOf = new Map(nodeIds.map((id, i) => [id, i]));
  const disp: { x: number; y: number }[] = nodeIds.map(() => ({ x: 0, y: 0 }));

  const iterations = 300;
  let temperature = extent / 10;
  const cooling = temperature / iterations;

  for (let iter = 0; iter < iterations; iter++) {
    for (const d of disp) {
      d.x = 0;
      d.y = 0;
    }

    // 反発力: 全ノード対（近いほど強く押し合う）。
    for (let i = 0; i < n; i++) {
      const pi = pos.get(nodeIds[i])!;
      for (let j = i + 1; j < n; j++) {
        const pj = pos.get(nodeIds[j])!;
        let dx = pi.x - pj.x;
        let dy = pi.y - pj.y;
        let dist = Math.sqrt(dx * dx + dy * dy);
        if (dist < 0.01) {
          dx = (rng() - 0.5) * 0.1;
          dy = (rng() - 0.5) * 0.1;
          dist = 0.1;
        }
        const force = (k * k) / dist;
        const fx = (dx / dist) * force;
        const fy = (dy / dist) * force;
        disp[i].x += fx;
        disp[i].y += fy;
        disp[j].x -= fx;
        disp[j].y -= fy;
      }
    }

    // 引力: 辺で繋がったノード同士（重みが大きい辺ほど強く引き合う——
    // 似ている/確信度の高い関係ほど近くに描かれてほしいため）。
    for (const e of edges) {
      const si = indexOf.get(e.source);
      const ti = indexOf.get(e.target);
      if (si === undefined || ti === undefined) continue;
      const ps = pos.get(e.source)!;
      const pt = pos.get(e.target)!;
      const dx = ps.x - pt.x;
      const dy = ps.y - pt.y;
      const dist = Math.max(0.01, Math.sqrt(dx * dx + dy * dy));
      const force = (dist * dist) / k / Math.max(0.2, e.weight);
      const fx = (dx / dist) * force;
      const fy = (dy / dist) * force;
      disp[si].x -= fx;
      disp[si].y -= fy;
      disp[ti].x += fx;
      disp[ti].y += fy;
    }

    // 変位を温度で制限しながら適用（冷却スケジュールで徐々に収束させる）。
    for (let i = 0; i < n; i++) {
      const d = disp[i];
      const dist = Math.max(0.01, Math.sqrt(d.x * d.x + d.y * d.y));
      const capped = Math.min(dist, temperature);
      const p = pos.get(nodeIds[i])!;
      p.x += (d.x / dist) * capped;
      p.y += (d.y / dist) * capped;
      p.x = Math.max(-extent, Math.min(extent, p.x));
      p.y = Math.max(-extent, Math.min(extent, p.y));
    }
    temperature = Math.max(1, temperature - cooling);
  }

  return pos;
}

export function buildConceptMap(params: BuildConceptMapParams): ConceptMap {
  const { nodes: nodeSet, edges } = collectSubgraph(params);
  const nodeIds = [...nodeSet];
  const extent = 500;
  const positions = layoutForceDirected(nodeIds, edges, hashSeed(params.focus));

  const nodes: ConceptMapNode[] = nodeIds.map((phrase) => {
    const p = positions.get(phrase) ?? { x: 0, y: 0 };
    return { phrase, isFocus: phrase === params.focus, docFreq: params.docFreqOf(phrase), x: p.x, y: p.y };
  });

  return { nodes, edges, extent };
}

import type { ExportedGraphDependency, ExportedJudgment, ExportedMorphism } from "./types";

/**
 * 「この定理は何に依拠しているのか」を**連鎖として**組み立てる層。
 *
 * これまでこの区画が出せていたのは、判断1件の詳細ページに並ぶ
 * 「依拠 (5)」「利用元 (3)」という**1ホップぶんのチップの列**だけだった。
 * 依存グラフは実際には深さ48段（`holder_Moser` から最下層の定義まで
 * 47本の辺）まで伸びているのに、利用者はそれを1段ずつ手で辿って、
 * 自分の頭の中で繋ぎ直さないと全体像に届かなかった。
 * 「証明が何に依拠するかを簡単に追える」というこのサイトの主眼に対して、
 * 1ホップのチップ列は答えになっていない。
 *
 * ここでやること:
 *
 *   - 根から下へ依存関係の閉包を取り、**段（layer）に割り付ける**。
 *     段は根からの**最長路**で決める（最短路ではない）。最短路で割ると
 *     「Aに直接依拠し、かつBを経由してもAに依拠する」形のとき辺が段を
 *     跨いで水平に走り、絵が読めなくなるため。
 *   - **背骨（spine）**——根から最も深い依存へ至る最長の鎖——を強調する。
 *     証明の骨格はこの鎖で、残りは横から刺さる補題である、という読み方を
 *     絵の側で提示する。
 *   - 同じ段に複数の前提が並ぶときは**横に広げる**（重心法で並べ替え、
 *     交差を減らす）。1列に押し込めない。
 *   - 依存辺（証明が実際に参照した判断）と射（含意・特殊化・一般化・同値
 *     という論理的関係）を**別種の辺として**描き分ける。
 *
 * 描画はしない——DOMを作るのは `lineageView.ts` の仕事で、ここは純粋な
 * グラフ演算だけを持つ（`hybridSearch.ts` / `proofSearch.ts` と同じ方針）。
 */

export interface LineageGraph {
  judgmentById: Map<number, ExportedJudgment>;
  /** 判断id → その証明が参照している判断のid */
  dependsOn: Map<number, number[]>;
  /** 判断id → その判断を参照している判断のid */
  usedBy: Map<number, number[]>;
  /**
   * 判断id → その判断に接続する射。P2（`docs/P2_STATUS.md`）以降、`id`は
   * 証拠層（`mathesis-provenance`）のRelationAssertion idそのもの——
   * 以前は別に`morphismProvenance`という2つ目のidマップを持っていたが、
   * `morphisms.json`自体が証拠層から生成されるようになったので不要になった。
   */
  morphismsOf: Map<number, ExportedMorphism[]>;
  /**
   * P5, Item 1（`docs/P5_PLAN.md`）: 依存辺`"${from}->${to}"` →
   * `traversalPolicy`。`dependsOn`自体は判断idの配列のまま（既存の呼び出し
   * 元を壊さない）にして、信頼度はこの並行マップで引く。射は
   * `ExportedMorphism.traversalPolicy`を直接持っているので別マップは不要。
   */
  dependencyPolicy: Map<string, ExportedGraphDependency["traversalPolicy"]>;
  /**
   * P6.1（`docs/LEAN_DEPENDENCY_POLICY.md`）: 依存辺`"${from}->${to}"` →
   * そのassertion id。`showAssertionDetail`（`provenancePanel.ts`）へ渡して
   * `assertions.json`の詳細（checker-derivedならevidenceの
   * `dependencyOrigin`/`formalRevision`まで）を開けるようにする——
   * 射のチップが`data-assertion-id`で同じことをしているのと同じ理由。
   */
  dependencyAssertionId: Map<string, number>;
  /** P6.1: 依存辺`"${from}->${to}"` → `"checker-derived"`|`"text-extracted"`。 */
  dependencyOrigin: Map<string, ExportedGraphDependency["origin"]>;
}

export type LineageRelation =
  | "dependency"
  | "implication"
  | "specialization"
  | "generalization"
  | "equivalence";

export interface LineageNode {
  id: number;
  judgment: ExportedJudgment;
  /** 根を0とする段。負の値は「この定理を使っている側」（根より上）。 */
  depth: number;
  /** 背骨（最長の依存鎖）の上にあるか。 */
  onSpine: boolean;
  /** 部分グラフに載せきれず省いた前提の数。0なら全部見えている。 */
  hiddenDeps: number;
  x: number;
  y: number;
}

export interface LineageEdge {
  from: number;
  to: number;
  relation: LineageRelation;
  onSpine: boolean;
  /** 射のときだけ。ヒューリスティック提案である旨を出すのに使う。 */
  morphism?: ExportedMorphism;
  /** P5, Item 1: この辺自身の信頼度。`lineageView.ts`が視覚的に区別する。 */
  traversalPolicy: ExportedGraphDependency["traversalPolicy"];
}

export interface Lineage {
  rootId: number;
  nodes: LineageNode[];
  edges: LineageEdge[];
  /** 根 → 最深部 の順に並んだ、最長の依存鎖。 */
  spine: number[];
  /** 実際の依存閉包の全長（省略前）。「48段のうち4段を表示」と言うため。 */
  fullDepth: number;
  /** 深さ・幅の制限で描かれなかった判断の数。 */
  omitted: number;
  width: number;
  height: number;
}

export interface LineageOptions {
  /** 根から下へ何段まで描くか。 */
  maxDepth: number;
  /** 1段に並べる最大件数。超えた分は `hiddenDeps` に畳む。 */
  maxPerLayer: number;
  /** 根の上に何件「これを使っている定理」を出すか。 */
  maxUsedBy: number;
  /** 射を辺として描くか。 */
  showMorphisms: boolean;
  /**
   * P5, Item 1（`docs/P5_PLAN.md`、ARCHITECTURE_NEXT.md §7）: trueなら
   * `traversalPolicy === "default_traversal"`の辺だけを辿る——既定は
   * false（今までどおり全辺を表示、回帰を起こさない）。実データでは
   * まだ`default_traversal`の辺が1本も無い（`depends_on`は全件`extracted`、
   * 射は全件レビュー未実施の`proposed`）ため、trueにすると意図的に
   * ほぼ空の系譜になる——`docs/P5_STATUS.md`が実測値として記録している、
   * バグではなく現在のデータの実情。
   */
  trustedOnly: boolean;
}

export const DEFAULT_LINEAGE_OPTIONS: LineageOptions = {
  maxDepth: 4,
  maxPerLayer: 6,
  maxUsedBy: 4,
  showMorphisms: true,
  trustedOnly: false,
};

/** `LineageGraph.dependencyPolicy`のキー形式。呼び出し元をここに揃える。 */
export function dependencyKey(from: number, to: number): string {
  return `${from}->${to}`;
}

function isTrusted(policy: ExportedGraphDependency["traversalPolicy"]): boolean {
  return policy === "default_traversal";
}

/**
 * `trustedOnly`が立っているときだけ絞り込む依存先一覧。図の組み立て
 * （`findSpine`/`collectSubgraph`/`buildLineage`）だけでなく、
 * `lineageView.ts`の判断詳細（「依拠 (N)」チップの列）からも呼ばれる
 * ——図で隠した辺が詳細欄には残る、という食い違いを避けるため export する。
 */
export function traversableChildren(graph: LineageGraph, id: number, opts: LineageOptions): number[] {
  const children = graph.dependsOn.get(id) ?? [];
  if (!opts.trustedOnly) return children;
  return children.filter((c) => isTrusted(graph.dependencyPolicy.get(dependencyKey(id, c)) ?? "visible_only"));
}

/** 同じく`usedBy`版（辺の向きは`other -> id`なので鍵の引き方が逆になる）。 */
export function traversableUsedBy(graph: LineageGraph, id: number, opts: LineageOptions): number[] {
  const users = graph.usedBy.get(id) ?? [];
  if (!opts.trustedOnly) return users;
  return users.filter((other) => isTrusted(graph.dependencyPolicy.get(dependencyKey(other, id)) ?? "visible_only"));
}

/** 配置の寸法。`lineageView.ts` のCSSと合わせてある。 */
export const NODE_W = 186;
export const NODE_H = 56;
const H_GAP = 20;
const V_GAP = 66;
const PAD_X = 14;
const PAD_Y = 14;

/**
 * 各判断から下へ伸びる依存鎖の最長の長さ。
 *
 * 「背骨」を決めるのに要る。1,431件・4,797辺に対して1度だけ計算して
 * 使い回す（根を変えるたびに全体を計算し直す必要は無い——この値は
 * 根に依らないため）。依存グラフは非巡回であることを実データで確認済み
 * だが、万一の巡回でも無限再帰しないよう訪問中の印を持って打ち切る。
 */
export function computeChainDepths(dependsOn: Map<number, number[]>, ids: Iterable<number>): Map<number, number> {
  const depth = new Map<number, number>();
  const visiting = new Set<number>();

  const walk = (id: number): number => {
    const cached = depth.get(id);
    if (cached !== undefined) return cached;
    if (visiting.has(id)) return 0; // 巡回していたら0で打ち切る
    visiting.add(id);
    let best = 0;
    for (const child of dependsOn.get(id) ?? []) {
      const d = walk(child) + 1;
      if (d > best) best = d;
    }
    visiting.delete(id);
    depth.set(id, best);
    return best;
  };

  for (const id of ids) walk(id);
  return depth;
}

/**
 * 根から最深部へ至る最長の鎖。各段で「そこから最も深く伸びる子」を選ぶ。
 * 同じ深さの子が複数あるときは依存の多いほうを採る（枝葉より、証明の
 * 本筋になっている補題が選ばれやすいように）。
 */
function findSpine(rootId: number, graph: LineageGraph, chainDepth: Map<number, number>, opts: LineageOptions): number[] {
  const spine = [rootId];
  const seen = new Set([rootId]);
  let current = rootId;
  for (;;) {
    const children = traversableChildren(graph, current, opts).filter((c) => !seen.has(c));
    if (children.length === 0) break;
    let best = children[0];
    let bestKey = -1;
    for (const c of children) {
      // `chainDepth`は全辺込みで一度だけ計算した値（下のコメント参照）——
      // `trustedOnly`時は同点付近の優先順位が全辺基準でわずかにずれうるが、
      // 候補自体は`traversableChildren`で既に絞られているので選ばれる
      // ノードそのものは正しい。
      const key = (chainDepth.get(c) ?? 0) * 1000 + (graph.dependsOn.get(c)?.length ?? 0);
      if (key > bestKey) {
        bestKey = key;
        best = c;
      }
    }
    spine.push(best);
    seen.add(best);
    current = best;
  }
  return spine;
}

/**
 * 部分グラフを集める。段ごとに幅を制限しつつ、**背骨の上のノードは必ず
 * 残す**——幅の制限で証明の本筋が消えては本末転倒なので、背骨を最優先で
 * 席に着かせてから残りを埋める。
 */
function collectSubgraph(
  rootId: number,
  graph: LineageGraph,
  spine: number[],
  opts: LineageOptions,
): { layers: number[][]; hidden: Map<number, number>; omitted: number } {
  const spineSet = new Set(spine);
  const layers: number[][] = [];
  const placed = new Map<number, number>(); // id → depth
  const hidden = new Map<number, number>();
  let omitted = 0;

  placed.set(rootId, 0);
  layers.push([rootId]);

  for (let d = 1; d <= opts.maxDepth; d += 1) {
    const parents = layers[d - 1];
    // 親の並び順を保ったまま候補を集める（同じ子が複数の親から来ることが
    // あるので重複を落とす）。背骨の子は先頭に寄せる。
    const candidates: number[] = [];
    const seen = new Set<number>();
    for (const p of parents) {
      for (const c of traversableChildren(graph, p, opts)) {
        if (placed.has(c) || seen.has(c)) continue;
        seen.add(c);
        candidates.push(c);
      }
    }
    if (candidates.length === 0) break;

    candidates.sort((a, b) => {
      const sa = spineSet.has(a) ? 1 : 0;
      const sb = spineSet.has(b) ? 1 : 0;
      if (sa !== sb) return sb - sa;
      // 背骨以外は、さらに下へ伸びる（＝証明の骨に近い）ものを優先。
      return (graph.dependsOn.get(b)?.length ?? 0) - (graph.dependsOn.get(a)?.length ?? 0);
    });

    const keep = candidates.slice(0, opts.maxPerLayer);
    omitted += candidates.length - keep.length;
    for (const c of keep) placed.set(c, d);
    layers.push(keep);
  }

  // 最下段に残った「まだ下がある」ノードに、見えていない前提の数を持たせる。
  for (const [id, d] of placed) {
    const deps = graph.dependsOn.get(id) ?? [];
    const unseen = deps.filter((c) => !placed.has(c)).length;
    if (unseen > 0) {
      hidden.set(id, unseen);
      if (d === opts.maxDepth) omitted += unseen;
    }
  }

  return { layers, hidden, omitted };
}

/**
 * 各段で背骨のノードを先頭（左端）へ持ってくる。
 *
 * 重心法だけに任せると背骨が段ごとに左右へ振れて、絵の中で一番大事な
 * 「証明の本筋」が目で追えなくなる。背骨を左端に固定し、そこから
 * 枝の補題を右へ広げると、縦一本の線を下へ辿りながら、各段で
 * 「ここでは他にこれとこれが要る」を横に読む形になる。
 */
function pinSpineFirst(layers: number[][], spine: number[]): void {
  const spineSet = new Set(spine);
  for (const layer of layers) {
    const at = layer.findIndex((id) => spineSet.has(id));
    if (at <= 0) continue;
    const [node] = layer.splice(at, 1);
    layer.unshift(node);
  }
}

/** 重心法。各段のノードを、親（または子）の平均位置の順に並べ替える。 */
function orderLayers(layers: number[][], adjacency: Map<number, number[]>, reverse: boolean): void {
  const indexIn = (layer: number[]): Map<number, number> => {
    const m = new Map<number, number>();
    layer.forEach((id, i) => m.set(id, i));
    return m;
  };
  const range = reverse
    ? [...layers.keys()].slice(0, -1).reverse()
    : [...layers.keys()].slice(1);

  for (const li of range) {
    const fixed = indexIn(layers[reverse ? li + 1 : li - 1]);
    const scored = layers[li].map((id, i) => {
      const neighbors = (adjacency.get(id) ?? []).map((n) => fixed.get(n)).filter((v): v is number => v !== undefined);
      const bary = neighbors.length === 0 ? i : neighbors.reduce((a, b) => a + b, 0) / neighbors.length;
      return { id, bary, i };
    });
    scored.sort((a, b) => (a.bary === b.bary ? a.i - b.i : a.bary - b.bary));
    layers[li] = scored.map((s) => s.id);
  }
}

/**
 * x座標を決める。
 *
 * 段の中は等間隔に詰めたまま、**段ごと剛体でずらして**親（または子）の
 * 位置に重ねる。ノードを1件ずつ希望位置へ寄せて最小間隔で押し合う
 * やり方も試したが、段の左端のノードだけが親の位置に貼り付いて残りが
 * そこから右へ押し出されるため、段が下がるごとに絵全体が左右へ流れて
 * 階段状になった（実データの `holder_Moser` で、26件を描くのに
 * 2,040pxの幅を使い、うち800pxが空白という状態になっていた）。
 * 段をまとめて動かせば段内の間隔が崩れず、親子の重なりだけが最適化
 * されるので、横幅も読みやすさも素直に収まる。
 *
 * 上下に何度か往復するのは、段どうしを繋いで全体で揃えるため——
 * 下向きだけだと最下段が根の位置を引き戻せない。
 */
function assignX(
  layers: number[][],
  parentsOf: Map<number, number[]>,
  childrenOf: Map<number, number[]>,
  spine: number[],
): Map<number, number> {
  const x = new Map<number, number>();
  const step = NODE_W + H_GAP;
  for (const layer of layers) {
    layer.forEach((id, i) => x.set(id, i * step));
  }

  const shiftLayer = (layer: number[], adj: Map<number, number[]>): void => {
    let total = 0;
    let count = 0;
    for (const id of layer) {
      const near = (adj.get(id) ?? []).map((n) => x.get(n)).filter((v): v is number => v !== undefined);
      if (near.length === 0) continue;
      total += near.reduce((a, b) => a + b, 0) / near.length - (x.get(id) ?? 0);
      count += 1;
    }
    if (count === 0) return;
    const delta = total / count;
    for (const id of layer) x.set(id, (x.get(id) ?? 0) + delta);
  };

  for (let pass = 0; pass < 3; pass += 1) {
    for (let li = 1; li < layers.length; li += 1) shiftLayer(layers[li], parentsOf);
    for (let li = layers.length - 2; li >= 0; li -= 1) shiftLayer(layers[li], childrenOf);
  }

  // 背骨が通っている段は、背骨のノードが同じx座標に来るように段ごとずらす。
  // `pinSpineFirst` で背骨は各段の左端に来ているので、これで縦一本の線に
  // 揃い、枝の補題はその右へ広がる。背骨のノードが載っていない段
  // （根の上の「これを使っている定理」など）は、上の重心合わせのまま残す。
  const spineSet = new Set(spine);
  let spineX: number | null = null;
  for (const layer of layers) {
    const node = layer.find((id) => spineSet.has(id));
    if (node === undefined) continue;
    const at = x.get(node) ?? 0;
    if (spineX === null) spineX = at;
    else {
      const delta = spineX - at;
      for (const id of layer) x.set(id, (x.get(id) ?? 0) + delta);
    }
  }

  const min = Math.min(...[...x.values()]);
  for (const [id, v] of x) x.set(id, Math.round(v - min + PAD_X));
  return x;
}

/**
 * 根を1件受け取って、描ける形の系譜を返す。
 *
 * @param chainDepth `computeChainDepths` の結果（使い回す）
 */
export function buildLineage(
  rootId: number,
  graph: LineageGraph,
  chainDepth: Map<number, number>,
  opts: LineageOptions = DEFAULT_LINEAGE_OPTIONS,
): Lineage | null {
  const root = graph.judgmentById.get(rootId);
  if (root === undefined) return null;

  const spine = findSpine(rootId, graph, chainDepth, opts);
  const { layers, hidden, omitted } = collectSubgraph(rootId, graph, spine, opts);

  // 根の「上」——この定理を使っている側。深さ -1 の段として先頭に足す。
  const above = traversableUsedBy(graph, rootId, opts).slice(0, opts.maxUsedBy);
  const allLayers = above.length > 0 ? [above, ...layers] : layers;
  const baseDepth = above.length > 0 ? -1 : 0;

  const inGraph = new Set<number>();
  for (const layer of allLayers) for (const id of layer) inGraph.add(id);

  // 段の間の辺（依存関係）を集める。
  const spineSet = new Set(spine);
  const edges: LineageEdge[] = [];
  const parentsOf = new Map<number, number[]>();
  const childrenOf = new Map<number, number[]>();
  const link = (
    from: number,
    to: number,
    relation: LineageRelation,
    traversalPolicy: ExportedGraphDependency["traversalPolicy"],
    morphism?: ExportedMorphism,
  ): void => {
    const onSpine =
      relation === "dependency" &&
      spineSet.has(from) &&
      spineSet.has(to) &&
      spine.indexOf(to) === spine.indexOf(from) + 1;
    edges.push({ from, to, relation, onSpine, morphism, traversalPolicy });
    childrenOf.set(from, [...(childrenOf.get(from) ?? []), to]);
    parentsOf.set(to, [...(parentsOf.get(to) ?? []), from]);
  };

  for (const id of inGraph) {
    for (const child of graph.dependsOn.get(id) ?? []) {
      if (!inGraph.has(child)) continue;
      const policy = graph.dependencyPolicy.get(dependencyKey(id, child)) ?? "visible_only";
      if (opts.trustedOnly && !isTrusted(policy)) continue;
      link(id, child, "dependency", policy);
    }
  }

  if (opts.showMorphisms) {
    const seenMorphism = new Set<number>();
    for (const id of inGraph) {
      for (const m of graph.morphismsOf.get(id) ?? []) {
        if (seenMorphism.has(m.id)) continue;
        if (!inGraph.has(m.src) || !inGraph.has(m.dst)) continue;
        if (m.src === m.dst) continue;
        if (opts.trustedOnly && !isTrusted(m.traversalPolicy)) continue;
        seenMorphism.add(m.id);
        edges.push({ from: m.src, to: m.dst, relation: m.kind, onSpine: false, morphism: m, traversalPolicy: m.traversalPolicy });
      }
    }
  }

  orderLayers(allLayers, parentsOf, false);
  orderLayers(allLayers, childrenOf, true);
  orderLayers(allLayers, parentsOf, false);
  pinSpineFirst(allLayers, spine);
  const x = assignX(allLayers, parentsOf, childrenOf, spine);

  const nodes: LineageNode[] = [];
  allLayers.forEach((layer, li) => {
    const depth = baseDepth + li;
    for (const id of layer) {
      const judgment = graph.judgmentById.get(id);
      if (judgment === undefined) continue;
      nodes.push({
        id,
        judgment,
        depth,
        onSpine: spineSet.has(id),
        hiddenDeps: hidden.get(id) ?? 0,
        x: x.get(id) ?? PAD_X,
        y: PAD_Y + li * (NODE_H + V_GAP),
      });
    }
  });

  const width = Math.max(...nodes.map((n) => n.x + NODE_W)) + PAD_X;
  const height = PAD_Y * 2 + allLayers.length * NODE_H + (allLayers.length - 1) * V_GAP;

  return {
    rootId,
    nodes,
    edges,
    spine,
    fullDepth: chainDepth.get(rootId) ?? 0,
    omitted,
    width,
    height,
  };
}

/** 証明の概略の1段。背骨を上から読み下したもの。 */
export interface OutlineStep {
  id: number;
  judgment: ExportedJudgment;
  /**
   * 背骨の次の段。この段が主に依拠する判断——「主に」の判定は
   * `findSpine` と同じで、そこから下へ最も長く鎖が伸びるものを指す。
   * 鎖の底では `null`。
   */
  next: ExportedJudgment | null;
  /** 背骨から外れて横から刺さる補題（次の段は含まない）。 */
  side: ExportedJudgment[];
  /** 横から刺さる補題の総数（`side` は先頭数件だけ）。 */
  sideCount: number;
}

export interface Outline {
  steps: OutlineStep[];
  /** 表示件数の上限で切り落とした段数。0なら鎖を最後まで出している。 */
  remaining: number;
  /** 鎖の全長（段数）。 */
  total: number;
}

/**
 * 背骨を「証明の概略」として読み下す。
 *
 * 絵は全体の形を掴むのに向くが、順を追って読むには向かない。同じ連鎖を
 * 1. 2. 3. と番号を振った文章の列としても出す——数学者が証明を読むときの
 * 自然な形はこちらなので、絵と文章の両方から同じ構造に入れるようにする。
 */
export function spineOutline(lineage: Lineage, graph: LineageGraph, opts: LineageOptions = DEFAULT_LINEAGE_OPTIONS, limit = 12): Outline {
  const steps: OutlineStep[] = [];
  const spine = lineage.spine;
  for (let i = 0; i < Math.min(limit, spine.length); i += 1) {
    const id = spine[i];
    const judgment = graph.judgmentById.get(id);
    if (judgment === undefined) continue;
    const nextId = spine[i + 1];
    // `trustedOnly`のときは図と同じ辺だけを「横から刺さる補題」として
    // 数える——図では隠したのに文章では出る、という食い違いを避ける。
    const deps = traversableChildren(graph, id, opts);
    // 「横から刺さる補題」は、背骨の次の段以外の依存すべて。番号を追って
    // 読んでいる人にとって、この段で追加で要るものはこれ、という意味。
    const side = deps
      .filter((d) => d !== nextId)
      .map((d) => graph.judgmentById.get(d))
      .filter((j): j is ExportedJudgment => j !== undefined);
    steps.push({
      id,
      judgment,
      next: nextId === undefined ? null : (graph.judgmentById.get(nextId) ?? null),
      side: side.slice(0, 6),
      sideCount: side.length,
    });
  }
  return { steps, remaining: Math.max(0, spine.length - steps.length), total: spine.length };
}

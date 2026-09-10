/**
 * P8.3（`docs/P8_3_STATUS.md`）: `mathGraphLineage.ts`が組む1段の局所近傍を
 * DOM/SVGにする。`lineageView.ts`（Mathesis自身の判断専用、`LineageView`は
 * 自前の状態と自前のDOMルートを持つクラス）とは意図的に別コンポーネント・
 * 別CSS名前空間（`mgl-*`）にした上で、状態を持たない純粋な描画関数として
 * 書く——`MathGraphDiscoveryPanel`自体が「状態→`render()`で丸ごと再構築」
 * という流儀（`dynamicTaxonomy.ts`と同じ）を既に採っているため、ここに
 * 別ライフサイクルの子コンポーネントを混ぜるより、状態を親
 * （`MathGraphDiscoveryPanel`）に持たせてこちらは毎回呼び出される描画関数に
 * したほうが素直で壊れにくい。
 *
 * ディレクティブの「Typeclass-hierarchy records should be represented as
 * structural external nodes, not as ordinary Lean declaration nodes」を
 * そのまま実装: 見た目は近い系統でも同じCSSクラスは一切使わず、破線の枠と
 * 由来バッジで常に外部データであることを示す。
 */
import { SOURCE_BADGE_CLASS, SOURCE_LABEL, STRUCTURAL_CANDIDATE_CAVEAT } from "./mathGraphDiscovery";
import { buildNeighborhood, type MathGraphNeighborNode, type MathGraphNeighborhood } from "./mathGraphLineage";
import type { DiscoveryEdge } from "./types";
import { escapeHtml } from "./util";

const SVG_NS = "http://www.w3.org/2000/svg";
const NODE_W = 168;
const NODE_H = 44;
const H_GAP = 14;
const ROW_GAP = 58;
const PAD = 14;

/** ラベルは`"declName (module)"`形式——表示は宣言名だけに切り詰め、
 * モジュール名はtitleツールチップへ回す。 */
function shortLabel(label: string): string {
  const i = label.indexOf(" (");
  return i === -1 ? label : label.slice(0, i);
}

export interface MathGraphLineageState {
  repoSlug: string;
  focus: string;
  detailFor: string | null;
}

export interface MathGraphLineageHandlers {
  /** 隣接ノード本体のクリック——そのノードへ再root。 */
  onFocus: (label: string) => void;
  /** 隣接ノードの「ⓘ」——辺の由来を開閉（再rootしない）。 */
  onToggleDetail: (label: string) => void;
  onClose: () => void;
}

/** `edges`は焦点と同じ`repoSlug`グループぶんに既に絞り込み済みのもの。 */
export function renderMathGraphLineage(
  edges: DiscoveryEdge[],
  state: MathGraphLineageState,
  handlers: MathGraphLineageHandlers,
): HTMLElement {
  const n = buildNeighborhood(edges, state.focus);

  const wrap = document.createElement("div");
  wrap.className = "mgl";

  const head = document.createElement("div");
  head.className = "mgl-head";
  const title = document.createElement("span");
  title.className = "mgl-title";
  title.textContent = `Local dependency graph — ${state.repoSlug} (external, Math-Graph — not independently verified by Mathesis)`;
  head.appendChild(title);
  const closeBtn = document.createElement("button");
  closeBtn.type = "button";
  closeBtn.className = "mgl-close";
  closeBtn.textContent = "×";
  closeBtn.setAttribute("aria-label", "Close graph view");
  closeBtn.onclick = handlers.onClose;
  head.appendChild(closeBtn);
  wrap.appendChild(head);

  wrap.appendChild(renderCanvas(n, state, handlers));
  return wrap;
}

function renderCanvas(n: MathGraphNeighborhood, state: MathGraphLineageState, handlers: MathGraphLineageHandlers): HTMLElement {
  const viewport = document.createElement("div");
  viewport.className = "mgl-viewport";

  const rowCount = Math.max(n.usedBy.length, 1, n.dependsOn.length);
  const width = PAD * 2 + rowCount * (NODE_W + H_GAP) - H_GAP;
  const usedByRow = n.usedBy.length > 0 ? 1 : 0;
  const dependsOnRow = n.dependsOn.length > 0 ? 1 : 0;
  const height = PAD * 2 + NODE_H + usedByRow * (NODE_H + ROW_GAP) + dependsOnRow * (NODE_H + ROW_GAP);

  const canvas = document.createElement("div");
  canvas.className = "mgl-canvas";
  canvas.style.width = `${width}px`;
  canvas.style.height = `${height}px`;

  const focusY = PAD + usedByRow * (NODE_H + ROW_GAP);
  const focusX = width / 2 - NODE_W / 2;
  const usedByY = PAD;
  const dependsOnY = focusY + NODE_H + ROW_GAP;
  const usedByX = rowX(n.usedBy.length, width);
  const dependsOnX = rowX(n.dependsOn.length, width);

  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("class", "mgl-edges");
  svg.setAttribute("width", String(width));
  svg.setAttribute("height", String(height));
  svg.setAttribute("aria-hidden", "true");
  n.usedBy.forEach((_, i) => svg.appendChild(edgePath(usedByX[i] + NODE_W / 2, usedByY + NODE_H, focusX + NODE_W / 2, focusY)));
  n.dependsOn.forEach((_, i) => svg.appendChild(edgePath(focusX + NODE_W / 2, focusY + NODE_H, dependsOnX[i] + NODE_W / 2, dependsOnY)));
  canvas.appendChild(svg);

  n.usedBy.forEach((neighbor, i) => canvas.appendChild(renderNeighbor(neighbor, usedByX[i], usedByY, state, handlers)));
  if (n.usedByOmitted > 0) canvas.appendChild(renderOmittedNote(n.usedByOmitted, usedByX, usedByY));

  canvas.appendChild(renderFocus(n.focus, focusX, focusY));

  n.dependsOn.forEach((neighbor, i) => canvas.appendChild(renderNeighbor(neighbor, dependsOnX[i], dependsOnY, state, handlers)));
  if (n.dependsOnOmitted > 0) canvas.appendChild(renderOmittedNote(n.dependsOnOmitted, dependsOnX, dependsOnY));

  viewport.appendChild(canvas);

  if (state.detailFor !== null) {
    const edge = [...n.usedBy, ...n.dependsOn].find((x) => x.label === state.detailFor)?.edge;
    if (edge) viewport.appendChild(renderDetail(edge));
  }

  return viewport;
}

function rowX(count: number, width: number): number[] {
  if (count === 0) return [];
  const rowWidth = count * NODE_W + (count - 1) * H_GAP;
  const start = width / 2 - rowWidth / 2;
  return Array.from({ length: count }, (_, i) => start + i * (NODE_W + H_GAP));
}

function edgePath(x1: number, y1: number, x2: number, y2: number): SVGPathElement {
  const path = document.createElementNS(SVG_NS, "path");
  const bend = Math.max(14, (y2 - y1) * 0.4);
  path.setAttribute("d", `M${x1},${y1} C${x1},${y1 + bend} ${x2},${y2 - bend} ${x2},${y2}`);
  path.setAttribute("class", "mgl-edge");
  return path;
}

function renderFocus(label: string, x: number, y: number): HTMLElement {
  const el = document.createElement("div");
  el.className = "mgl-node mgl-node-focus";
  el.style.left = `${x}px`;
  el.style.top = `${y}px`;
  el.style.width = `${NODE_W}px`;
  el.style.height = `${NODE_H}px`;
  el.title = label;
  el.innerHTML = `<span class="mgl-node-name">${escapeHtml(shortLabel(label))}</span>`;
  return el;
}

function renderNeighbor(
  neighbor: MathGraphNeighborNode,
  x: number,
  y: number,
  state: MathGraphLineageState,
  handlers: MathGraphLineageHandlers,
): HTMLElement {
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = `mgl-node mgl-node-neighbor ${state.detailFor === neighbor.label ? "is-detail-open" : ""}`;
  btn.style.left = `${x}px`;
  btn.style.top = `${y}px`;
  btn.style.width = `${NODE_W}px`;
  btn.style.height = `${NODE_H}px`;
  btn.title = neighbor.label;
  const badgeClass = SOURCE_BADGE_CLASS[neighbor.edge.source];
  btn.innerHTML = `
    <span class="mgl-node-name">${escapeHtml(shortLabel(neighbor.label))}</span>
    <span class="mgl-node-meta">
      <span class="mgl-node-badge ${badgeClass}"></span>
      <span class="mgl-node-info" data-info="1" title="Why does this edge exist?">ⓘ</span>
    </span>
  `;
  btn.onclick = (ev) => {
    const target = ev.target as HTMLElement;
    if (target.closest(".mgl-node-info")) {
      ev.stopPropagation();
      handlers.onToggleDetail(neighbor.label);
      return;
    }
    handlers.onFocus(neighbor.label);
  };
  return btn;
}

function renderOmittedNote(count: number, xs: number[], y: number): HTMLElement {
  const el = document.createElement("div");
  el.className = "mgl-omitted";
  const x = xs.length > 0 ? xs[xs.length - 1] + NODE_W + 8 : PAD;
  el.style.left = `${x}px`;
  el.style.top = `${y}px`;
  el.textContent = `+${count} more`;
  return el;
}

function renderDetail(edge: DiscoveryEdge): HTMLElement {
  const box = document.createElement("div");
  box.className = "mgl-detail";
  box.innerHTML = `
    <span class="mgd-badge ${SOURCE_BADGE_CLASS[edge.source]}">${escapeHtml(SOURCE_LABEL[edge.source])}</span>
    <div class="mgl-detail-meta">
      epistemic state: <b>${escapeHtml(edge.epistemicState)}</b> ·
      traversal: <b>${escapeHtml(edge.traversalPolicy)}</b>
      ${edge.edgeType ? ` · edge type: ${escapeHtml(edge.edgeType)}` : ""}
      ${edge.license ? ` · license: ${escapeHtml(edge.license)}` : ""}
    </div>
    ${edge.source === "math-graph-structural-candidate" ? `<div class="mgl-detail-caveat">${escapeHtml(STRUCTURAL_CANDIDATE_CAVEAT)}</div>` : ""}
    ${edge.locator ? `<div class="mgl-detail-locator">${escapeHtml(edge.locator)}</div>` : ""}
  `;
  return box;
}

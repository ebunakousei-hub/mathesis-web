import type { ConceptMap, ConceptMapEdge, ConceptMapNode } from "./conceptMap";
import { t } from "./i18n";
import { escapeHtml } from "./util";

/**
 * `conceptMap.ts`が計算した局所地図をSVGで描く。マーカー
 * （矢じり）の作り方は`lineageView.ts`の既存パターン
 * （`createElementNS`＋`<defs><marker>`）をそのまま踏襲し、色も
 * `--lin-special`/`--lin-equiv`という既存のLineageView用トークンを
 * 再利用する——この地図が「別のアプリ」に見えないようにするため。
 */

const SVG_NS = "http://www.w3.org/2000/svg";

const MARGIN = 60;
const MIN_RADIUS = 5;
const MAX_RADIUS = 16;
const FOCUS_RADIUS = 20;

function radiusFor(docFreq: number, isFocus: boolean): number {
  if (isFocus) return FOCUS_RADIUS;
  // 対数スケール——文書頻度は桁で違うことが普通なので、線形だと
  // 大半のノードが点になってしまう。
  const scaled = Math.log10(1 + docFreq) * 5;
  return Math.max(MIN_RADIUS, Math.min(MAX_RADIUS, scaled));
}

function edgeColorVar(kind: ConceptMapEdge["kind"]): string {
  switch (kind) {
    case "specialization":
      return "var(--lin-special)";
    case "equivalent":
      return "var(--lin-equiv)";
    default:
      return "var(--lin-dep)";
  }
}

function addArrowMarker(defs: SVGDefsElement, id: string, color: string): void {
  const marker = document.createElementNS(SVG_NS, "marker");
  marker.setAttribute("id", id);
  marker.setAttribute("viewBox", "0 0 8 8");
  marker.setAttribute("refX", "7");
  marker.setAttribute("refY", "4");
  marker.setAttribute("markerWidth", "6");
  marker.setAttribute("markerHeight", "6");
  marker.setAttribute("orient", "auto-start-reverse");
  const path = document.createElementNS(SVG_NS, "path");
  path.setAttribute("d", "M0,0 L8,4 L0,8 Z");
  path.setAttribute("fill", color);
  marker.appendChild(path);
  defs.appendChild(marker);
}

/**
 * `map`を描画する。`onRecenter`は中心以外のノードをクリックしたときに
 * 呼ばれ、呼び出し側がそのノードを新しい中心として地図を作り直す
 * ——ページ全体を作り直すGoogleマップのパン操作に近い、少しずつ
 * 探索していく体験にするため。
 */
export function renderConceptMap(map: ConceptMap, onRecenter: (phrase: string) => void): HTMLElement {
  const wrap = document.createElement("div");
  wrap.className = "dt-concept-map";

  const half = map.extent + MARGIN;
  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("class", "dt-concept-map-svg");
  svg.setAttribute("viewBox", `${-half} ${-half} ${half * 2} ${half * 2}`);
  svg.setAttribute("preserveAspectRatio", "xMidYMid meet");
  svg.setAttribute("role", "img");
  svg.setAttribute("aria-label", t("conceptMapAriaLabel"));

  const defs = document.createElementNS(SVG_NS, "defs") as SVGDefsElement;
  addArrowMarker(defs, "dt-map-arrow-special", "var(--lin-special)");
  svg.appendChild(defs);

  const nodeByPhrase = new Map<string, ConceptMapNode>(map.nodes.map((n) => [n.phrase, n]));

  const edgeGroup = document.createElementNS(SVG_NS, "g");
  for (const edge of map.edges) {
    const from = nodeByPhrase.get(edge.source);
    const to = nodeByPhrase.get(edge.target);
    if (from === undefined || to === undefined) continue;
    const line = document.createElementNS(SVG_NS, "line");
    line.setAttribute("x1", String(from.x));
    line.setAttribute("y1", String(from.y));
    line.setAttribute("x2", String(to.x));
    line.setAttribute("y2", String(to.y));
    line.setAttribute("stroke", edgeColorVar(edge.kind));
    line.setAttribute("stroke-width", edge.kind === "neighbor" ? "1" : "2");
    if (edge.kind === "equivalent") line.setAttribute("stroke-dasharray", "4 3");
    if (edge.kind === "specialization") line.setAttribute("marker-end", "url(#dt-map-arrow-special)");
    line.setAttribute("opacity", edge.kind === "neighbor" ? "0.45" : "0.85");
    edgeGroup.appendChild(line);
  }
  svg.appendChild(edgeGroup);

  const nodeGroup = document.createElementNS(SVG_NS, "g");
  for (const node of map.nodes) {
    const g = document.createElementNS(SVG_NS, "g");
    g.setAttribute("class", node.isFocus ? "dt-map-node dt-map-node-focus" : "dt-map-node");
    g.setAttribute("transform", `translate(${node.x}, ${node.y})`);
    if (!node.isFocus) {
      g.style.cursor = "pointer";
      g.addEventListener("click", () => onRecenter(node.phrase));
    }

    const circle = document.createElementNS(SVG_NS, "circle");
    circle.setAttribute("r", String(radiusFor(node.docFreq, node.isFocus)));
    g.appendChild(circle);

    const title = document.createElementNS(SVG_NS, "title");
    title.textContent = `${node.phrase} (docFreq=${node.docFreq})`;
    g.appendChild(title);

    const label = document.createElementNS(SVG_NS, "text");
    label.setAttribute("y", String(radiusFor(node.docFreq, node.isFocus) + 14));
    label.setAttribute("text-anchor", "middle");
    label.textContent = node.phrase;
    g.appendChild(label);

    nodeGroup.appendChild(g);
  }
  svg.appendChild(nodeGroup);

  wrap.appendChild(svg);

  const legend = document.createElement("div");
  legend.className = "dt-concept-map-legend";
  legend.innerHTML = `
    <span><i class="dt-map-swatch" style="background:var(--accent)"></i>${escapeHtml(t("mapLegendFocus"))}</span>
    <span><i class="dt-map-swatch dt-map-swatch-line" style="background:var(--lin-dep)"></i>${escapeHtml(t("mapLegendNeighbor"))}</span>
    <span><i class="dt-map-swatch dt-map-swatch-line" style="background:var(--lin-special)"></i>${escapeHtml(t("mapLegendSpecialization"))}</span>
    <span><i class="dt-map-swatch dt-map-swatch-line" style="background:var(--lin-equiv)"></i>${escapeHtml(t("mapLegendEquivalent"))}</span>
  `;
  wrap.appendChild(legend);

  const hint = document.createElement("p");
  hint.className = "dt-concept-map-hint";
  hint.textContent = t("mapClickToRecenter");
  wrap.appendChild(hint);

  return wrap;
}

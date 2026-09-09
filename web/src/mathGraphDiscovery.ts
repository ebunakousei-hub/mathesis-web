/**
 * P7.4（`docs/P7_4_STATUS.md`）: 「Math-Graph比較/発見モード」パネル。
 * 既存の`ProofGraphExplorer`/`LineageView`（数値judgment idの契約に縛られる、
 * P7.1参照）とは完全に独立した読み取り専用の別パネル——
 * `mathesis-provenance export-discovery`が書き出した自己完結JSONを1回
 * fetchするだけで、本番の系譜ビュー・検索・既定の信頼グラフには一切
 * 触れない。既定は「外部の発見を隠す」——ユーザーが明示的にトグルを
 * 押すまでMath-Graph由来の辺は一切表示しない。
 */
import type { DiscoveryEdge, DiscoveryExport, DiscoverySource } from "./types";
import { escapeHtml, reportProvenanceIssue } from "./util";

const SOURCE_LABEL: Record<DiscoverySource, string> = {
  "mathesis-checker": "Mathesis checker-derived (own Lean build)",
  "mathesis-text": "Mathesis text-extracted",
  "math-graph-literal": "Math-Graph literal dependency (external)",
  "math-graph-hierarchy": "Math-Graph typeclass-hierarchy discovery (external)",
};

const SOURCE_BADGE_CLASS: Record<DiscoverySource, string> = {
  "mathesis-checker": "mgd-badge-checker",
  "mathesis-text": "mgd-badge-text",
  "math-graph-literal": "mgd-badge-literal",
  "math-graph-hierarchy": "mgd-badge-hierarchy",
};

function isExternal(source: DiscoverySource): boolean {
  return source === "math-graph-literal" || source === "math-graph-hierarchy";
}

export class MathGraphDiscoveryPanel {
  private root: HTMLElement;
  private projects: DiscoveryExport[] = [];
  private loadError = false;
  private loaded = false;
  /** P7.4: 既定でfalse——「外部の辺は既定では隠す」というユーザー指示そのもの。 */
  private showExternal = false;

  constructor(root: HTMLElement, private sourcePaths: string[]) {
    this.root = root;
    this.render();
    this.load();
  }

  private async load(): Promise<void> {
    try {
      const responses = await Promise.all(this.sourcePaths.map((p) => fetch(p)));
      for (const [i, resp] of responses.entries()) {
        if (!resp.ok) {
          if (resp.status !== 404) reportProvenanceIssue(`${this.sourcePaths[i]} returned HTTP ${resp.status}`);
          this.loadError = true;
          this.render();
          return;
        }
      }
      this.projects = (await Promise.all(responses.map((r) => r.json()))) as DiscoveryExport[];
      this.loaded = true;
    } catch (err) {
      reportProvenanceIssue(`Math-Graph discovery panel failed to load: ${err}`);
      this.loadError = true;
    }
    this.render();
  }

  private render(): void {
    const wrap = document.createElement("div");
    wrap.className = "mgd-wrap";

    const heading = document.createElement("h3");
    heading.className = "mgd-heading";
    heading.textContent = "Math-Graph comparison / discovery mode";
    wrap.appendChild(heading);

    const intro = document.createElement("p");
    intro.className = "mgd-intro";
    intro.innerHTML =
      "A bounded, offline comparison against <a href=\"https://huggingface.co/datasets/uw-math-ai/math-graph\" target=\"_blank\" rel=\"noopener\">Math-Graph</a> " +
      "(uw-math-ai, CC BY 4.0) for two pilot Mathlib namespaces (<code>docs/P7_STATUS.md</code>&ndash;<code>P7_4_STATUS.md</code>). " +
      "External edges are <b>hidden by default</b> and never affect the default lineage view, search, or trusted-traversal graph.";
    wrap.appendChild(intro);

    if (this.loadError) {
      const err = document.createElement("p");
      err.className = "mgd-status mgd-error";
      err.textContent = "Could not load the discovery export (it may not have been generated for this build).";
      wrap.appendChild(err);
      this.root.innerHTML = "";
      this.root.appendChild(wrap);
      return;
    }
    if (!this.loaded) {
      const loading = document.createElement("p");
      loading.className = "mgd-status";
      loading.textContent = "Loading…";
      wrap.appendChild(loading);
      this.root.innerHTML = "";
      this.root.appendChild(wrap);
      return;
    }

    wrap.appendChild(this.renderToggle());
    for (const project of this.projects) {
      wrap.appendChild(this.renderProject(project));
    }

    this.root.innerHTML = "";
    this.root.appendChild(wrap);
  }

  private renderToggle(): HTMLElement {
    const bar = document.createElement("div");
    bar.className = "mgd-toggle-bar";

    const label = document.createElement("label");
    label.className = "mgd-toggle";
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = this.showExternal;
    checkbox.onchange = () => {
      this.showExternal = checkbox.checked;
      this.render();
    };
    label.appendChild(checkbox);
    const span = document.createElement("span");
    span.textContent = "Show external Math-Graph discoveries";
    label.appendChild(span);
    bar.appendChild(label);

    return bar;
  }

  private renderProject(project: DiscoveryExport): HTMLElement {
    const section = document.createElement("div");
    section.className = "mgd-project";

    const title = document.createElement("h4");
    title.className = "mgd-project-title";
    title.textContent = project.projectLabel;
    section.appendChild(title);

    const counts = document.createElement("div");
    counts.className = "mgd-counts";
    counts.innerHTML = `
      <span class="mgd-count-chip ${SOURCE_BADGE_CLASS["mathesis-checker"]}">${project.counts.mathesisChecker} checker-derived</span>
      <span class="mgd-count-chip ${SOURCE_BADGE_CLASS["mathesis-text"]}">${project.counts.mathesisText} text-extracted</span>
      <span class="mgd-count-chip ${SOURCE_BADGE_CLASS["math-graph-literal"]}">${project.counts.mathGraphLiteral} Math-Graph literal</span>
      <span class="mgd-count-chip ${SOURCE_BADGE_CLASS["math-graph-hierarchy"]}">${project.counts.mathGraphHierarchy} Math-Graph hierarchy</span>
    `;
    section.appendChild(counts);

    const visibleEdges = project.edges.filter((e) => this.showExternal || !isExternal(e.source));
    const externalHidden = project.edges.length - visibleEdges.length;
    if (!this.showExternal && externalHidden > 0) {
      const hint = document.createElement("p");
      hint.className = "mgd-hidden-hint";
      hint.textContent = `${externalHidden} external Math-Graph edge(s) hidden — enable the toggle above to show them.`;
      section.appendChild(hint);
    }

    const list = document.createElement("ul");
    list.className = "mgd-edge-list";
    // Mathesis's own edges are summarized, not enumerated one-by-one here
    // (the existing lineage view already does that job) -- this panel's
    // marginal value is the external comparison, so only list edges that
    // are either external or otherwise worth surfacing in this context.
    for (const edge of visibleEdges) {
      if (!isExternal(edge.source)) continue;
      list.appendChild(this.renderEdge(edge));
    }
    if (this.showExternal && list.children.length === 0) {
      const empty = document.createElement("p");
      empty.className = "mgd-status";
      empty.textContent = "No external edges for this project.";
      section.appendChild(empty);
    } else if (this.showExternal) {
      section.appendChild(list);
    }

    return section;
  }

  private renderEdge(edge: DiscoveryEdge): HTMLElement {
    const li = document.createElement("li");
    li.className = "mgd-edge-item";
    const badgeClass = SOURCE_BADGE_CLASS[edge.source];
    li.innerHTML = `
      <div class="mgd-edge-row">
        <span class="mgd-badge ${badgeClass}">${escapeHtml(SOURCE_LABEL[edge.source])}</span>
        <span class="mgd-edge-relation">${escapeHtml(edge.subject)} &rarr; ${escapeHtml(edge.object)}</span>
      </div>
      <div class="mgd-edge-meta">
        epistemic state: <b>${escapeHtml(edge.epistemicState)}</b> ·
        traversal: <b>${escapeHtml(edge.traversalPolicy)}</b>
        ${edge.edgeType ? ` · edge type: ${escapeHtml(edge.edgeType)}` : ""}
      </div>
      <div class="mgd-edge-attribution">
        External dataset: Math-Graph${edge.license ? ` &mdash; ${escapeHtml(edge.license)}` : ""} &mdash; not independently verified by Mathesis.
      </div>
      ${edge.locator ? `<div class="mgd-edge-locator">${escapeHtml(edge.locator)}</div>` : ""}
    `;
    return li;
  }
}

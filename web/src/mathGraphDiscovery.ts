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

/** 一度に描画する辺の件数——「paginated TheoremGraph results」要求
 * （P8.2 Stage 3ディレクティブ）への対応。P8.1で1本のexportに最大数百件の
 * 外部辺が載りうるようになったため、無制限描画は避ける。 */
const EDGES_PAGE_SIZE = 50;

export class MathGraphDiscoveryPanel {
  private root: HTMLElement;
  private projects: DiscoveryExport[] = [];
  private loadError = false;
  private loaded = false;
  /** P7.4: 既定でfalse——「外部の辺は既定では隠す」というユーザー指示そのもの。 */
  private showExternal = false;
  /** プロジェクトグループ単位(`projectLabel`+`repoSlug`)で「もっと見る」を
   * 何回押したか。キーが無ければ1ページ目(`EDGES_PAGE_SIZE`件)だけ表示。 */
  private visibleCount: Record<string, number> = {};

  constructor(root: HTMLElement, private sourcePaths: string[]) {
    this.root = root;
    this.render();
    this.load();
  }

  /**
   * P8.2: 各パスを独立にfetch・parseし、1本の失敗(404・その他の非ok・
   * JSONとして壊れている——例えばVite開発サーバのSPA fallbackが未生成
   * ファイルへの要求にも200+`index.html`を返すため、ローカル開発では
   * "200だがJSONではない"応答が実際に起きる)を他のプロジェクトへ
   * 巻き添えさせない。以前は`Promise.all`全体を1つのtry/catchで囲んで
   * いたため、1本のfetch/parseが失敗するだけで他の全プロジェクトの表示
   * まで消えていた(このファイル自身の冒頭コメントが謳う「404を静かに
   * 扱い、ページ全体を壊さない」を実際には満たしていなかった)。1つも
   * 読めなければ、その時だけエラー表示。
   */
  private async load(): Promise<void> {
    const results = await Promise.all(
      this.sourcePaths.map(async (path): Promise<DiscoveryExport | null> => {
        try {
          const resp = await fetch(path);
          if (!resp.ok) {
            if (resp.status !== 404) reportProvenanceIssue(`${path} returned HTTP ${resp.status}`);
            return null;
          }
          return (await resp.json()) as DiscoveryExport;
        } catch (err) {
          reportProvenanceIssue(`${path} failed to load or parse: ${err}`);
          return null;
        }
      }),
    );
    this.projects = results.filter((r): r is DiscoveryExport => r !== null);
    this.loaded = true;
    this.loadError = this.projects.length === 0;
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

    // P8.2: "coverage metrics" — a source-Lean-project breakdown of the
    // same external edges, when this export spans more than one repo
    // (P8.1's combined pilot DB does). Guarded with `?.` — an export
    // generated before P8.2 (schema_version predates these fields) won't
    // have this key at all; treat that the same as "no breakdown".
    if (project.byProject?.length > 0) {
      const byProjectBox = document.createElement("div");
      byProjectBox.className = "mgd-by-project";
      byProjectBox.innerHTML = project.byProject
        .map(
          (p) =>
            `<span class="mgd-count-chip mgd-badge-project">${escapeHtml(p.repoSlug)}: ${p.literalCount} literal, ${p.hierarchyCount} hierarchy</span>`,
        )
        .join(" ");
      section.appendChild(byProjectBox);
    }

    // P8.2: an honest statement of MSC coverage for this export's external
    // declarations — see `discovery_export.rs::msc_classification_note`.
    if (project.mscClassificationNote?.length > 0) {
      const note = document.createElement("p");
      note.className = "mgd-msc-note";
      note.textContent = project.mscClassificationNote;
      section.appendChild(note);
    }

    const visibleEdges = project.edges.filter((e) => this.showExternal || !isExternal(e.source));
    const externalHidden = project.edges.length - visibleEdges.length;
    if (!this.showExternal && externalHidden > 0) {
      const hint = document.createElement("p");
      hint.className = "mgd-hidden-hint";
      hint.textContent = `${externalHidden} external Math-Graph edge(s) hidden — enable the toggle above to show them.`;
      section.appendChild(hint);
    }

    if (this.showExternal) {
      // P8.2: group external edges by their source Lean project (falling
      // back to one unlabeled group when `sourceProject` can't be resolved,
      // e.g. the older project2/3.json exports) so a combined multi-project
      // export like P8.1's isn't one undifferentiated wall of edges.
      const groups = new Map<string, DiscoveryEdge[]>();
      for (const edge of visibleEdges) {
        if (!isExternal(edge.source)) continue;
        const key = edge.sourceProject ?? "";
        const list = groups.get(key);
        if (list) list.push(edge);
        else groups.set(key, [edge]);
      }
      if (groups.size === 0) {
        const empty = document.createElement("p");
        empty.className = "mgd-status";
        empty.textContent = "No external edges for this project.";
        section.appendChild(empty);
      } else {
        for (const [repoSlug, edges] of groups) {
          section.appendChild(this.renderEdgeGroup(project.projectLabel, repoSlug, edges));
        }
      }
    }

    return section;
  }

  /** P8.2: one source-project's edges, paginated ("paginated TheoremGraph results"). */
  private renderEdgeGroup(projectLabel: string, repoSlug: string, edges: DiscoveryEdge[]): HTMLElement {
    const box = document.createElement("div");
    box.className = "mgd-edge-group";

    if (repoSlug.length > 0) {
      const heading = document.createElement("h5");
      heading.className = "mgd-edge-group-title";
      heading.textContent = `${repoSlug} (${edges.length})`;
      box.appendChild(heading);
    }

    const groupKey = `${projectLabel}::${repoSlug}`;
    const shown = Math.min(this.visibleCount[groupKey] ?? EDGES_PAGE_SIZE, edges.length);

    const list = document.createElement("ul");
    list.className = "mgd-edge-list";
    for (const edge of edges.slice(0, shown)) list.appendChild(this.renderEdge(edge));
    box.appendChild(list);

    if (shown < edges.length) {
      const more = document.createElement("button");
      more.type = "button";
      more.className = "mgd-show-more";
      more.textContent = `Show more (${shown} of ${edges.length})`;
      more.onclick = () => {
        this.visibleCount[groupKey] = shown + EDGES_PAGE_SIZE;
        this.render();
      };
      box.appendChild(more);
    }

    return box;
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

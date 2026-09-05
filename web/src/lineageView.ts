import { t, type TKey } from "./i18n";
import {
  buildLineage,
  DEFAULT_LINEAGE_OPTIONS,
  NODE_H,
  NODE_W,
  spineOutline,
  type Lineage,
  type LineageEdge,
  type LineageGraph,
  type LineageNode,
  type LineageOptions,
  type LineageRelation,
} from "./lineage";
import { showAssertionDetail } from "./provenancePanel";
import { renderStatementWithMath } from "./tex";
import type { ExportedJudgment, ExportedMorphism } from "./types";
import { escapeHtml, unwrapLeanSymbols } from "./util";

const SVG_NS = "http://www.w3.org/2000/svg";

const RELATION_LABEL_KEY: Record<LineageRelation, TKey> = {
  dependency: "relDependency",
  implication: "kindImplication",
  specialization: "kindSpecialization",
  generalization: "kindGeneralization",
  equivalence: "kindEquivalence",
};

/**
 * 射の`status`（proposed/accepted/rejected）を、短いバッジ文言と
 * 詳しい説明（ツールチップ用）の組で持つ。バッジだけでは
 * 「ヒューリスティックが機械的に提案しただけで人間のレビューはまだ」
 * という重要な留保が伝わらない——ツールチップの長文に埋めると見落とされる
 * ため、バッジ自体は短く目立つ形で必ず表示し、詳細は補足として添える。
 */
const MORPHISM_STATUS_KEYS: Record<ExportedMorphism["status"], [TKey, TKey]> = {
  proposed: ["morphismStatusProposedBadge", "morphismStatusProposed"],
  accepted: ["morphismStatusAcceptedBadge", "morphismStatusAccepted"],
  rejected: ["morphismStatusRejectedBadge", "morphismStatusRejected"],
};

const PARSE_STATUS_LABEL_KEY: Record<string, TKey> = {
  full: "statusFull",
  partial: "statusPartial",
  failed: "statusFailed",
  informal: "statusInformal",
};

/** `parseStatus`の生の値（"full"/"partial"/"failed"/"informal"）を日英ラベルへ。 */
function parseStatusLabel(parseStatus: string): string {
  const key = PARSE_STATUS_LABEL_KEY[parseStatus];
  return key ? t(key) : parseStatus;
}

export interface LineageViewHandlers {
  /** ノードをもう一段掘る（そのノードを新しい根にする）。 */
  onReroot: (id: number) => void;
  /** 概念タクソノミー側へ渡す（この判断に関係しそうな概念を探す）。 */
  onSearchConcepts: (query: string) => void;
}

/**
 * 系譜（`lineage.ts` が組んだグラフ）をDOMにする。
 *
 * 辺はSVG、ノードはHTMLの `<button>` を絶対配置で重ねる。SVGの `<text>`
 * だけで組むと、Leanの識別子（`holder_Moser_of_homogeneousWeakSolution`
 * は39文字）の折り返し・省略・フォーカス可視化を全部自前で書く羽目に
 * なる。辺の曲線はSVGに、文字と当たり判定はHTMLに任せるほうが、
 * キーボード操作もそのまま効いて素直に済む。
 */
export class LineageView {
  private root: HTMLElement;
  private graph: LineageGraph;
  private chainDepth: Map<number, number>;
  private handlers: LineageViewHandlers;
  private opts: LineageOptions = { ...DEFAULT_LINEAGE_OPTIONS };

  private rootId: number | null = null;
  private selected: number | null = null;
  private expanded = new Set<number>();
  /** 判断名 → id。命題本文の中の識別子をリンクにするのに使う。 */
  private idByName = new Map<string, number>();

  constructor(root: HTMLElement, graph: LineageGraph, chainDepth: Map<number, number>, handlers: LineageViewHandlers) {
    this.root = root;
    this.graph = graph;
    this.chainDepth = chainDepth;
    this.handlers = handlers;
    for (const [id, j] of graph.judgmentById) {
      if (j.name !== null && j.name.length > 0) this.idByName.set(j.name, id);
    }
  }

  setRoot(id: number): void {
    this.rootId = id;
    this.selected = id;
    this.expanded.clear();
    this.render();
  }

  refreshLanguage(): void {
    this.render();
  }

  private render(): void {
    this.root.innerHTML = "";
    if (this.rootId === null) return;
    const lineage = buildLineage(this.rootId, this.graph, this.chainDepth, this.opts);
    if (lineage === null) return;

    const wrap = document.createElement("div");
    wrap.className = "lin";
    wrap.appendChild(this.renderHead(lineage));
    wrap.appendChild(this.renderLegend());
    wrap.appendChild(this.renderCanvas(lineage));
    wrap.appendChild(this.renderOutline(lineage));
    if (this.selected !== null) {
      const detail = this.renderDetail(this.selected);
      if (detail !== null) wrap.appendChild(detail);
    }
    this.root.appendChild(wrap);
  }

  // ── 見出しと操作 ───────────────────────────────────────────────

  private renderHead(lineage: Lineage): HTMLElement {
    const head = document.createElement("div");
    head.className = "lin-head";

    const shown = Math.max(...lineage.nodes.map((n) => n.depth));
    const summary = document.createElement("p");
    summary.className = "lin-summary";
    summary.textContent = t("lineageSummary")
      .replace("{shown}", String(shown))
      .replace("{full}", String(lineage.fullDepth))
      .replace("{nodes}", String(lineage.nodes.length))
      .replace("{omitted}", String(lineage.omitted));
    head.appendChild(summary);

    const controls = document.createElement("div");
    controls.className = "lin-controls";

    const depthBtn = (delta: number, key: TKey): void => {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "lin-ctl";
      btn.textContent = t(key);
      btn.disabled = delta < 0 ? this.opts.maxDepth <= 1 : this.opts.maxDepth >= 12;
      btn.onclick = () => {
        this.opts = { ...this.opts, maxDepth: Math.min(12, Math.max(1, this.opts.maxDepth + delta)) };
        this.render();
      };
      controls.appendChild(btn);
    };
    depthBtn(1, "lineageDeeper");
    depthBtn(-1, "lineageShallower");

    const morphToggle = document.createElement("button");
    morphToggle.type = "button";
    morphToggle.className = `lin-ctl ${this.opts.showMorphisms ? "active" : ""}`;
    morphToggle.textContent = t("lineageToggleMorphisms");
    morphToggle.onclick = () => {
      this.opts = { ...this.opts, showMorphisms: !this.opts.showMorphisms };
      this.render();
    };
    controls.appendChild(morphToggle);

    head.appendChild(controls);
    return head;
  }

  private renderLegend(): HTMLElement {
    const legend = document.createElement("div");
    legend.className = "lin-legend";
    const item = (cls: string, text: string): void => {
      const el = document.createElement("span");
      el.className = "lin-legend-item";
      el.innerHTML = `<span class="lin-legend-line ${cls}"></span>${escapeHtml(text)}`;
      legend.appendChild(el);
    };
    item("is-spine", t("legendSpine"));
    item("is-dependency", t("legendDependency"));
    item("is-specialization", t("legendSpecialization"));
    item("is-equivalence", t("legendEquivalence"));
    return legend;
  }

  // ── 図 ────────────────────────────────────────────────────────

  private renderCanvas(lineage: Lineage): HTMLElement {
    const viewport = document.createElement("div");
    viewport.className = "lin-viewport";

    const canvas = document.createElement("div");
    canvas.className = "lin-canvas";
    canvas.style.width = `${lineage.width}px`;
    canvas.style.height = `${lineage.height}px`;

    const pos = new Map(lineage.nodes.map((n) => [n.id, n]));
    canvas.appendChild(this.renderEdges(lineage, pos));
    for (const node of lineage.nodes) canvas.appendChild(this.renderNode(node, lineage));

    viewport.appendChild(canvas);
    return viewport;
  }

  private renderEdges(lineage: Lineage, pos: Map<number, LineageNode>): SVGSVGElement {
    const svg = document.createElementNS(SVG_NS, "svg");
    svg.setAttribute("class", "lin-edges");
    svg.setAttribute("width", String(lineage.width));
    svg.setAttribute("height", String(lineage.height));
    svg.setAttribute("aria-hidden", "true");

    const defs = document.createElementNS(SVG_NS, "defs");
    for (const [id, cls] of [
      ["lin-arrow", "is-dependency"],
      ["lin-arrow-spine", "is-spine"],
      ["lin-arrow-rel", "is-relation"],
    ] as const) {
      const marker = document.createElementNS(SVG_NS, "marker");
      marker.setAttribute("id", id);
      marker.setAttribute("viewBox", "0 0 8 8");
      marker.setAttribute("refX", "7");
      marker.setAttribute("refY", "4");
      marker.setAttribute("markerWidth", "7");
      marker.setAttribute("markerHeight", "7");
      marker.setAttribute("orient", "auto-start-reverse");
      const path = document.createElementNS(SVG_NS, "path");
      path.setAttribute("d", "M0,0 L8,4 L0,8 z");
      path.setAttribute("class", `lin-arrow-head ${cls}`);
      marker.appendChild(path);
      defs.appendChild(marker);
    }
    svg.appendChild(defs);

    // 選んだノードに繋がる辺だけ前面に出す（重なったときに読めるように）。
    const ordered = [...lineage.edges].sort((a, b) => Number(this.touches(a)) - Number(this.touches(b)));
    for (const edge of ordered) {
      const from = pos.get(edge.from);
      const to = pos.get(edge.to);
      if (from === undefined || to === undefined) continue;
      svg.appendChild(this.edgePath(edge, from, to));
    }
    return svg;
  }

  private touches(edge: LineageEdge): boolean {
    return this.selected !== null && (edge.from === this.selected || edge.to === this.selected);
  }

  private edgePath(edge: LineageEdge, from: LineageNode, to: LineageNode): SVGPathElement {
    const path = document.createElementNS(SVG_NS, "path");
    const x1 = from.x + NODE_W / 2;
    const x2 = to.x + NODE_W / 2;

    let d: string;
    let marker: string;
    if (edge.relation === "dependency") {
      // 上の段 → 下の段。垂直に立ち上げてから曲げる。
      const y1 = from.y + NODE_H;
      const y2 = to.y;
      const bend = Math.max(18, (y2 - y1) * 0.45);
      d = `M${x1},${y1} C${x1},${y1 + bend} ${x2},${y2 - bend} ${x2},${y2}`;
      marker = edge.onSpine ? "lin-arrow-spine" : "lin-arrow";
    } else {
      // 射は段を跨がないことがある。左右の縁を結んで、依存辺と混ざらない
      // ように大きく外へ膨らませる。
      const leftToRight = x1 <= x2;
      const sx = leftToRight ? from.x + NODE_W : from.x;
      const ex = leftToRight ? to.x : to.x + NODE_W;
      const sy = from.y + NODE_H / 2;
      const ey = to.y + NODE_H / 2;
      const bow = leftToRight ? 34 : -34;
      d = `M${sx},${sy} C${sx + bow},${sy} ${ex - bow},${ey} ${ex},${ey}`;
      marker = "lin-arrow-rel";
    }

    path.setAttribute("d", d);
    path.setAttribute("marker-end", `url(#${marker})`);
    path.setAttribute(
      "class",
      [
        "lin-edge",
        `is-${edge.relation}`,
        edge.onSpine ? "is-spine" : "",
        this.touches(edge) ? "is-active" : "",
        this.selected !== null && !this.touches(edge) ? "is-dimmed" : "",
      ]
        .filter(Boolean)
        .join(" "),
    );
    return path;
  }

  private renderNode(node: LineageNode, lineage: Lineage): HTMLElement {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.style.left = `${node.x}px`;
    btn.style.top = `${node.y}px`;
    btn.style.width = `${NODE_W}px`;
    btn.style.height = `${NODE_H}px`;
    btn.className = [
      "lin-node",
      `is-${node.judgment.kind}`,
      node.onSpine ? "is-spine" : "",
      node.id === lineage.rootId ? "is-root" : "",
      node.depth < 0 ? "is-above" : "",
      node.id === this.selected ? "is-selected" : "",
    ]
      .filter(Boolean)
      .join(" ");

    const name = node.judgment.name ?? t("anonymousLabel");
    btn.title = `${name}\n${node.judgment.statement}`;
    btn.innerHTML = `
      <span class="lin-node-name">${escapeHtml(name)}</span>
      <span class="lin-node-meta">
        <span class="lin-node-kind">${escapeHtml(node.judgment.kind)}</span>
        ${node.hiddenDeps > 0 ? `<span class="lin-node-more">+${node.hiddenDeps}</span>` : ""}
      </span>
    `;
    btn.onclick = () => {
      this.selected = node.id;
      this.render();
      this.root.querySelector(".lin-detail")?.scrollIntoView({ behavior: "smooth", block: "nearest" });
    };
    btn.ondblclick = () => this.handlers.onReroot(node.id);
    return btn;
  }

  // ── 証明の概略 ────────────────────────────────────────────────

  /**
   * 背骨を番号付きで読み下す。各段は畳んだ状態では「何に依拠するか」の
   * 1行だけを出し、押すと命題本文と前提が開く——絵で形を掴んでから、
   * 気になった段だけ中身を読む、という順で辿れるようにする。
   */
  private renderOutline(lineage: Lineage): HTMLElement {
    const box = document.createElement("div");
    box.className = "lin-outline";

    const heading = document.createElement("h4");
    heading.className = "lin-outline-title";
    heading.textContent = t("lineageOutlineTitle");
    box.appendChild(heading);

    const hint = document.createElement("p");
    hint.className = "lin-outline-hint";
    hint.textContent = t("lineageOutlineHint");
    box.appendChild(hint);

    const outline = spineOutline(lineage, this.graph);
    const list = document.createElement("ol");
    list.className = "lin-steps";

    outline.steps.forEach((step, i) => {
      const li = document.createElement("li");
      li.className = `lin-step ${this.expanded.has(step.id) ? "is-open" : ""}`;

      const head = document.createElement("button");
      head.type = "button";
      head.className = "lin-step-head";
      const because =
        step.next === null
          ? t("lineageStepBase")
          : t("lineageStepBecause")
              .replace("{next}", step.next.name ?? t("anonymousLabel"))
              .replace("{side}", String(step.sideCount));
      head.innerHTML = `
        <span class="lin-step-marker">${i + 1}</span>
        <span class="lin-step-body">
          <span class="lin-step-name">${escapeHtml(step.judgment.name ?? t("anonymousLabel"))}</span>
          <span class="lin-step-because">${escapeHtml(because)}</span>
        </span>
        <span class="lin-step-caret" aria-hidden="true">${this.expanded.has(step.id) ? "▾" : "▸"}</span>
      `;
      head.setAttribute("aria-expanded", String(this.expanded.has(step.id)));
      head.onclick = () => {
        if (this.expanded.has(step.id)) this.expanded.delete(step.id);
        else this.expanded.add(step.id);
        this.selected = step.id;
        this.render();
      };
      li.appendChild(head);

      if (this.expanded.has(step.id)) {
        const body = document.createElement("div");
        body.className = "lin-step-detail";
        body.appendChild(this.renderStatement(step.judgment.statement, step.judgment.parseStatus));
        if (step.side.length > 0) {
          const supports = document.createElement("div");
          supports.className = "lin-step-supports";
          supports.innerHTML = `<span class="lin-step-supports-label">${escapeHtml(t("lineageStepSideLabel"))}</span>`;
          for (const sideJudgment of step.side) supports.appendChild(this.chip(sideJudgment));
          if (step.sideCount > step.side.length) {
            const more = document.createElement("span");
            more.className = "lin-more";
            more.textContent = `+${step.sideCount - step.side.length}`;
            supports.appendChild(more);
          }
          body.appendChild(supports);
        }
        li.appendChild(body);
      }
      list.appendChild(li);
    });

    // 鎖が表示件数より長いとき、残りが何段あるかを最後に置く。ここが
    // 無いと「12段で終わり」に見えてしまい、47段の鎖を12段と誤解させる。
    if (outline.remaining > 0) {
      const rest = document.createElement("li");
      rest.className = "lin-step lin-step-rest";
      rest.textContent = t("lineageOutlineRest")
        .replace("{remaining}", String(outline.remaining))
        .replace("{total}", String(outline.total));
      list.appendChild(rest);
    }

    box.appendChild(list);
    return box;
  }

  // ── 選んだ判断の詳細 ──────────────────────────────────────────

  private renderDetail(id: number): HTMLElement | null {
    const j = this.graph.judgmentById.get(id);
    if (j === undefined) return null;

    const box = document.createElement("div");
    box.className = "lin-detail";

    const head = document.createElement("div");
    head.className = "lin-detail-head";
    head.innerHTML = `
      <span class="pg-row-kind pg-kind-${escapeHtml(j.kind)}">${escapeHtml(j.kind)}</span>
      <h4 class="lin-detail-name">${escapeHtml(j.name ?? t("anonymousLabel"))}</h4>
    `;
    const reroot = document.createElement("button");
    reroot.type = "button";
    reroot.className = "lin-ctl lin-reroot";
    reroot.textContent = t("lineageReroot");
    reroot.onclick = () => this.handlers.onReroot(id);
    head.appendChild(reroot);
    box.appendChild(head);

    box.appendChild(this.renderStatement(j.statement, j.parseStatus));

    if (j.context.length > 0) {
      const ctx = document.createElement("div");
      ctx.className = "lin-detail-context";
      ctx.innerHTML =
        `<span class="lin-detail-label">${escapeHtml(t("contextLabel"))}</span>` +
        j.context.map((c) => `<code class="pg-context-item">${escapeHtml(c)}</code>`).join("");
      box.appendChild(ctx);
    }

    const meta = document.createElement("div");
    meta.className = "lin-detail-meta";
    const arxiv =
      j.paperArxivId === null
        ? ""
        : `<a class="lin-arxiv" href="https://arxiv.org/abs/${encodeURIComponent(j.paperArxivId)}" target="_blank" rel="noopener">arXiv:${escapeHtml(j.paperArxivId)}</a>`;
    meta.innerHTML = `
      <span class="lin-src">${escapeHtml(j.sourceFile)}:${j.sourceLine}</span>
      <span class="pg-parse-status pg-parse-${escapeHtml(j.parseStatus)}" title="${escapeHtml(t("parseStatusNotVerifiedHint"))}">${escapeHtml(t("parseStatus"))}: ${escapeHtml(parseStatusLabel(j.parseStatus))}</span>
      ${arxiv}
    `;
    box.appendChild(meta);

    box.appendChild(this.renderRelationRow(t("dependsOnLabel"), this.graph.dependsOn.get(id) ?? []));
    box.appendChild(this.renderRelationRow(t("usedByLabel"), this.graph.usedBy.get(id) ?? []));
    box.appendChild(this.renderMorphisms(id));

    const toConcepts = document.createElement("button");
    toConcepts.type = "button";
    toConcepts.className = "lin-ctl lin-to-concepts";
    toConcepts.textContent = t("lineageToConcepts");
    toConcepts.onclick = () => this.handlers.onSearchConcepts(j.name ?? j.statement);
    box.appendChild(toConcepts);

    return box;
  }

  private renderRelationRow(label: string, ids: number[]): HTMLElement {
    const row = document.createElement("div");
    row.className = "lin-detail-row";
    // 依存関係は識別子の名前一致で機械的に検出したもの（証明項上の
    // 最小依存であることや意味上の依存であることは未確認）——バッジまでは
    // 出さないが、ラベルにホバーすれば分かるようにしておく。
    row.innerHTML = `<span class="lin-detail-label" title="${escapeHtml(t("dependencyInferredHint"))}">${escapeHtml(label)} (${ids.length})</span>`;
    if (ids.length === 0) {
      const empty = document.createElement("span");
      empty.className = "pg-related-empty";
      empty.textContent = t("searchTierEmpty");
      row.appendChild(empty);
      return row;
    }
    for (const other of ids.slice(0, 12)) {
      const j = this.graph.judgmentById.get(other);
      if (j !== undefined) row.appendChild(this.chip(j));
    }
    if (ids.length > 12) {
      const more = document.createElement("span");
      more.className = "lin-more";
      more.textContent = `+${ids.length - 12}`;
      row.appendChild(more);
    }
    return row;
  }

  private renderMorphisms(id: number): HTMLElement {
    const row = document.createElement("div");
    row.className = "lin-detail-row";
    const morphisms = this.graph.morphismsOf.get(id) ?? [];
    row.innerHTML = `<span class="lin-detail-label">${escapeHtml(t("morphismsLabel"))} (${morphisms.length})</span>`;
    if (morphisms.length === 0) {
      const empty = document.createElement("span");
      empty.className = "pg-related-empty";
      empty.textContent = t("morphismsEmpty");
      row.appendChild(empty);
      return row;
    }
    for (const m of morphisms.slice(0, 8)) {
      const otherId = m.src === id ? m.dst : m.src;
      const other = this.graph.judgmentById.get(otherId);
      const chip = document.createElement("button");
      chip.type = "button";
      chip.className = `lin-chip is-${m.kind}`;
      // ステータス（proposed/accepted/rejected）を必ず可視のバッジとして
      // 出す——rationaleだけのツールチップに埋めると見落とされ、
      // ヒューリスティックが機械的に提案しただけの未承認候補が、承認済みの
      // 事実であるかのように見えてしまう（実データにはこれまで人間による
      // レビューを一切行っていないため、現状は全件が"proposed"）。
      const [badgeKey, hintKey] = MORPHISM_STATUS_KEYS[m.status] ?? MORPHISM_STATUS_KEYS.proposed;
      const statusBadge = `<span class="lin-chip-status lin-chip-status-${escapeHtml(m.status)}" title="${escapeHtml(t(hintKey))}">${escapeHtml(t(badgeKey))}</span>`;
      const rationale = m.rationale ? `${t(hintKey)} — ${t("morphismRationaleLabel")}: ${m.rationale}` : t(hintKey);
      chip.title = rationale;
      // Phase 1 (`mathesis-provenance`)追跡情報。無ければ何も足さない——
      // 既存の見た目・挙動は変わらない。詳細は`provenancePanel.ts`の
      // ダイアログで見せる——チップ本体のクリック（再root）を邪魔しない
      // よう別ボタンにする。
      const provenance = this.graph.morphismProvenance?.get(m.id);
      const provenanceBadge = provenance
        ? `<span class="lin-chip-provenance" title="assertion #${provenance.assertionId} (release ${escapeHtml(provenance.releaseTag)}) — click for details" data-assertion-id="${provenance.assertionId}">ⓘ</span>`
        : "";
      chip.innerHTML = `<span class="lin-chip-rel">${escapeHtml(t(RELATION_LABEL_KEY[m.kind]))}</span>${escapeHtml(other?.name ?? `#${otherId}`)}${statusBadge}${provenanceBadge}`;
      chip.onclick = (ev) => {
        const target = ev.target as HTMLElement;
        const provEl = target.closest<HTMLElement>(".lin-chip-provenance");
        if (provEl) {
          ev.stopPropagation();
          const id = Number(provEl.dataset.assertionId);
          void showAssertionDetail(id);
          return;
        }
        this.selected = otherId;
        this.render();
      };
      row.appendChild(chip);
    }
    if (morphisms.length > 8) {
      const more = document.createElement("span");
      more.className = "lin-more";
      more.textContent = `+${morphisms.length - 8}`;
      row.appendChild(more);
    }
    return row;
  }

  private chip(j: ExportedJudgment): HTMLElement {
    const chip = document.createElement("button");
    chip.type = "button";
    chip.className = `lin-chip is-${j.kind}`;
    chip.textContent = j.name ?? `#${j.id}`;
    chip.title = unwrapLeanSymbols(j.statement);
    chip.onclick = () => {
      this.selected = j.id;
      this.render();
    };
    return chip;
  }

  /**
   * 命題本文を出す。分岐は`parseStatus`で行う——由来がLeanかLaTeXかで
   * 「本文」の中身の性質そのものが違うため。
   *
   * Leanの命題（full/partial/failed）は `MeasurableSet
   * (unitBallBadAnnulusOne d)` のように、**他の判断の名前が本文の中に
   * 直接現れる**。その識別子をリンクにして、本文から依存先へそのまま
   * 飛べるようにする——「この式のこの記号は何だったか」を、別の欄を
   * 探しに行かずに追えるのが、このデータ構造でしか出せない見せ方なので。
   * 命題は Lean の構文そのものであってTeXではないので、TeXとして組版は
   * しない（変換したように見せるのは、実データに無い形を足すことになる）。
   * 等幅ではなく数学記号の揃った書体で、字間を空けて出すに留める。
   *
   * informalな命題（診断⑥、`mathesis-fulltext`が橋渡ししたarXivのLaTeX
   * 定理文）は逆に**本物のLaTeX**なので、地の文に埋め込まれた`$...$`等の
   * 数式区間だけ温MLで組版する（`renderStatementWithMath`）。識別子への
   * リンクは行わない——他の判断の名前が本文に直接現れる保証は無く
   * （そもそも別の言語の自然文）、Lean側の理由付けがそのまま当てはまらない。
   */
  private renderStatement(raw: string, parseStatus?: string): HTMLElement {
    const el = document.createElement("div");
    el.className = "lin-statement";
    if (parseStatus === "informal") {
      void renderStatementWithMath(el, raw);
      return el;
    }
    const statement = unwrapLeanSymbols(raw);
    const re = /[A-Za-z_][A-Za-z0-9_'.]*/g;
    let last = 0;
    let m: RegExpExecArray | null;
    while ((m = re.exec(statement)) !== null) {
      const targetId = this.idByName.get(m[0]);
      if (targetId === undefined) continue;
      if (m.index > last) el.appendChild(document.createTextNode(statement.slice(last, m.index)));
      const link = document.createElement("button");
      link.type = "button";
      link.className = "lin-ident";
      link.textContent = m[0];
      const targetStatement = this.graph.judgmentById.get(targetId)?.statement;
      link.title = targetStatement === undefined ? "" : unwrapLeanSymbols(targetStatement);
      link.onclick = () => {
        this.selected = targetId;
        this.render();
      };
      el.appendChild(link);
      last = m.index + m[0].length;
    }
    if (last < statement.length) el.appendChild(document.createTextNode(statement.slice(last)));
    return el;
  }
}

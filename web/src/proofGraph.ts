import { t } from "./i18n";
import { computeChainDepths, type LineageGraph } from "./lineage";
import { LineageView } from "./lineageView";
import {
  buildProofSearchIndex,
  NO_FILTERS,
  proofSearch,
  type JudgmentHit,
  type ProofSearchFilters,
  type ProofSearchIndex,
  type ProofSearchResult,
} from "./proofSearch";
import type { ExportedJudgment, ExportedMorphism, GraphExport, JudgmentsProvenanceExport } from "./types";
import { escapeHtml, formatGeneratedAt, unwrapLeanSymbols } from "./util";

const SEARCH_TOP_K = 25;

type View = {
  query: string;
  file: string | null;
  judgment: number | null;
  filters: ProofSearchFilters;
};

/**
 * Phase 10: `mathesis-import --export` が書き出した実データ（Phase 9で
 * Lean 4コーパスを取り込んだ判断グラフ、アーキテクチャ.txt 5.8）を表示する。
 * `web/src/main.ts`の「デモ判断ノード一覧」（`KernelStore.seedDemoJudgments()`、
 * 4件の手書きデモ）とは別物——こちらは`mathesis-graph`のSQLiteに実際に
 * 保存された判断・依存関係・論文リンクをそのまま読む。`DynamicTaxonomyExplorer`
 * と同じ「ファイル読み込み→カード形式の一覧+ドリルダウン」パターンを踏襲する。
 */
export class ProofGraphExplorer {
  private root: HTMLElement;
  private data: GraphExport | null = null;
  private loadError = false;
  private view: View = { query: "", file: null, judgment: null, filters: { ...NO_FILTERS } };

  private judgmentById = new Map<number, ExportedJudgment>();
  private dependsOn = new Map<number, number[]>();
  private usedBy = new Map<number, number[]>();
  private byFile = new Map<string, ExportedJudgment[]>();
  private morphismsOf = new Map<number, ExportedMorphism[]>();
  private morphismProvenance = new Map<number, { assertionId: number; releaseTag: string }>();
  private searchIndex: ProofSearchIndex | null = null;
  /**
   * 系譜ビュー。判断1件を選んだときの主役——依存の鎖を図と概略の両方で出す。
   * 検索のたびに作り直すと選択状態と展開状態が消えるので、根の要素ごと
   * 使い回す（`render()` は毎回 `innerHTML = ""` するが、この要素への
   * 参照は持ち続けているので中身は保たれる）。
   */
  private lineageView: LineageView | null = null;
  private lineageRoot = document.createElement("div");
  /** 検索語が変わったことを外へ知らせる（main.ts がURLへ写すのに使う）。 */
  onQueryChange: ((query: string) => void) | null = null;
  /** 判断から概念タクソノミー側へ渡す（main.ts が区画を切り替える）。 */
  onSearchConcepts: ((query: string) => void) | null = null;

  constructor(root: HTMLElement) {
    this.root = root;
    this.renderLoading();
    this.load();
  }

  refreshLanguage(): void {
    this.render();
  }

  /**
   * ページ最上部の統合検索から呼ぶ、上位ヒットだけを返す軽い問い合わせ。
   * 描画も表示状態の変更もしない。
   */
  topHits(query: string, limit: number): ExportedJudgment[] {
    if (this.searchIndex === null) return [];
    const r = proofSearch(query, this.searchIndex, NO_FILTERS, limit);
    return [...r.exact, ...r.name, ...r.statement].slice(0, limit).map((h) => h.judgment);
  }

  /** この区画にクエリを渡して開く（統合検索からの遷移用）。 */
  focusQuery(query: string): void {
    this.view = { ...this.view, query, file: null, judgment: null };
    this.render();
  }

  private async load(): Promise<void> {
    try {
      const resp = await fetch(`${import.meta.env.BASE_URL}judgments.json`);
      if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
      this.data = (await resp.json()) as GraphExport;
      this.buildIndices(this.data);
    } catch (err) {
      console.error("Failed to load judgments.json:", err);
      this.loadError = true;
    }
    // Phase 1 (`mathesis-provenance`)の追跡サイドカー。無くても/失敗しても
    // 既存の画面は今までどおり動く——単にこのMapが空のままになるだけ。
    try {
      const resp = await fetch(`${import.meta.env.BASE_URL}judgments.provenance.json`);
      if (resp.ok) {
        const prov = (await resp.json()) as JudgmentsProvenanceExport;
        for (const m of prov.morphisms) {
          this.morphismProvenance.set(m.morphismId, { assertionId: m.assertionId, releaseTag: prov.releaseTag });
        }
      }
    } catch (err) {
      console.warn("judgments.provenance.json not available:", err);
    }
    this.render();
  }

  private buildIndices(d: GraphExport): void {
    for (const j of d.judgments) {
      this.judgmentById.set(j.id, j);
      const list = this.byFile.get(j.sourceFile) ?? [];
      list.push(j);
      this.byFile.set(j.sourceFile, list);
    }
    for (const list of this.byFile.values()) {
      list.sort((a, b) => a.sourceLine - b.sourceLine);
    }
    for (const dep of d.dependencies) {
      const from = this.dependsOn.get(dep.from) ?? [];
      from.push(dep.to);
      this.dependsOn.set(dep.from, from);
      const to = this.usedBy.get(dep.to) ?? [];
      to.push(dep.from);
      this.usedBy.set(dep.to, to);
    }
    for (const m of d.morphisms) {
      const srcList = this.morphismsOf.get(m.src) ?? [];
      srcList.push(m);
      this.morphismsOf.set(m.src, srcList);
      if (m.dst !== m.src) {
        const dstList = this.morphismsOf.get(m.dst) ?? [];
        dstList.push(m);
        this.morphismsOf.set(m.dst, dstList);
      }
    }
    // 識別子のトークン分割は判断1,431件ぶん——キー入力のたびにやり直す
    // 必要は無いので、読み込み時に一度だけ索引にしておく。
    this.searchIndex = buildProofSearchIndex(d.judgments, d.dependencies, d.morphisms);

    // 各判断から下へ伸びる依存鎖の最長の長さ。根に依らない量なので、
    // 判断を選ぶたびにではなく、ここで4,797辺ぶん一度だけ計算する。
    const graph: LineageGraph = {
      judgmentById: this.judgmentById,
      dependsOn: this.dependsOn,
      usedBy: this.usedBy,
      morphismsOf: this.morphismsOf,
      // 同じMap参照を渡す——judgments.provenance.jsonの取得は`load()`側で
      // 並行して進み、この時点ではまだ空のことがある。後から埋まっても
      // 参照は共有されているので、次のrender()から反映される。
      morphismProvenance: this.morphismProvenance,
    };
    const chainDepth = computeChainDepths(this.dependsOn, this.judgmentById.keys());
    this.lineageView = new LineageView(this.lineageRoot, graph, chainDepth, {
      onReroot: (id) => {
        this.view = { ...this.view, judgment: id };
        this.render();
      },
      onSearchConcepts: (query) => this.onSearchConcepts?.(query),
    });
  }

  private renderLoading(): void {
    this.root.innerHTML = `<div class="pg-status">${t("proofGraphLoading")}</div>`;
  }

  private render(): void {
    if (this.loadError) {
      this.root.innerHTML = `<div class="pg-status pg-error">${t("proofGraphError")}</div>`;
      return;
    }
    if (!this.data) {
      this.renderLoading();
      return;
    }

    const active = document.activeElement;
    const wasSearchFocused = active instanceof HTMLInputElement && active.id === "pg-search-input";
    const cursorPos = wasSearchFocused ? active.selectionStart : null;

    const wrap = document.createElement("div");
    wrap.className = "pg-wrap";

    wrap.appendChild(this.renderStats(this.data));
    wrap.appendChild(this.renderSearchBox());

    const searching = this.view.query.trim().length > 0;
    if (searching || this.hasActiveFilter()) {
      wrap.appendChild(this.renderFilterBar());
    }

    if (this.view.judgment !== null) {
      const j = this.judgmentById.get(this.view.judgment);
      wrap.appendChild(j ? this.renderJudgmentDetail(j) : this.renderNotFound());
    } else if (searching) {
      wrap.appendChild(this.renderSearchResults());
    } else if (this.view.file !== null) {
      wrap.appendChild(this.renderJudgmentList(this.filtered(this.byFile.get(this.view.file) ?? []), this.view.file));
    } else {
      wrap.appendChild(this.renderFileBrowser());
    }

    this.root.innerHTML = "";
    this.root.appendChild(wrap);

    if (wasSearchFocused) {
      const input = this.root.querySelector<HTMLInputElement>("#pg-search-input");
      if (input) {
        input.focus();
        if (cursorPos !== null) input.setSelectionRange(cursorPos, cursorPos);
      }
    }
  }

  private hasActiveFilter(): boolean {
    return this.view.filters.kind !== null || this.view.filters.parseStatus !== null;
  }

  private filtered(judgments: ExportedJudgment[]): ExportedJudgment[] {
    const { kind, parseStatus } = this.view.filters;
    return judgments.filter(
      (j) => (kind === null || j.kind === kind) && (parseStatus === null || j.parseStatus === parseStatus),
    );
  }

  /** 種別（definition/theorem）とパース状態（full/partial）の絞り込み。 */
  private renderFilterBar(): HTMLElement {
    const bar = document.createElement("div");
    bar.className = "pg-filters";

    const group = (
      label: string,
      current: string | null,
      values: string[],
      apply: (next: string | null) => ProofSearchFilters,
    ): void => {
      const box = document.createElement("div");
      box.className = "pg-filter-group";
      const heading = document.createElement("span");
      heading.className = "pg-filter-label";
      heading.textContent = label;
      box.appendChild(heading);

      for (const value of [null, ...values]) {
        const btn = document.createElement("button");
        btn.type = "button";
        btn.className = `pg-filter-chip ${current === value ? "active" : ""}`;
        btn.textContent = value ?? t("filterAll");
        btn.onclick = () => {
          this.view = { ...this.view, filters: apply(value), judgment: null };
          this.render();
        };
        box.appendChild(btn);
      }
      bar.appendChild(box);
    };

    group(t("filterKindLabel"), this.view.filters.kind, ["definition", "theorem"], (kind) => ({
      ...this.view.filters,
      kind,
    }));
    group(t("filterParseLabel"), this.view.filters.parseStatus, ["full", "partial", "informal"], (parseStatus) => ({
      ...this.view.filters,
      parseStatus,
    }));

    return bar;
  }

  private renderSearchResults(): HTMLElement {
    const wrap = document.createElement("div");
    wrap.className = "pg-search-results";
    if (this.searchIndex === null) return wrap;

    const result = proofSearch(this.view.query, this.searchIndex, this.view.filters, SEARCH_TOP_K);

    wrap.appendChild(this.renderSearchTier(t("pgTierExact"), t("pgTierExactHint"), result.exact));
    wrap.appendChild(this.renderSearchTier(t("pgTierName"), t("pgTierNameHint"), result.name));
    wrap.appendChild(this.renderSearchTier(t("pgTierStatement"), t("pgTierStatementHint"), result.statement));
    wrap.appendChild(this.renderSearchTier(t("pgTierConnected"), t("pgTierConnectedHint"), result.connected));

    if (this.isEmptyResult(result)) {
      const empty = document.createElement("p");
      empty.className = "pg-empty";
      empty.textContent = t("proofGraphNoResults");
      wrap.appendChild(empty);
    }
    return wrap;
  }

  private isEmptyResult(r: ProofSearchResult): boolean {
    return r.exact.length === 0 && r.name.length === 0 && r.statement.length === 0 && r.connected.length === 0;
  }

  private renderSearchTier(label: string, hint: string, hits: JudgmentHit[]): HTMLElement {
    const section = document.createElement("div");
    section.className = "pg-search-tier";

    const heading = document.createElement("h4");
    heading.className = "pg-search-tier-title";
    heading.textContent = `${label} (${hits.length})`;
    heading.title = hint;
    section.appendChild(heading);

    const sub = document.createElement("p");
    sub.className = "pg-search-tier-hint";
    sub.textContent = hint;
    section.appendChild(sub);

    if (hits.length === 0) {
      const empty = document.createElement("p");
      empty.className = "pg-related-empty";
      empty.textContent = t("searchTierEmpty");
      section.appendChild(empty);
      return section;
    }

    const list = document.createElement("div");
    list.className = "pg-list";
    for (const hit of hits) {
      list.appendChild(this.renderJudgmentRow(hit.judgment, hit));
    }
    section.appendChild(list);
    return section;
  }

  /** `connected` 段階の「なぜ出てきたか」を1行で示す。 */
  private connectionNote(hit: JudgmentHit): string | null {
    if (hit.via === undefined) return null;
    const anchor = this.judgmentById.get(hit.via.anchorId);
    const anchorName = anchor?.name ?? `#${hit.via.anchorId}`;
    const relation =
      hit.via.relation === "dependsOn"
        ? t("viaDependsOn")
        : hit.via.relation === "usedBy"
          ? t("viaUsedBy")
          : t("viaMorphism");
    return `${relation}: ${anchorName}`;
  }

  private renderStats(d: GraphExport): HTMLElement {
    const el = document.createElement("div");
    el.className = "pg-stats";
    const paperBits = d.papers.map((p) => {
      const label = p.title ?? p.arxivId;
      return `<a href="https://arxiv.org/abs/${encodeURIComponent(p.arxivId)}" target="_blank" rel="noopener">${escapeHtml(label)}</a> (${p.judgmentCount})`;
    });
    el.innerHTML =
      `${d.judgmentCount.toLocaleString()} judgments · ${d.dependencyCount.toLocaleString()} dependency edges · ` +
      `${d.morphismCount.toLocaleString()} morphisms · ${t("generatedAtLabel")}: ${formatGeneratedAt(d.generatedAtUnix)}` +
      (paperBits.length > 0 ? ` · ${t("sourcePaperLabel")}: ${paperBits.join(", ")}` : "");
    return el;
  }

  private renderSearchBox(): HTMLElement {
    const wrap = document.createElement("div");
    wrap.className = "pg-search";

    const input = document.createElement("input");
    input.id = "pg-search-input";
    input.type = "text";
    input.className = "pg-search-input";
    input.placeholder = t("searchJudgmentsPlaceholder");
    input.autocomplete = "off";
    input.value = this.view.query;
    input.oninput = () => {
      this.view = { ...this.view, query: input.value, judgment: null };
      this.render();
      this.onQueryChange?.(input.value);
    };
    wrap.appendChild(input);

    if (this.view.query.trim().length > 0) {
      const clearBtn = document.createElement("button");
      clearBtn.type = "button";
      clearBtn.className = "pg-search-clear";
      clearBtn.textContent = "×";
      clearBtn.setAttribute("aria-label", t("clearSearch"));
      clearBtn.onclick = () => {
        this.view = { ...this.view, query: "", judgment: null };
        this.render();
        this.onQueryChange?.("");
      };
      wrap.appendChild(clearBtn);
    }

    return wrap;
  }

  private renderFileBrowser(): HTMLElement {
    const grid = document.createElement("div");
    grid.className = "pg-grid";
    const files = [...this.byFile.entries()]
      .map(([file, judgments]) => [file, this.filtered(judgments)] as const)
      .filter(([, judgments]) => judgments.length > 0)
      .sort((a, b) => a[0].localeCompare(b[0]));
    for (const [file, judgments] of files) {
      const card = document.createElement("button");
      card.type = "button";
      card.className = "pg-card";
      card.innerHTML = `
        <div class="pg-card-title">${escapeHtml(file)}</div>
        <div class="pg-card-meta">${judgments.length} ${t("judgmentsLabel")}</div>
      `;
      card.onclick = () => {
        this.view = { ...this.view, file, judgment: null };
        this.render();
      };
      grid.appendChild(card);
    }
    return grid;
  }

  private renderJudgmentList(judgments: ExportedJudgment[], file: string | null): HTMLElement {
    const wrap = document.createElement("div");

    if (file !== null) {
      const backBtn = document.createElement("button");
      backBtn.type = "button";
      backBtn.className = "pg-back";
      backBtn.textContent = t("back");
      backBtn.onclick = () => {
        this.view = { ...this.view, file: null, judgment: null };
        this.render();
      };
      wrap.appendChild(backBtn);

      const heading = document.createElement("h3");
      heading.className = "pg-file-heading";
      heading.textContent = file;
      wrap.appendChild(heading);
    }

    if (judgments.length === 0) {
      const empty = document.createElement("p");
      empty.className = "pg-empty";
      empty.textContent = t("proofGraphNoResults");
      wrap.appendChild(empty);
      return wrap;
    }

    const list = document.createElement("div");
    list.className = "pg-list";
    for (const j of judgments) {
      list.appendChild(this.renderJudgmentRow(j));
    }
    wrap.appendChild(list);
    return wrap;
  }

  private renderJudgmentRow(j: ExportedJudgment, hit?: JudgmentHit): HTMLElement {
    const row = document.createElement("button");
    row.type = "button";
    row.className = "pg-row";
    const note = hit === undefined ? null : this.connectionNote(hit);
    row.innerHTML = `
      <span class="pg-row-kind pg-kind-${escapeHtml(j.kind)}">${escapeHtml(j.kind)}</span>
      <span class="pg-row-name">${escapeHtml(j.name ?? t("anonymousLabel"))}</span>
      <span class="pg-row-statement">${escapeHtml(unwrapLeanSymbols(j.statement))}</span>
      ${note === null ? "" : `<span class="pg-row-via">${escapeHtml(note)}</span>`}
    `;
    row.onclick = () => {
      this.view = { ...this.view, judgment: j.id };
      this.render();
    };
    return row;
  }

  private renderNotFound(): HTMLElement {
    const el = document.createElement("p");
    el.className = "pg-empty";
    el.textContent = t("searchNoResults");
    return el;
  }

  /**
   * 判断1件を選んだときの画面。以前はここに「依存している判断 (5)」
   * 「参照している判断 (3)」というチップの列を出すだけだった——依存の鎖は
   * 実際には最大48段まで伸びているのに、利用者は1ホップずつ手で辿って
   * 頭の中で繋ぎ直すしかなかった。今は `LineageView` が鎖そのものを
   * 図と概略の両方で出し、そのノードを押せば同じ画面の中で辿り続けられる。
   */
  private renderJudgmentDetail(j: ExportedJudgment): HTMLElement {
    const wrap = document.createElement("div");
    wrap.className = "pg-detail";

    const backBtn = document.createElement("button");
    backBtn.type = "button";
    backBtn.className = "pg-back";
    backBtn.textContent = t("back");
    backBtn.onclick = () => {
      this.view = { ...this.view, judgment: null };
      this.render();
    };
    wrap.appendChild(backBtn);

    const header = document.createElement("div");
    header.className = "pg-detail-header";
    header.innerHTML = `
      <span class="pg-row-kind pg-kind-${escapeHtml(j.kind)}">${escapeHtml(j.kind)}</span>
      <h3 class="pg-detail-name">${escapeHtml(j.name ?? t("anonymousLabel"))}</h3>
    `;
    wrap.appendChild(header);

    const lineageHeading = document.createElement("h4");
    lineageHeading.className = "pg-lineage-title";
    lineageHeading.textContent = t("lineageTitle");
    wrap.appendChild(lineageHeading);

    this.lineageView?.setRoot(j.id);
    wrap.appendChild(this.lineageRoot);

    return wrap;
  }
}

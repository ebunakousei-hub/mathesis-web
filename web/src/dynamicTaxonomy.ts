import { buildConceptMap } from "./conceptMap";
import { renderConceptMap } from "./conceptMapView";
import {
  buildConceptSearchIndex,
  hybridSearch,
  type ConceptSearchIndex,
  type Correction,
  type HybridSearchResult,
  type SearchHit,
} from "./hybridSearch";
import { t } from "./i18n";
import { showAssertionDetail } from "./provenancePanel";
import { splitWords } from "./queryIndex";
import type { SearchWorkerMessage } from "./searchWorker";
import { expandRelatedEdges, expandSearchIndex } from "./taxonomyData";
import type {
  AliasExport,
  ConceptPaper,
  ExportedCluster,
  ExportedField,
  PapersExport,
  ProvenanceRelationEdge,
  RelatedEdge,
  RelatedEdgesExport,
  SearchIndexColumns,
  TaxonomyShell,
} from "./types";
import { escapeHtml, formatGeneratedAt } from "./util";

type Tab = "fields" | "novel";
type View = { tab: Tab; field: ExportedField | null; openCluster: number | null; query: string };

const SEARCH_TOP_K = 12;
/**
 * 打鍵から検索を走らせるまでの待ち時間。80,727件の索引に対する検索と
 * 区画全体の再描画を1打鍵ごとに同期実行すると入力が引っかかるので、
 * 打鍵が止まってからまとめて1回だけ走らせる。素早く打っている最中に
 * 中間状態の結果を出しても読まれないため、体感の情報量は落ちない。
 */
const SEARCH_DEBOUNCE_MS = 120;

/**
 * `web/src/fields.ts` の静的taxonomyとは別に、`mathesis-taxonomy export`
 * が書き出した実データ（アーキテクチャ.txt 5.7 Phase 6）を表示する。
 * 31分野規模になるとfields.tsの円形レイアウト（もともと5分野+学際領域
 * 用に設計）には収まらないため、ここではカード形式の一覧+ドリルダウンに
 * している——データの規模に合わせてUIパターンを選び直した。
 */
export class DynamicTaxonomyExplorer {
  private root: HTMLElement;
  private data: TaxonomyShell | null = null;
  private loadError = false;
  private view: View = { tab: "fields", field: null, openCluster: null, query: "" };
  /** フル索引（`searchWorker.ts`が構築してpostMessageで届ける）。 */
  private searchIndex: ConceptSearchIndex | null = null;
  /**
   * ヘッドシャード（`taxonomy.head.json`、文書頻度上位800件、数十KB）から
   * 即座に作る小さな索引（診断④「配信の不可分性」への対応）。フル索引が
   * Worker上でまだ出来上がっていない間、これで即答する——`ensureSearchIndex`
   * 参照。
   */
  private headSearchIndex: ConceptSearchIndex | null = null;
  private debounceTimer: number | null = null;
  /** 検索語が変わったことを外へ知らせる（main.ts がURLへ写すのに使う）。 */
  onQueryChange: ((query: string) => void) | null = null;
  /**
   * 「地図で見る」で開いている概念（診断⑤への対応）。nullなら閉じている。
   * クリックした行の直下に地図パネルを展開する——別画面へ飛ばさず、
   * その場で意味的近傍を辿れるようにする。
   */
  private mapFocus: string | null = null;
  /**
   * embedding近傍の辺（`taxonomy.related.json`、実データ10.57MB）。
   * ページを開いただけの利用者には要らないので、最初に検索されるまで
   * 取りに行かない。詳しい理由は Rust側 `export.rs::RelatedEdgesExport`。
   */
  private relatedEdges: Record<string, RelatedEdge[]> = {};
  private relatedState: "idle" | "loading" | "ready" | "error" = "idle";
  /**
   * 概念ごとの出典論文（`taxonomy.papers.json`）。`relatedEdges`と同じ
   * 理由——検索するまで要らない——で遅延読み込みにしてある。
   *
   * 以前はarXiv IDの文字列だけで、検索結果の単位が「フレーズ」に
   * とどまっていた。今は題名・年つきの**文書**として出す（詳しくは
   * Rust側 `export.rs::PapersExport`）。
   */
  private conceptPapers: Record<string, ConceptPaper[]> = {};
  private papersState: "idle" | "loading" | "ready" | "error" = "idle";
  /**
   * 表記ゆれ（`taxonomy.aliases.json`、0.75MB）。畳んだ事実を行に出す
   * ため——黙って畳むと、利用者は自分が打った表記が結果に無い理由が
   * 分からない。
   */
  private aliases: Record<string, string[]> = {};
  /**
   * 型付き関係（`relations.json`、P2 `docs/P2_STATUS.md`——
   * `mathesis-provenance web-export`が証拠層から直接生成）。フレーズ→
   * そのフレーズが関わる関係の一覧（向きは`TypedRelationView.relation`
   * 参照）。根拠文つきのConfirmed/Groundedしか含まれない。各行は生成元の
   * `assertionId`を最初から持っているので、以前の`relationProvenance`
   * サイドカー・突き合わせは不要になった。
   */
  private relations: Record<string, TypedRelationView[]> = {};

  constructor(root: HTMLElement) {
    this.root = root;
    this.renderLoading();
    this.loadHeadShard();
    this.loadFullIndexInWorker();
  }

  refreshLanguage(): void {
    this.render();
  }

  /**
   * ページ最上部の統合検索から呼ぶ、上位ヒットだけを返す軽い問い合わせ。
   * 描画も表示状態の変更もしない。
   *
   * `corrections` も一緒に返す。以前はここで捨てていたため、統合検索
   * 経由だと綴り訂正が起きても利用者に見えなかった——例えば
   * "Campanato"（この語自体はコーパスに無い）と打つと、区画側の検索欄
   * では「もしかして: campanato → campana」と出るのに、統合検索の
   * プレビューには無関係な「campana」が理由の説明なしに並ぶだけだった。
   * 動的タクソノミー区画自体はこの情報を既に`HybridSearchResult`として
   * 持っていたので、ここで捨てずに返すだけで直る。
   */
  topHits(query: string, limit: number): { hits: SearchHit[]; corrections: Correction[] } {
    const index = this.ensureSearchIndex();
    if (index === null || this.data === null) return { hits: [], corrections: [] };
    const result = hybridSearch(query, index, this.relatedEdges, limit);
    // 意図に近い順に、段階をまたいで詰める。
    const hits = [...result.exact, ...result.specialization, ...result.closest].slice(0, limit);
    return { hits, corrections: result.corrections };
  }

  /** この区画にクエリを渡して開く（統合検索からの遷移用）。 */
  focusQuery(query: string): void {
    this.view = { ...this.view, query, field: null, openCluster: null };
    this.render();
  }

  /**
   * `taxonomy.head.json`（文書頻度上位800件、数十KB）を主スレッドで
   * 直接読み込む。フル索引（9.0MB）の構築が終わる**前**でも、人気の高い
   * 概念への検索にはこれで即答できる——診断④「配信の不可分性」への
   * 対応の要（`export.rs::build_head_shard`参照）。件数が小さいので
   * `buildConceptSearchIndex`をこのスレッドで直接呼んでも数msで終わる。
   */
  private loadHeadShard(): void {
    void (async () => {
      try {
        const resp = await fetch(`${import.meta.env.BASE_URL}taxonomy.head.json`);
        if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
        const columns = (await resp.json()) as SearchIndexColumns;
        this.headSearchIndex = buildConceptSearchIndex(expandSearchIndex(columns));
        this.render();
      } catch (err) {
        // ヘッドシャードは即答のための最適化に過ぎない——読み込みに
        // 失敗してもフル索引（Worker側）が追い付けば検索は成立するので、
        // ここでは`loadError`を立てない。
        console.error("Failed to load taxonomy.head.json:", err);
      }
    })();
  }

  /**
   * `taxonomy.json`（9.0MB）のfetch・JSON.parse・索引構築をWeb Worker
   * （`searchWorker.ts`）へ丸ごと委ねる。実測632ms（parse 378ms + 索引
   * 構築254ms）がメインスレッドを塞いでいた問題（診断④）を、そのまま
   * 別スレッドへ移すことで直接解消する——バイト数を減らす最適化ではなく、
   * 「全件終わるまで1件も検索に答えられない」という不可分性そのものへの
   * 対処。索引が届くまでの間も`headSearchIndex`が検索に答え続ける。
   */
  private loadFullIndexInWorker(): void {
    const worker = new Worker(new URL("./searchWorker.ts", import.meta.url), { type: "module" });
    worker.onmessage = (ev: MessageEvent<SearchWorkerMessage>) => {
      const message = ev.data;
      if (message.type === "ready") {
        this.data = message.shell;
        this.searchIndex = message.searchIndex;
      } else {
        console.error("Failed to load taxonomy.json in worker:", message.message);
        this.loadError = true;
      }
      worker.terminate();
      this.render();
    };
    worker.onerror = (ev) => {
      console.error("searchWorker crashed:", ev.message);
      this.loadError = true;
      worker.terminate();
      this.render();
    };
  }

  /**
   * 検索が実際に行われたときだけ、近傍の辺を取りに行く。読み込み中も
   * 他の4段階は普通に出るので、待たされるのは「関連概念」段階だけ。
   */
  private ensureRelatedEdges(): void {
    if (this.relatedState !== "idle") return;
    this.relatedState = "loading";
    void (async () => {
      try {
        const resp = await fetch(`${import.meta.env.BASE_URL}taxonomy.related.json`);
        if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
        this.relatedEdges = expandRelatedEdges((await resp.json()) as RelatedEdgesExport);
        this.relatedState = "ready";
      } catch (err) {
        console.error("Failed to load taxonomy.related.json:", err);
        this.relatedState = "error";
      }
      // 読み終わった時点でまだ検索中なら、related段階だけ埋めて描き直す。
      if (this.view.query.trim().length > 0) this.render();
    })();
  }

  /** 検索が実際に行われたときだけ、出典論文と表記ゆれを取りに行く。 */
  private ensureSamplePapers(): void {
    if (this.papersState !== "idle") return;
    this.papersState = "loading";
    void (async () => {
      try {
        const resp = await fetch(`${import.meta.env.BASE_URL}taxonomy.papers.json`);
        if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
        this.conceptPapers = expandPapers((await resp.json()) as PapersExport);
        this.papersState = "ready";
      } catch (err) {
        console.error("Failed to load taxonomy.papers.json:", err);
        this.papersState = "error";
      }
      // 表記ゆれ・型付き関係はどちらも小さい（0.75MB / 0.58MB）ので
      // 同じきっかけでまとめて取る。失敗しても行の見た目が少し減る
      // だけなので、状態は持たない。
      try {
        const resp = await fetch(`${import.meta.env.BASE_URL}taxonomy.aliases.json`);
        if (resp.ok) this.aliases = expandAliases((await resp.json()) as AliasExport);
      } catch (err) {
        console.error("Failed to load taxonomy.aliases.json:", err);
      }
      try {
        const resp = await fetch(`${import.meta.env.BASE_URL}relations.json`);
        if (resp.ok) this.relations = expandRelations((await resp.json()) as ProvenanceRelationEdge[]);
      } catch (err) {
        console.error("Failed to load relations.json:", err);
      }
      if (this.view.query.trim().length > 0) this.render();
    })();
  }

  private renderLoading(): void {
    this.root.innerHTML = `<div class="dt-status">${t("dynamicTaxonomyLoading")}</div>`;
  }

  /**
   * フル索引（Worker側で構築中）がまだ届いていなければ、ヘッドシャード
   * （文書頻度上位800件）で代用する——どちらも無ければ`null`（読み込み
   * すら終わっていない、ページを開いた直後の一瞬だけ）。フル索引が届いた
   * 時点でこの区画は自動的に再描画され（`loadFullIndexInWorker`）、以後は
   * 常にフル索引を返すようになる。
   */
  private ensureSearchIndex(): ConceptSearchIndex | null {
    return this.searchIndex ?? this.headSearchIndex;
  }

  /** 表示中の検索結果が、フル索引ではなくヘッドシャードだけによるものか。 */
  private usingHeadShardOnly(): boolean {
    return this.searchIndex === null && this.headSearchIndex !== null;
  }

  private render(): void {
    if (this.loadError) {
      this.root.innerHTML = `<div class="dt-status dt-error">${t("dynamicTaxonomyError")}</div>`;
      return;
    }
    if (!this.data) {
      this.renderLoading();
      return;
    }

    // innerHTMLの丸ごと差し替えでフォーカスが飛ぶため、検索入力欄に
    // フォーカスがある間は再描画後に位置ごと戻す（1文字打つたびに
    // フォーカスが外れるのは検索体験として致命的なので）。
    const active = document.activeElement;
    const wasSearchFocused = active instanceof HTMLInputElement && active.id === "dt-search-input";
    const cursorPos = wasSearchFocused ? active.selectionStart : null;

    const wrap = document.createElement("div");
    wrap.className = "dt-wrap";

    const explain = document.createElement("p");
    explain.className = "section-sub dt-explain";
    explain.textContent = t("dynamicTaxonomyExplain");
    wrap.appendChild(explain);

    wrap.appendChild(this.renderStats(this.data));
    wrap.appendChild(this.renderSearchBox());

    const query = this.view.query.trim();
    if (query.length > 0) {
      wrap.appendChild(this.renderSearchResults(query));
    } else {
      wrap.appendChild(this.renderTabs());
      if (this.view.tab === "fields") {
        wrap.appendChild(this.view.field ? this.renderFieldDetail(this.view.field) : this.renderFieldGrid(this.data.fields));
      } else {
        wrap.appendChild(this.renderClusterGrid(this.data.novelClusters, true));
      }
    }

    this.root.innerHTML = "";
    this.root.appendChild(wrap);

    if (wasSearchFocused) {
      const input = this.root.querySelector<HTMLInputElement>("#dt-search-input");
      if (input) {
        input.focus();
        if (cursorPos !== null) input.setSelectionRange(cursorPos, cursorPos);
      }
    }
  }

  private renderSearchBox(): HTMLElement {
    const wrap = document.createElement("div");
    wrap.className = "dt-search";

    const input = document.createElement("input");
    input.id = "dt-search-input";
    input.type = "text";
    input.className = "dt-search-input";
    input.placeholder = t("searchConceptsPlaceholder");
    input.autocomplete = "off";
    input.value = this.view.query;
    input.oninput = () => {
      const typed = input.value;
      if (this.debounceTimer !== null) window.clearTimeout(this.debounceTimer);
      this.debounceTimer = window.setTimeout(() => {
        this.debounceTimer = null;
        this.view = { ...this.view, query: typed };
        this.render();
        this.onQueryChange?.(typed);
      }, SEARCH_DEBOUNCE_MS);
    };
    wrap.appendChild(input);

    if (this.view.query.trim().length > 0) {
      const clearBtn = document.createElement("button");
      clearBtn.type = "button";
      clearBtn.className = "dt-search-clear";
      clearBtn.textContent = "×";
      clearBtn.setAttribute("aria-label", t("clearSearch"));
      clearBtn.onclick = () => {
        this.view = { ...this.view, query: "" };
        this.render();
        this.onQueryChange?.("");
      };
      wrap.appendChild(clearBtn);
    }

    return wrap;
  }

  private renderSearchResults(query: string): HTMLElement {
    const wrap = document.createElement("div");
    wrap.className = "dt-search-results";
    const index = this.ensureSearchIndex();
    if (index === null) return wrap;
    this.ensureRelatedEdges();
    this.ensureSamplePapers();
    const result = hybridSearch(query, index, this.relatedEdges, SEARCH_TOP_K);

    const interpretation = this.renderInterpretation(query, result);
    if (interpretation !== null) wrap.appendChild(interpretation);

    // フル索引（Worker側）がまだ届いていない間は、ヘッドシャード
    // （文書頻度上位800件）だけで答えている——診断④への対応。人気の低い
    // 概念はまだ検索できないので、「該当なし」と区別できるよう伝える。
    if (this.usingHeadShardOnly()) {
      const hint = document.createElement("p");
      hint.className = "dt-search-interpretation dt-index-loading-hint";
      hint.textContent = t("fullIndexLoadingHint");
      wrap.appendChild(hint);
    }

    wrap.appendChild(this.renderSearchTier(t("tierExact"), result.exact, t("searchTierEmpty")));
    wrap.appendChild(this.renderSearchTier(t("tierSameConcept"), result.sameConcept, t("searchTierEmpty")));
    wrap.appendChild(this.renderSearchTier(t("tierSpecialization"), result.specialization, t("searchTierEmpty")));
    // related段階だけは別ファイル（10.57MB）の到着待ちになりうるので、
    // 空の理由が「まだ読み込み中」なのか「近傍が無い」なのかを区別して出す。
    const relatedEmpty =
      this.relatedState === "loading"
        ? t("relatedLoading")
        : this.relatedState === "error"
          ? t("relatedLoadFailed")
          : t("relatedNeedsExactMatch");
    wrap.appendChild(this.renderSearchTier(t("tierRelated"), result.related, relatedEmpty));
    // 部分一致は、上の4段階が薄いときの受け皿。全段階が埋まっているときに
    // まで出すと画面が冗長になるので、上位3段階が空のときだけ見せる。
    if (result.exact.length === 0 && result.sameConcept.length === 0 && result.specialization.length === 0) {
      wrap.appendChild(this.renderSearchTier(t("tierClosest"), result.closest, t("searchTierEmpty")));
    }

    if (this.isEmptyResult(result)) {
      const empty = document.createElement("p");
      empty.className = "dt-search-empty";
      empty.textContent = t("searchNoResults");
      wrap.appendChild(empty);
    }

    return wrap;
  }

  /**
   * 「入力をどう解釈したか」を見せる。綴りを直したり機能語を落としたりした
   * ことを黙って行うと、利用者は自分の入力と違う結果が出た理由が分からない
   * ——Googleの「次の検索結果を表示しています」に相当する説明を必ず出す。
   */
  private renderInterpretation(query: string, result: HybridSearchResult): HTMLElement | null {
    const typedWords = query.trim().toLowerCase().split(/\s+/).filter((w) => w.length > 0);
    const interpreted = result.interpretedTokens;
    const changed = result.corrections.length > 0 || interpreted.join(" ") !== typedWords.join(" ");
    if (!changed || interpreted.length === 0) return null;

    const el = document.createElement("p");
    el.className = "dt-search-interpretation";
    const searchedAs = t("searchedAs").replace("{q}", interpreted.join(" "));
    if (result.corrections.length > 0) {
      const pairs = result.corrections.map((c) => `${c.from} → ${c.to}`).join(", ");
      el.textContent = `${t("didYouMean")}: ${pairs} — ${searchedAs}`;
    } else {
      el.textContent = searchedAs;
    }
    return el;
  }

  private isEmptyResult(result: HybridSearchResult): boolean {
    return (
      result.exact.length === 0 &&
      result.sameConcept.length === 0 &&
      result.specialization.length === 0 &&
      result.related.length === 0 &&
      result.closest.length === 0
    );
  }

  private renderSearchTier(label: string, hits: SearchHit[], emptyMessage: string): HTMLElement {
    const section = document.createElement("div");
    section.className = "dt-search-tier";

    const heading = document.createElement("h4");
    heading.className = "dt-search-tier-title";
    heading.textContent = `${label} (${hits.length})`;
    section.appendChild(heading);

    if (hits.length === 0) {
      const empty = document.createElement("p");
      empty.className = "dt-search-tier-empty";
      empty.textContent = emptyMessage;
      section.appendChild(empty);
      return section;
    }

    const list = document.createElement("div");
    list.className = "dt-search-tier-list";
    for (const h of hits) list.appendChild(this.renderSearchHitRow(h));
    section.appendChild(list);
    return section;
  }

  /**
   * 検索結果の1行。フレーズ本体は`<button>`（クリックでその語へ検索を
   * ピボット、従来どおり）だが、出典arXivリンクは実際の`<a>`要素として
   * 別に置く——ボタンの中にリンクを入れる（`<button><a>…</a></button>`）
   * のはHTML仕様上不正な入れ子（インタラクティブ要素は入れ子にできない）
   * で、クリックイベントもボタン側へ伝播してリンク単体を押せなくなる。
   */
  private renderSearchHitRow(h: SearchHit): HTMLElement {
    const row = document.createElement("div");
    row.className = "dt-search-hit";

    const main = document.createElement("button");
    main.type = "button";
    main.className = "dt-search-hit-main";
    // 分野集中度は、判定に足る論文数があった候補にだけ付く（大半は null）。
    const conc =
      h.fieldConcentration === null
        ? ""
        : `<span class="dt-search-hit-conc" title="${escapeHtml(t("concentrationHint"))}">${h.fieldConcentration.toFixed(2)}</span>`;
    main.innerHTML = `
      <span class="dt-search-hit-phrase">${escapeHtml(h.phrase)}</span>
      <span class="dt-search-hit-freq">${h.docFreq}</span>
      <span class="dt-search-hit-msc">${escapeHtml(h.mscCode ?? "-")}</span>
      ${conc}
    `;
    main.onclick = () => {
      this.view = { ...this.view, query: h.phrase };
      this.render();
    };
    row.appendChild(main);

    const mapToggle = document.createElement("button");
    mapToggle.type = "button";
    mapToggle.className = "dt-map-toggle";
    const isOpen = this.mapFocus === h.phrase;
    mapToggle.textContent = isOpen ? t("hideConceptMap") : t("showConceptMap");
    mapToggle.setAttribute("aria-expanded", String(isOpen));
    mapToggle.onclick = () => {
      this.mapFocus = isOpen ? null : h.phrase;
      this.ensureRelatedEdges();
      this.ensureSamplePapers();
      this.render();
    };
    row.appendChild(mapToggle);

    if (this.mapFocus === h.phrase) {
      row.appendChild(this.renderConceptMapPanel(h.phrase));
    }

    // 表記ゆれを畳んだ事実をその場で見せる。"elliptic curves" の行に
    // 「＋ elliptic curve, Elliptic Curves」と出ることで、利用者が単数形で
    // 打ったのに複数形の行が返ってきた理由がその場で分かる。
    const variants = this.aliases[h.phrase];
    if (variants !== undefined && variants.length > 0) {
      const el = document.createElement("p");
      el.className = "dt-search-hit-aliases";
      el.textContent = `${t("aliasesLabel")} ${variants.join(", ")}`;
      row.appendChild(el);
    }

    const relations = this.relations[h.phrase];
    if (relations !== undefined && relations.length > 0) {
      row.appendChild(this.renderTypedRelations(relations));
    }

    const papers = this.conceptPapers[h.phrase];
    if (papers !== undefined && papers.length > 0) {
      row.appendChild(this.renderConceptPapers(papers));
    } else if (this.papersState === "loading") {
      const loading = document.createElement("p");
      loading.className = "dt-search-hit-papers-loading";
      loading.textContent = t("samplePapersLoading");
      row.appendChild(loading);
    }

    return row;
  }

  /**
   * 型付き関係（特殊化/一般化/同値）を、根拠文つきで表示する。
   *
   * 従来の「特殊化」段階（`search.rs::is_specialization`）は文字列包含
   * だけの判定で、根拠を示せなかった。こちらは分布的統計と実際の論文の
   * 一文から抽出したもので、**その一文そのもの**を必ず一緒に見せる——
   * 実測精度は約50%（当初36%、`relations.rs`冒頭コメント）で、確定した
   * 事実として主張できる水準ではないため、読者が引用元の一文を読んで
   * その場で判断できることを、正しさの根拠そのものにしている。
   */
  private renderTypedRelations(relations: TypedRelationView[]): HTMLElement {
    const box = document.createElement("div");
    box.className = "dt-search-hit-relations";

    const label = document.createElement("span");
    label.className = "dt-search-hit-relations-label";
    label.textContent = t("typedRelationsLabel");
    box.appendChild(label);

    const list = document.createElement("ul");
    list.className = "dt-relation-list";
    // Confirmed→Grounded、確信度降順（既にexport側でこの順に並んでいる
    // ので追加のソートはしない）。
    for (const r of relations) {
      const item = document.createElement("li");
      item.className = "dt-relation";

      const head = document.createElement("div");
      head.className = "dt-relation-head";
      const relationLabel =
        r.relation === "broader" ? t("relationBroader") : r.relation === "narrower" ? t("relationNarrower") : t("relationEquivalent");
      const badge = document.createElement("span");
      badge.className = `dt-relation-badge dt-relation-badge-${r.status}`;
      badge.textContent = r.status === "confirmed" ? t("relationConfirmed") : t("relationGrounded");
      badge.title = t("relationBadgeHint");
      head.append(`${relationLabel} `, this.makeRelationTargetButton(r.other), " ", badge);
      item.appendChild(head);

      const evidence = document.createElement("p");
      evidence.className = "dt-relation-evidence";
      const link = document.createElement("a");
      link.href = `https://arxiv.org/abs/${encodeURIComponent(r.evidenceArxivId)}`;
      link.target = "_blank";
      link.rel = "noopener";
      link.textContent = r.evidenceArxivId;
      link.onclick = (ev) => ev.stopPropagation();
      evidence.append(`"${r.evidenceSentence}" — `, link);
      item.appendChild(evidence);

      // `r.assertionId`は`relations.json`自体が証拠層から生成される
      // ようになった（P2、`docs/P2_STATUS.md`）ため常に存在する——クリックで
      // `provenancePanel.ts`の詳細ダイアログを開く（外部レビュー
      // 2026-09-05提案4）。
      const provBtn = document.createElement("button");
      provBtn.type = "button";
      provBtn.className = "dt-relation-provenance";
      provBtn.textContent = `Provenance: assertion #${r.assertionId}`;
      provBtn.onclick = (ev) => {
        ev.stopPropagation();
        void showAssertionDetail(r.assertionId);
      };
      item.appendChild(provBtn);

      list.appendChild(item);
    }
    box.appendChild(list);
    return box;
  }

  private makeRelationTargetButton(phrase: string): HTMLElement {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "dt-relation-target";
    btn.textContent = phrase;
    btn.onclick = (ev) => {
      ev.stopPropagation();
      this.view = { ...this.view, query: phrase };
      this.render();
    };
    return btn;
  }

  /**
   * 「この語は実際にどの論文で使われているか」を、**題名つきの文書**として
   * 示す。以前はarXiv IDの文字列を並べるだけで、利用者は中身を見るために
   * 必ずサイトの外へ出る必要があった。
   */
  /**
   * 「地図で見る」パネル（診断⑤への対応）。`relatedEdges`（コサイン
   * 類似度）と`relations`（特殊化/同値）はどちらも検索するまで遅延
   * 読み込みされる既存のデータ——読み込み中は地図の代わりに理由つきの
   * 待機表示を出す（`ensureRelatedEdges`と同じ方針）。
   */
  private renderConceptMapPanel(focus: string): HTMLElement {
    if (this.relatedState !== "ready") {
      const waiting = document.createElement("p");
      waiting.className = "dt-concept-map-hint";
      waiting.textContent = this.relatedState === "error" ? t("relatedLoadFailed") : t("relatedLoading");
      return waiting;
    }
    const map = buildConceptMap({
      focus,
      relatedEdges: this.relatedEdges,
      relations: this.relations,
      docFreqOf: (phrase) => this.docFreqOf(phrase),
    });
    return renderConceptMap(map, (newFocus) => {
      this.mapFocus = newFocus;
      this.render();
    });
  }

  /** 任意のフレーズ（検索結果に無い2ホップ先の近傍も含む）の文書頻度を引く。 */
  private docFreqOf(phrase: string): number {
    const index = this.searchIndex ?? this.headSearchIndex;
    if (index === null) return 0;
    const ids = index.byPhrase.get(splitWords(phrase).join(" "));
    return ids !== undefined && ids.length > 0 ? index.entries[ids[0]].docFreq : 0;
  }

  private renderConceptPapers(papers: ConceptPaper[]): HTMLElement {
    const box = document.createElement("div");
    box.className = "dt-search-hit-papers";

    const label = document.createElement("span");
    label.className = "dt-search-hit-papers-label";
    label.textContent = t("samplePapersLabel");
    box.appendChild(label);

    const list = document.createElement("ul");
    list.className = "dt-paper-list";
    for (const paper of papers) {
      const item = document.createElement("li");
      item.className = "dt-paper";

      const link = document.createElement("a");
      link.className = "dt-paper-title";
      link.href = `https://arxiv.org/abs/${encodeURIComponent(paper.arxivId)}`;
      link.target = "_blank";
      link.rel = "noopener";
      link.textContent = paper.title;
      // フレーズ本体の`<button>`とは兄弟要素なのでイベント伝播の心配は
      // 無いが、将来行全体をクリック領域にする変更に備えて明示しておく。
      link.onclick = (ev) => ev.stopPropagation();
      item.appendChild(link);

      const meta = document.createElement("span");
      meta.className = "dt-paper-meta";
      const bits = [paper.arxivId];
      if (paper.year !== null) bits.push(String(paper.year));
      if (paper.primaryCategory !== null) bits.push(paper.primaryCategory);
      meta.textContent = bits.join(" · ");
      item.appendChild(meta);

      list.appendChild(item);
    }
    box.appendChild(list);
    return box;
  }

  private renderStats(d: TaxonomyShell): HTMLElement {
    const el = document.createElement("div");
    el.className = "dt-stats";
    el.textContent =
      `${d.paperCount.toLocaleString()} papers → ${d.candidateCount.toLocaleString()} candidates → ` +
      `${d.resolvedConceptCount.toLocaleString()} concepts (aliases folded) → ` +
      `${d.clusterCount.toLocaleString()} clusters (${d.fields.length} MSC fields, ${d.novelClusters.length} novel, ${d.ambiguousClusterCount} ambiguous) · ` +
      `${t("generatedAtLabel")}: ${formatGeneratedAt(d.generatedAtUnix)}`;
    return el;
  }

  private renderTabs(): HTMLElement {
    const el = document.createElement("div");
    el.className = "dt-tabs";
    const fieldsBtn = document.createElement("button");
    fieldsBtn.className = `dt-tab ${this.view.tab === "fields" ? "active" : ""}`;
    fieldsBtn.textContent = t("tabByField");
    fieldsBtn.onclick = () => {
      this.view = { ...this.view, tab: "fields", field: null, openCluster: null };
      this.render();
    };
    const novelBtn = document.createElement("button");
    novelBtn.className = `dt-tab ${this.view.tab === "novel" ? "active" : ""}`;
    novelBtn.textContent = t("tabNovel");
    novelBtn.onclick = () => {
      this.view = { ...this.view, tab: "novel", field: null, openCluster: null };
      this.render();
    };
    el.append(fieldsBtn, novelBtn);
    return el;
  }

  private renderFieldGrid(fields: ExportedField[]): HTMLElement {
    const grid = document.createElement("div");
    grid.className = "dt-grid";
    for (const f of fields) {
      const card = document.createElement("button");
      card.className = "dt-card dt-field-card";
      card.innerHTML = `
        <div class="dt-card-code">${escapeHtml(f.code)}</div>
        <div class="dt-card-title">${escapeHtml(f.name)}</div>
        <div class="dt-card-meta">${f.clusterCount} ${t("clustersLabel")} · ${f.conceptCount} ${t("conceptsLabel")}</div>
      `;
      card.onclick = () => {
        this.view = { ...this.view, tab: "fields", field: f, openCluster: null };
        this.render();
      };
      grid.appendChild(card);
    }
    return grid;
  }

  private renderFieldDetail(field: ExportedField): HTMLElement {
    const wrap = document.createElement("div");

    const backBtn = document.createElement("button");
    backBtn.className = "dt-back";
    backBtn.textContent = t("back");
    backBtn.onclick = () => {
      this.view = { ...this.view, tab: "fields", field: null, openCluster: null };
      this.render();
    };
    wrap.appendChild(backBtn);

    const heading = document.createElement("h3");
    heading.className = "dt-field-heading";
    heading.textContent = `${field.code}  ${field.name}`;
    wrap.appendChild(heading);

    wrap.appendChild(this.renderClusterGrid(field.clusters, false));
    return wrap;
  }

  private renderClusterGrid(clusters: ExportedCluster[], novel: boolean): HTMLElement {
    const grid = document.createElement("div");
    grid.className = "dt-cluster-grid";
    for (const c of clusters) {
      const card = document.createElement("div");
      card.className = "dt-cluster-card";

      const headBtn = document.createElement("button");
      headBtn.className = "dt-cluster-head";
      const title = novel
        ? (c.members[0]?.phrase ?? `#${c.id}`)
        : `${escapeHtml(c.dominantCode ?? "")} ${escapeHtml(c.dominantName ?? "")}`;
      const confidenceHtml = novel ? "" : `<span class="dt-confidence">${Math.round(c.confidence * 100)}% ${t("confidenceLabel")}</span>`;
      headBtn.innerHTML = `
        <span class="dt-cluster-title">${novel ? escapeHtml(title) : title}</span>
        <span class="dt-cluster-size">${c.size} ${t("membersLabel")}</span>
        ${confidenceHtml}
      `;
      const isOpen = this.view.openCluster === c.id;
      headBtn.onclick = () => {
        this.view = { ...this.view, openCluster: isOpen ? null : c.id };
        this.render();
      };
      card.appendChild(headBtn);

      if (isOpen) {
        card.appendChild(this.renderMembers(c, novel));
      }

      grid.appendChild(card);
    }
    return grid;
  }

  private renderMembers(cluster: ExportedCluster, novel: boolean): HTMLElement {
    const box = document.createElement("div");
    box.className = "dt-members";
    if (novel) {
      const hint = document.createElement("p");
      hint.className = "dt-novel-hint";
      hint.textContent = t("novelClusterHint");
      box.appendChild(hint);
    }
    const rows = cluster.members
      .map(
        (m) => `
        <div class="dt-member-row">
          <span class="dt-member-phrase">${escapeHtml(m.phrase)}</span>
          <span class="dt-member-freq">${m.docFreq}</span>
          <span class="dt-member-msc">${escapeHtml(m.mscCode ?? "-")}</span>
        </div>`,
      )
      .join("");
    box.innerHTML += rows;
    return box;
  }
}

/**
 * 列形式＋添字参照で届いた出典論文を、フレーズ引きのマップへ展開する。
 * 論文本体は1回しか送られてこないので、ここで題名つきのオブジェクトに
 * 組み直す（同じ論文オブジェクトを複数の概念が共有する——コピーしない）。
 */
function expandPapers(columns: PapersExport): Record<string, ConceptPaper[]> {
  const docs = new Array<ConceptPaper>(columns.arxivId.length);
  for (let i = 0; i < columns.arxivId.length; i++) {
    docs[i] = {
      arxivId: columns.arxivId[i],
      title: columns.title[i],
      year: columns.year[i],
      primaryCategory: columns.primaryCategory[i],
    };
  }
  const map: Record<string, ConceptPaper[]> = {};
  for (let i = 0; i < columns.source.length; i++) {
    map[columns.source[i]] = columns.papers[i].map((idx) => docs[idx]);
  }
  return map;
}

/** 列形式で届いた表記ゆれを、代表表記引きのマップへ展開する。 */
function expandAliases(columns: AliasExport): Record<string, string[]> {
  const map: Record<string, string[]> = {};
  for (let i = 0; i < columns.representative.length; i++) {
    map[columns.representative[i]] = columns.aliases[i];
  }
  return map;
}

/**
 * 1件の型付き関係を、片方の概念から見た形に向き付けたもの。`assertionId`は
 * `ProvenanceRelationEdge`からそのまま引き継ぐ——`relations.json`自体が
 * 証拠層から生成されるようになった（P2、`docs/P2_STATUS.md`）ので、以前の
 * ような向き復元による突き合わせなしに、この1件だけで出典まで辿れる。
 */
export interface TypedRelationView {
  assertionId: number;
  other: string;
  /**
   * "narrower" = otherはこの概念**より特殊**（この概念はotherを一般化）。
   * "broader"  = otherはこの概念**より一般的**（この概念はotherの特殊化）。
   * "equivalent" = 同値。
   */
  relation: "narrower" | "broader" | "equivalent";
  status: "confirmed" | "grounded";
  confidence: number | null;
  evidenceSentence: string;
  evidenceArxivId: string;
}

/**
 * 行形式の`relations.json`を、フレーズ引きの双方向リストへ展開する。
 * specialization_ofは両端から見え方が違う——subject側からは「objectの方が
 * 広い」、object側からは「subjectの方が狭い」。
 */
function expandRelations(edges: ProvenanceRelationEdge[]): Record<string, TypedRelationView[]> {
  const map: Record<string, TypedRelationView[]> = {};
  const push = (phrase: string, view: TypedRelationView) => {
    (map[phrase] ??= []).push(view);
  };
  for (const e of edges) {
    const { subject, object, status, confidence, evidenceSentence, evidenceArxivId, assertionId } = e;
    if (e.kind === "equivalent_to") {
      push(subject, { assertionId, other: object, relation: "equivalent", status, confidence, evidenceSentence, evidenceArxivId });
      push(object, { assertionId, other: subject, relation: "equivalent", status, confidence, evidenceSentence, evidenceArxivId });
    } else {
      push(subject, { assertionId, other: object, relation: "broader", status, confidence, evidenceSentence, evidenceArxivId });
      push(object, { assertionId, other: subject, relation: "narrower", status, confidence, evidenceSentence, evidenceArxivId });
    }
  }
  return map;
}

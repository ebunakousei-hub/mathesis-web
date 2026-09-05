import type { KernelStore } from "mathesis-wasm";
import { DynamicTaxonomyExplorer } from "./dynamicTaxonomy";
import { findLabelById } from "./fields";
import { t, getLang, setLang, label, type Lang } from "./i18n";
import { Layer35Panel } from "./layer35";
import { LeanPlaygroundExplorer } from "./leanPlayground";
import { ProofGraphExplorer } from "./proofGraph";
import { tokenizeIdentifier } from "./proofSearch";
import { looksLikeTex, renderTex, texToSearchTerms, type TexTerms } from "./tex";
import type { JudgmentView, LeanParseResult } from "./types";
import { escapeHtml } from "./util";

interface ParsedStatement {
  display: string;
  canonical_hash: string;
  status: "full" | "partial" | "failed";
}

const app = document.querySelector<HTMLDivElement>("#app")!;

app.innerHTML = `
  <header>
    <div>
      <h1>${t("title")}</h1>
    </div>
    <div class="lang-toggle" id="lang-toggle">
      <button data-lang="ja">日本語</button>
      <button data-lang="en">English</button>
    </div>
  </header>
  <p class="tagline" id="tagline">${t("tagline")}</p>

  <input class="search-box" id="search-box" type="text" placeholder="${t("searchPlaceholder")}" autocomplete="off" />
  <p class="search-hint" id="search-hint">${t("texHint")}</p>
  <div class="math-preview" id="math-preview" hidden>
    <div class="math-preview-label" id="math-preview-label">${t("texPreviewLabel")}</div>
    <div class="math-preview-body" id="math-preview-body"></div>
    <div class="math-preview-terms" id="math-preview-terms"></div>
  </div>
  <div class="parse-panel" id="parse-panel"></div>

  <section>
    <h2 id="explorer-title">${t("explorerTitle")}</h2>
    <p class="section-sub" id="explorer-explain">${t("explorerExplain")}</p>
    <div id="dynamic-taxonomy-root"></div>
  </section>

  <section>
    <h2 id="proof-graph-title">${t("proofGraphTitle")}</h2>
    <p class="section-sub" id="proof-graph-explain">${t("proofGraphExplain")}</p>
    <div id="proof-graph-root"></div>
  </section>

  <section>
    <h2 id="lean-playground-title">${t("leanPlaygroundTitle")}</h2>
    <p class="section-sub" id="lean-playground-explain">${t("leanPlaygroundExplain")}</p>
    <div id="lean-playground-root"></div>
  </section>

  <section>
    <h2 id="demo-title">${t("demoTitle")}</h2>
    <p class="section-sub" id="demo-explain">${t("demoExplain")}</p>
    <div id="demo-table-root"></div>
  </section>

  <section>
    <h2 id="layer35-title">${t("layer35Title")}</h2>
    <p class="section-sub" id="layer35-explain">${t("layer35Explain")}</p>
    <div id="layer35-root"></div>
  </section>
`;

const parsePanel = document.querySelector<HTMLDivElement>("#parse-panel")!;
const searchBox = document.querySelector<HTMLInputElement>("#search-box")!;
const mathPreview = document.querySelector<HTMLDivElement>("#math-preview")!;
const mathPreviewBody = document.querySelector<HTMLDivElement>("#math-preview-body")!;
const mathPreviewTerms = document.querySelector<HTMLDivElement>("#math-preview-terms")!;
const demoTableRoot = document.querySelector<HTMLDivElement>("#demo-table-root")!;
const dynamicTaxonomyRoot = document.querySelector<HTMLDivElement>("#dynamic-taxonomy-root")!;
const proofGraphRoot = document.querySelector<HTMLDivElement>("#proof-graph-root")!;
const leanPlaygroundRoot = document.querySelector<HTMLDivElement>("#lean-playground-root")!;
const layer35Root = document.querySelector<HTMLDivElement>("#layer35-root")!;

let store: KernelStore | null = null;
/**
 * wasmカーネルの `parse_statement`。読み込みに成功するまで、そして
 * 失敗したままなら永久に `null`。
 *
 * wasmは静的importではなく `bootstrap()` の中で動的importする。静的import
 * のままだと、wasmモジュールの解決・取得に失敗した時点でこのファイル自体が
 * 実行されず、`app.innerHTML = ...` にすら到達せずに**ページ全体が真っ白**に
 * なる（実際にこの環境で `node_modules` のリンクが切れた際に発生した）。
 * 概念エクスプローラー（80,727概念）も証明グラフ（1,431判断）もwasmを
 * 一切使わないのに、112KBの任意機能の失敗で全部が消えるのは配信物として
 * 割に合わない。CDNの不調・WebAssembly非対応・プロキシによる
 * application/wasm のブロック・厳しいCSP、どれでも同じことが起きる。
 */
let parseStatement: ((input: string) => ParsedStatement) | null = null;
let wasmFailed = false;
let layer35: Layer35Panel | null = null;
const dynamicTaxonomy = new DynamicTaxonomyExplorer(dynamicTaxonomyRoot);
const proofGraph = new ProofGraphExplorer(proofGraphRoot);
// `parseLeanSource` もwasm側の関数なので、wasmが読み込み終わるまでは
// `null`のまま——コンストラクタはそれを織り込み済みで「読み込み中」を表示する。
const leanPlayground = new LeanPlaygroundExplorer(leanPlaygroundRoot, null);

/**
 * 画面の状態（検索語・開いている区画）をURLのハッシュに写す。
 *
 * これが無かった頃、この探索UIは**見つけたものを誰にも渡せなかった**——
 * URLは何をしても `http://localhost:5173/` のままで、検索結果を共有する
 * ことも、ブックマークすることも、ブラウザの戻るで一つ前の検索に戻る
 * こともできず、再読み込みすれば全部消えた。10万論文から概念を掘り当てる
 * ためのUIとしては、掘り当てた先を指し示せないのは致命的に近い。
 *
 * 状態はハッシュに置く（`?q=` ではなく `#q=`）。静的ホスティングでも
 * サーバ設定なしにそのまま動き、リロードで404にならないため。
 */
interface UrlState {
  q: string;
  pg: string;
}

function readUrlState(): UrlState {
  const params = new URLSearchParams(location.hash.replace(/^#/, ""));
  return {
    q: params.get("q") ?? "",
    pg: params.get("pg") ?? "",
  };
}

/** URLの書き換えで自分自身の `hashchange` に反応しないための番兵。 */
let writingUrl = false;

function writeUrlState(): void {
  const params = new URLSearchParams();
  const conceptQuery = document.querySelector<HTMLInputElement>("#dt-search-input")?.value ?? "";
  const proofQuery = document.querySelector<HTMLInputElement>("#pg-search-input")?.value ?? "";
  if (conceptQuery.trim().length > 0) params.set("q", conceptQuery.trim());
  if (proofQuery.trim().length > 0) params.set("pg", proofQuery.trim());

  const hash = params.toString();
  const next = `${location.pathname}${location.search}${hash.length > 0 ? `#${hash}` : ""}`;
  if (next === `${location.pathname}${location.search}${location.hash}`) return;

  writingUrl = true;
  // 打鍵のたびに履歴を積むと「戻る」が1文字ずつ戻る羽目になるので、
  // 同じ探索の続きは置き換え、区画の切り替えだけを履歴に残す。
  history.replaceState(null, "", next);
  writingUrl = false;
}

function applyUrlState(state: UrlState): void {
  if (state.q.length > 0) dynamicTaxonomy.focusQuery(state.q);
  if (state.pg.length > 0) proofGraph.focusQuery(state.pg);
}

window.addEventListener("hashchange", () => {
  if (writingUrl) return;
  applyUrlState(readUrlState());
});

dynamicTaxonomy.onQueryChange = writeUrlState;
proofGraph.onQueryChange = writeUrlState;

// 系譜で見ている判断から、同じ話題の概念をarXiv 10万論文側へ探しに行く。
// Leanの識別子は `holder_Moser_of_homogeneousWeakSolution` のような合成語
// なので、概念索引に投げる前に語へ割っておく（そのままでは1件も当たらない）。
// 静的な証明グラフ（`proofGraph`）と、その場でパースするLean貼り付け区画
// （`leanPlayground`）の両方から同じ導線を使う。
function searchConceptsFromLeanName(name: string): void {
  const query = tokenizeIdentifier(name).filter((w) => w.length >= 3).join(" ");
  if (query.length === 0) return;
  dynamicTaxonomy.focusQuery(query);
  writeUrlState();
  dynamicTaxonomyRoot.scrollIntoView({ behavior: "smooth", block: "start" });
}
proofGraph.onSearchConcepts = searchConceptsFromLeanName;
leanPlayground.onSearchConcepts = searchConceptsFromLeanName;

// 読み込み直後にURLの状態を反映する。データはまだ来ていないかもしれないが、
// 各区画は検索語を保持したまま、データが届いた時点で描き直す。
applyUrlState(readUrlState());

function renderParseEmpty(): void {
  parsePanel.innerHTML = `<div class="empty">${t("liveParseEmpty")}</div>`;
}

function statusPillHtml(status: string): string {
  const key = status === "full" ? "statusFull" : status === "partial" ? "statusPartial" : "statusFailed";
  return `<span class="status-pill status-${status}">${t(key)}</span>`;
}

function renderParseResult(result: ParsedStatement): void {
  parsePanel.innerHTML = `
    <div class="parse-row"><span class="k">${t("canonicalForm")}</span><span class="v">${escapeHtml(result.display)}</span></div>
    <div class="parse-row"><span class="k">${t("canonicalHash")}</span><span class="v">${result.canonical_hash || "—"}</span></div>
    <div class="parse-row"><span class="k">${t("parseStatus")}</span>${statusPillHtml(result.status)}</div>
  `;
}

function fieldTagsHtml(fields: string[]): string {
  if (fields.length === 0) return "—";
  return fields
    .map((id) => {
      const l = findLabelById(id);
      return `<span class="field-chip">${escapeHtml(l ? label(l) : id)}</span>`;
    })
    .join(" ");
}

function renderDemoTable(): void {
  if (!store) return;
  const judgments = store.listJudgments() as JudgmentView[];
  if (judgments.length === 0) {
    demoTableRoot.innerHTML = `<div class="empty">—</div>`;
    return;
  }
  const rows = judgments
    .map(
      (j) => `
      <tr>
        <td>${escapeHtml(j.name ?? "—")}</td>
        <td>${escapeHtml(j.kind)}</td>
        <td><code>${escapeHtml(j.context.join(", "))}</code></td>
        <td>${escapeHtml(j.statement)}</td>
        <td>${fieldTagsHtml(j.fields)}</td>
        <td><code>${j.canonical_hash.slice(0, 10)}…</code></td>
      </tr>`,
    )
    .join("");
  demoTableRoot.innerHTML = `
    <table class="demo-table">
      <thead>
        <tr>
          <th>${t("colName")}</th>
          <th>${t("colKind")}</th>
          <th>${t("colContext")}</th>
          <th>${t("colStatement")}</th>
          <th>${t("colFields")}</th>
          <th>${t("colHash")}</th>
        </tr>
      </thead>
      <tbody>${rows}</tbody>
    </table>
  `;
}

function applyLangUi(): void {
  const lang = getLang();
  document.documentElement.lang = lang;
  document.querySelectorAll<HTMLButtonElement>("#lang-toggle button").forEach((btn) => {
    btn.classList.toggle("active", btn.dataset.lang === lang);
  });
  document.querySelector("h1")!.textContent = t("title");
  document.querySelector("#tagline")!.textContent = t("tagline");
  searchBox.placeholder = t("searchPlaceholder");
  document.querySelector("#search-hint")!.textContent = t("texHint");
  document.querySelector("#math-preview-label")!.textContent = t("texPreviewLabel");
  document.querySelector("#explorer-title")!.textContent = t("explorerTitle");
  document.querySelector("#explorer-explain")!.textContent = t("explorerExplain");
  document.querySelector("#proof-graph-title")!.textContent = t("proofGraphTitle");
  document.querySelector("#proof-graph-explain")!.textContent = t("proofGraphExplain");
  document.querySelector("#lean-playground-title")!.textContent = t("leanPlaygroundTitle");
  document.querySelector("#lean-playground-explain")!.textContent = t("leanPlaygroundExplain");
  document.querySelector("#demo-title")!.textContent = t("demoTitle");
  document.querySelector("#demo-explain")!.textContent = t("demoExplain");
  document.querySelector("#layer35-title")!.textContent = t("layer35Title");
  document.querySelector("#layer35-explain")!.textContent = t("layer35Explain");

  if (searchBox.value.trim().length === 0) {
    renderParseEmpty();
  } else {
    handleSearchInput();
  }
  dynamicTaxonomy.refreshLanguage();
  proofGraph.refreshLanguage();
  leanPlayground.refreshLanguage();
  layer35?.refreshLanguage();
  renderDemoTable();
}

/**
 * 入力が「数式」なのか「概念・定理を探す言葉」なのかを見分ける。
 *
 * この判定が無かった頃、最上段の検索欄は入力が何であれ数式パーサに
 * 通すだけだった。"riemann hypothesis" と打つと
 * 「正規化された式: riemann hypothesis / パース状態: 完全」と表示され、
 * **一見成功したように見えるのに何も検索していない**——同じページの
 * 下には "riemann hypothesis" を含む概念が実際に存在するのに、
 * 最も目立つ入力欄からはそこへ到達できなかった。欄のラベルは
 * 「定理・数式を検索」だったので、なおさら誤解を招いていた。
 *
 * 関係子・演算子が1つも無い語の並びは、数式ではなく探し物の言葉と見なす
 * （"NP-hard" を式と誤判定しないよう、ハイフンは演算子に数えない）。
 */
function looksLikeFormula(text: string): boolean {
  return /[=<>+*/^∀∃∈∉⊂⊆∧∨¬→↔≠≤≥]/.test(text);
}

const GLOBAL_SEARCH_LIMIT = 5;

/**
 * 組版は非同期（temmlを初回だけ動的importする）なので、打鍵が速いと
 * 古い式の描画が新しい式より後に戻ってくることがある。連番を持って、
 * 追い越された結果は捨てる。
 */
let texRenderSeq = 0;

function hideMathPreview(): void {
  mathPreview.hidden = true;
  texRenderSeq += 1;
}

/**
 * 打ち込んでいるTeXを、そのすぐ下に組版して見せる。あわせて「この式から
 * どんな検索語を引いたか」も並べる——組版だけ見せると、当たらなかったとき
 * に理由が分からないため。何で検索したのかが見えていれば、`
abla` を
 * 足す・`H^1` を足す、と利用者が自分で寄せていける。
 */
async function showMathPreview(tex: string, terms: TexTerms): Promise<void> {
  const seq = ++texRenderSeq;
  mathPreview.hidden = false;

  // 一度離れた要素に組版してから差し替える。同じ要素へ直接描くと、
  // 追い越された古い描画が新しい式を上書きしうる。
  const staging = document.createElement("div");
  const result = await renderTex(staging, tex, false);
  if (seq !== texRenderSeq) return;

  mathPreviewBody.replaceChildren(...staging.childNodes);
  mathPreviewBody.classList.toggle("is-error", result.status === "error");

  mathPreviewTerms.innerHTML = "";
  if (terms.words.length === 0 && terms.symbols.length === 0) {
    const empty = document.createElement("span");
    empty.className = "math-preview-terms-empty";
    empty.textContent = t("texTermsEmpty");
    mathPreviewTerms.appendChild(empty);
    return;
  }
  const label = document.createElement("span");
  label.className = "math-preview-terms-label";
  label.textContent = t("texTermsLabel");
  mathPreviewTerms.appendChild(label);
  for (const w of terms.words) {
    const chip = document.createElement("span");
    chip.className = "term-chip is-word";
    chip.textContent = w;
    mathPreviewTerms.appendChild(chip);
  }
  for (const sym of terms.symbols) {
    const chip = document.createElement("span");
    chip.className = "term-chip is-symbol";
    chip.textContent = sym;
    mathPreviewTerms.appendChild(chip);
  }
}

function renderParseOrHint(value: string): void {
  if (parseStatement === null) {
    parsePanel.innerHTML = `<div class="empty">${escapeHtml(t(wasmFailed ? "wasmUnavailable" : "wasmLoading"))}</div>`;
    return;
  }
  renderParseResult(parseStatement(value));
}

function handleSearchInput(): void {
  const value = searchBox.value.trim();
  if (value.length === 0) {
    hideMathPreview();
    renderParseEmpty();
    return;
  }

  // TeXが最優先。`rac` や `^` を含む入力は、まず組版して見せてから、
  // そこから引いた検索語で探す。
  if (looksLikeTex(value)) {
    const terms = texToSearchTerms(value);
    void showMathPreview(value, terms);
    if (terms.words.length > 0 || terms.symbols.length > 0) {
      // 概念索引（英語の名詞句）に記号を渡しても1件も当たらないので、
      // そちらへ送るのは英単語だけにする。区画側の検索欄にそのまま
      // 表示される文字列でもあるので、当たりようのない記号を並べない。
      renderGlobalSearch(terms.words.join(" "), terms.query);
      return;
    }
    renderParseOrHint(value);
    return;
  }

  hideMathPreview();
  if (looksLikeFormula(value)) {
    renderParseOrHint(value);
    return;
  }
  renderGlobalSearch(value);
}

/**
 * 統合検索の結果。概念タクソノミー（10万論文から抽出した80,727概念）と
 * 証明グラフ（Lean由来の1,431判断）の両方に同じクエリを流し、上位だけを
 * 見せて、クリックでその区画へ送り込む。
 */
/**
 * 統合検索。TeXから翻訳したクエリのときも、各区画へ渡すのは**翻訳後の語**
 * にする——区画側の検索欄に `\int_\Omega` をそのまま入れても、そちらの
 * 索引（英語の概念名 / Leanの識別子）には1件も当たらないため。
 */
function renderGlobalSearch(conceptQuery: string, judgmentQuery: string = conceptQuery): void {
  const { hits: concepts, corrections } = dynamicTaxonomy.topHits(conceptQuery, GLOBAL_SEARCH_LIMIT);
  const judgments = proofGraph.topHits(judgmentQuery, GLOBAL_SEARCH_LIMIT);

  if (concepts.length === 0 && judgments.length === 0) {
    // どちらにも無い場合だけ、数式として解釈した結果を出す（入力が
    // 本当に未知の記号列だったときの逃げ道として残す）。
    parsePanel.innerHTML = `<div class="global-search-empty">${t("globalSearchEmpty")}</div>`;
    return;
  }

  parsePanel.innerHTML = "";
  const wrap = document.createElement("div");
  wrap.className = "global-search";

  if (concepts.length > 0) {
    // 綴り訂正が起きたことを、区画側と同じ文言でここにも出す。以前は
    // ここで訂正情報を捨てていたため、例えば「Campanato」（コーパスに
    // 無い語）と打つと、無関係な「campana」が理由の説明なしに並んで
    // いた——区画を開けば「もしかして: campanato → campana」と出るのに、
    // 統合検索のプレビューだけがそれを隠していた。
    if (corrections.length > 0) {
      const hint = document.createElement("p");
      hint.className = "global-search-correction";
      const pairs = corrections.map((c) => `${c.from} → ${c.to}`).join(", ");
      hint.textContent = `${t("didYouMean")}: ${pairs}`;
      wrap.appendChild(hint);
    }
    wrap.appendChild(
      globalSearchGroup(t("globalSearchConcepts"), concepts.map((c) => ({ label: c.phrase, meta: `${c.docFreq}` })), () => {
        dynamicTaxonomy.focusQuery(conceptQuery);
        // 区画の検索欄を直接打ったときと同じようにURLへ写す。統合検索
        // 経由で辿り着いた画面だけURLが空のままだと、そこで見つけたものを
        // 共有できない（`focusQuery` は区画側の入力イベントを通らないので
        // `onQueryChange` が鳴らない）。
        writeUrlState();
        dynamicTaxonomyRoot.scrollIntoView({ behavior: "smooth", block: "start" });
      }),
    );
  }
  if (judgments.length > 0) {
    wrap.appendChild(
      globalSearchGroup(
        t("globalSearchJudgments"),
        judgments.map((j) => ({ label: j.name ?? j.statement, meta: j.kind })),
        () => {
          proofGraph.focusQuery(judgmentQuery);
          writeUrlState();
          proofGraphRoot.scrollIntoView({ behavior: "smooth", block: "start" });
        },
      ),
    );
  }
  parsePanel.appendChild(wrap);
}

function globalSearchGroup(
  title: string,
  rows: { label: string; meta: string }[],
  onOpen: () => void,
): HTMLElement {
  const group = document.createElement("div");
  group.className = "global-search-group";

  const head = document.createElement("div");
  head.className = "global-search-head";
  head.textContent = title;
  group.appendChild(head);

  for (const row of rows) {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "global-search-row";
    item.innerHTML = `<span class="global-search-label">${escapeHtml(row.label)}</span><span class="global-search-meta">${escapeHtml(row.meta)}</span>`;
    item.onclick = onOpen;
    group.appendChild(item);
  }

  const more = document.createElement("button");
  more.type = "button";
  more.className = "global-search-more";
  more.textContent = t("globalSearchOpen");
  more.onclick = onOpen;
  group.appendChild(more);

  return group;
}

document.querySelector<HTMLDivElement>("#lang-toggle")!.addEventListener("click", (ev) => {
  const target = ev.target as HTMLElement;
  const lang = target.dataset.lang as Lang | undefined;
  if (!lang) return;
  setLang(lang);
  applyLangUi();
});

searchBox.addEventListener("input", handleSearchInput);

async function bootstrap(): Promise<void> {
  const wasm = await import("mathesis-wasm");
  await wasm.default();
  parseStatement = (input: string) => wasm.parse_statement(input) as ParsedStatement;

  store = new wasm.KernelStore();
  store.seedDemoJudgments();

  layer35 = new Layer35Panel(layer35Root, store, wasm.KernelStore.wellKnownStrategies());
  layer35.setJudgments(store.listJudgments() as JudgmentView[]);

  leanPlayground.setParseFn((source: string) => wasm.parseLeanSource(source) as LeanParseResult);

  renderParseEmpty();
  renderDemoTable();
  applyLangUi();
}

bootstrap().catch((err) => {
  // ここに来ても、ページの骨格・概念エクスプローラー・証明グラフは既に
  // 動いている。失われるのは数式のライブ解析・Lean貼り付け・手書きデモの
  // 各区画だけなので、それらにだけ理由を出して、残りはそのまま使わせる。
  console.error("Failed to initialize Mathesis kernel:", err);
  wasmFailed = true;
  renderWasmUnavailable();
});

function renderWasmUnavailable(): void {
  const notice = `<div class="empty">${escapeHtml(t("wasmUnavailable"))}</div>`;
  parsePanel.innerHTML = notice;
  demoTableRoot.innerHTML = notice;
  layer35Root.innerHTML = notice;
  leanPlaygroundRoot.innerHTML = notice;
}

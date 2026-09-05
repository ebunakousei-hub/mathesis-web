import { t } from "./i18n";
import { computeChainDepths, type LineageGraph } from "./lineage";
import { LineageView } from "./lineageView";
import type { ExportedJudgment, LeanParsedJudgment, LeanParseResult } from "./types";
import { escapeHtml, unwrapLeanSymbols } from "./util";

/**
 * このデモの実データそのもの（`fixtures/arxiv/DeGiorgi/DeGiorgi/BallExtension/Core.lean`
 * 冒頭）から取った、3件の`def`が実際に依存し合っている短い抜粋。
 * 「初めて使う人が何も貼らずにまず動きを見る」ための既定値であって、
 * 説明のための作り話ではない——[[never-fabricate-math-notation]]と同じ
 * 理由で、ここも実データ以外は入れない。
 */
const EXAMPLE_SOURCE = `def unitBallRetraction (x : E) : E :=
  if ‖x‖ ≤ 1 then
    x
  else if ‖x‖ < 2 then
    (‖x‖ ^ (2 : ℕ))⁻¹ • x
  else
    0

def unitBallCutoff (x : E) : ℝ :=
  min 1 (max (2 - ‖x‖) 0)

def unitBallExtension (u : E → ℝ) (x : E) : ℝ :=
  unitBallCutoff x * u (unitBallRetraction x)
`;

const DEBOUNCE_MS = 150;

/**
 * Leanのソースを貼り付けると、その場で判断ノードと依存関係を抽出して
 * 見せる区画。
 *
 * これまで「証明が何に依拠するかを追える」体験（`lineageView.ts`）は
 * CLIで事前にインポートした1件の実データ（DeGiorgiコーパス、1,431判断）
 * でしか見られなかった。ここは`README.md`の「今後の課題」に残っていた
 * ギャップ——「`mathesis-importer`のLeanパーサーは今のところCLIバイナリ
 * 内に閉じている」——を埋める区画で、CLIが使っているのと**全く同じ
 * パーサー**（`mathesis-lean-parse`、`mathesis-graph`に依存しないよう
 * 切り出した）を`mathesis-wasm`経由でブラウザから直接呼ぶ。
 *
 * 静的な`judgments.json`を読む`ProofGraphExplorer`とは別物——あちらは
 * ページを開いた時点で1,431件が既に揃っているが、こちらは利用者が
 * 貼り付けるたびに、その貼り付け内だけで閉じた小さなグラフを毎回
 * 一から作り直す。データが小さい（せいぜい数十〜数百判断）ので、
 * `DynamicTaxonomyExplorer`のような索引の使い回しは要らない——打鍵の
 * たびに`LineageGraph`を素朴に組み直しても十分速い。
 */
export class LeanPlaygroundExplorer {
  private root: HTMLElement;
  private parseFn: ((source: string) => LeanParseResult) | null;

  private source = "";
  private result: LeanParseResult = { judgments: [], dependencies: [] };
  private selectedId: number | null = null;
  private debounceHandle: ReturnType<typeof setTimeout> | null = null;

  private lineageRoot = document.createElement("div");
  private lineageView: LineageView | null = null;

  /** 貼り付けたLeanの判断名から、関係する概念をタクソノミー側へ探しに行く。 */
  onSearchConcepts: ((query: string) => void) | null = null;

  constructor(root: HTMLElement, parseFn: ((source: string) => LeanParseResult) | null) {
    this.root = root;
    this.parseFn = parseFn;
    this.render();
  }

  refreshLanguage(): void {
    this.render();
    this.lineageView?.refreshLanguage();
  }

  /** wasmの読み込みが後から終わったとき（`main.ts::bootstrap`参照）に呼ぶ。 */
  setParseFn(parseFn: (source: string) => LeanParseResult): void {
    this.parseFn = parseFn;
    this.reparse();
  }

  private reparse(): void {
    if (this.parseFn === null || this.source.trim().length === 0) {
      this.result = { judgments: [], dependencies: [] };
    } else {
      this.result = this.parseFn(this.source);
    }
    // 編集で消えた判断を選んだままにしない。
    if (this.selectedId !== null && !this.result.judgments.some((j) => j.id === this.selectedId)) {
      this.selectedId = null;
    }
    this.render();
  }

  private scheduleReparse(): void {
    if (this.debounceHandle !== null) clearTimeout(this.debounceHandle);
    this.debounceHandle = setTimeout(() => this.reparse(), DEBOUNCE_MS);
  }

  private render(): void {
    const wrap = document.createElement("div");
    wrap.className = "lp-wrap";

    wrap.appendChild(this.renderInput());

    if (this.parseFn === null) {
      const empty = document.createElement("p");
      empty.className = "lp-empty";
      empty.textContent = t("wasmLoading");
      wrap.appendChild(empty);
    } else if (this.source.trim().length === 0) {
      const empty = document.createElement("p");
      empty.className = "lp-empty";
      empty.textContent = t("leanPlaygroundEmpty");
      wrap.appendChild(empty);
    } else {
      wrap.appendChild(this.renderStats());
      wrap.appendChild(this.renderJudgmentList());
      if (this.selectedId !== null) {
        wrap.appendChild(this.renderSelectedLineage());
      }
    }

    this.root.innerHTML = "";
    this.root.appendChild(wrap);
  }

  private renderInput(): HTMLElement {
    const box = document.createElement("div");
    box.className = "lp-input-box";

    const textarea = document.createElement("textarea");
    textarea.className = "lp-textarea";
    textarea.placeholder = t("leanPlaygroundPlaceholder");
    textarea.value = this.source;
    textarea.spellcheck = false;
    textarea.oninput = () => {
      this.source = textarea.value;
      this.scheduleReparse();
    };
    box.appendChild(textarea);

    const controls = document.createElement("div");
    controls.className = "lp-controls";

    const exampleBtn = document.createElement("button");
    exampleBtn.type = "button";
    exampleBtn.className = "lp-ctl";
    exampleBtn.textContent = t("leanPlaygroundExampleButton");
    exampleBtn.onclick = () => {
      this.source = EXAMPLE_SOURCE;
      this.reparse();
    };
    controls.appendChild(exampleBtn);

    if (this.source.length > 0) {
      const clearBtn = document.createElement("button");
      clearBtn.type = "button";
      clearBtn.className = "lp-ctl";
      clearBtn.textContent = t("leanPlaygroundClear");
      clearBtn.onclick = () => {
        this.source = "";
        this.reparse();
      };
      controls.appendChild(clearBtn);
    }

    box.appendChild(controls);
    return box;
  }

  private renderStats(): HTMLElement {
    const p = document.createElement("p");
    p.className = "lp-stats";
    if (this.result.judgments.length === 0) {
      p.textContent = t("leanPlaygroundNoDeclarations");
      return p;
    }
    p.textContent = t("leanPlaygroundStats")
      .replace("{judgments}", String(this.result.judgments.length))
      .replace("{deps}", String(this.result.dependencies.length));
    return p;
  }

  private renderJudgmentList(): HTMLElement {
    const list = document.createElement("div");
    list.className = "lp-list";
    for (const j of this.result.judgments) {
      list.appendChild(this.renderJudgmentRow(j));
    }
    return list;
  }

  private renderJudgmentRow(j: LeanParsedJudgment): HTMLElement {
    const row = document.createElement("button");
    row.type = "button";
    row.className = `lp-row ${j.id === this.selectedId ? "is-selected" : ""}`;
    row.innerHTML = `
      <span class="pg-row-kind pg-kind-${escapeHtml(j.kind)}">${escapeHtml(j.kind)}</span>
      <span class="lp-row-name">${escapeHtml(j.name ?? t("anonymousLabel"))}</span>
      <span class="lp-row-statement">${escapeHtml(unwrapLeanSymbols(j.statement))}</span>
      <span class="pg-parse-status pg-parse-${escapeHtml(j.parseStatus)}">${escapeHtml(j.parseStatus)}</span>
    `;
    row.onclick = () => {
      this.selectedId = j.id;
      this.render();
    };
    return row;
  }

  /** `LeanParsedJudgment[]`/`{from,to}[]` から `LineageGraph` を組み立てて、
   * その場で `LineageView` を作り直す。データが小さいので毎回作り直して
   * 十分軽い——静的な証明グラフ側のように索引を使い回す必要は無い。 */
  private renderSelectedLineage(): HTMLElement {
    const judgmentById = new Map<number, ExportedJudgment>();
    for (const j of this.result.judgments) {
      judgmentById.set(j.id, {
        id: j.id,
        kind: j.kind,
        name: j.name,
        statement: j.statement,
        context: j.context,
        parseStatus: j.parseStatus,
        sourceFile: t("leanPlaygroundPastedSourceLabel"),
        sourceLine: j.sourceLine,
        paperArxivId: null,
      });
    }
    const dependsOn = new Map<number, number[]>();
    const usedBy = new Map<number, number[]>();
    for (const dep of this.result.dependencies) {
      const from = dependsOn.get(dep.from) ?? [];
      from.push(dep.to);
      dependsOn.set(dep.from, from);
      const to = usedBy.get(dep.to) ?? [];
      to.push(dep.from);
      usedBy.set(dep.to, to);
    }

    const graph: LineageGraph = {
      judgmentById,
      dependsOn,
      usedBy,
      morphismsOf: new Map(), // 貼り付けだけでは射（含意・特殊化等）は分からない。
    };
    const chainDepth = computeChainDepths(dependsOn, judgmentById.keys());

    this.lineageView = new LineageView(this.lineageRoot, graph, chainDepth, {
      onReroot: (id) => {
        this.selectedId = id;
        this.render();
      },
      onSearchConcepts: (query) => this.onSearchConcepts?.(query),
    });
    this.lineageView.setRoot(this.selectedId as number);

    const box = document.createElement("div");
    box.className = "lp-lineage";
    const heading = document.createElement("h4");
    heading.className = "pg-lineage-title";
    heading.textContent = t("lineageTitle");
    box.appendChild(heading);
    box.appendChild(this.lineageRoot);
    return box;
  }
}

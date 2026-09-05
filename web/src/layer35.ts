import { t } from "./i18n";
import type { JudgmentView } from "./types";
import { escapeHtml } from "./util";

/** `crates/mathesis-wasm/src/layer3.rs` の `MorphismView`/`QuotientClassView`/
 * `InferredPathView` と対応する、`serde_wasm_bindgen` 経由で受け取るJSON形。 */
interface MorphismView {
  id: number;
  src: number;
  dst: number;
  kind: "implication" | "specialization" | "generalization" | "equivalence";
  rationale: string | null;
  strategies: string[];
}

interface QuotientClassView {
  representative: number;
  members: number[];
}

interface HopView {
  src: number;
  dst: number;
  kind: string;
}

interface InferredPathView {
  composedKind: string;
  hops: HopView[];
}

/** `KernelStore`（`mathesis-wasm`）が公開する層3〜5メソッドの最小インターフェース。 */
export interface Layer3Store {
  listJudgments(): unknown;
  addMorphism(src: number, dst: number, kind: string, rationale?: string): number;
  listMorphisms(): unknown;
  tagStrategy(morphismId: number, name: string): void;
  quotientClasses(): unknown;
  shortestDerivation(from: number, to: number): unknown;
}

const KIND_LABEL_KEY = {
  implication: "kindImplication",
  specialization: "kindSpecialization",
  generalization: "kindGeneralization",
  equivalence: "kindEquivalence",
} as const;

const KINDS: Array<keyof typeof KIND_LABEL_KEY> = [
  "implication",
  "specialization",
  "generalization",
  "equivalence",
];

type View = {
  src: number | null;
  dst: number | null;
  kind: keyof typeof KIND_LABEL_KEY;
  strategyMorphism: number | null;
  inferenceFrom: number | null;
  inferenceTo: number | null;
  inferenceResult: InferredPathView | null;
  inferenceSearched: boolean;
  error: string | null;
};

/**
 * 層3〜5（射の型付け・同値類の縮約・戦略タグ・推論エンジン）のブラウザデモ。
 * `crates/mathesis-wasm/src/layer3.rs` が持つ、`mathesis-graph` と同じ
 * アルゴリズム（Union-Find縮約・エッジ合成則）へのUI。デモ判断ノード
 * （`KernelStore.seedDemoJudgments()`、5件）の間に人間が射を張り、戦略を
 * タグ付けし、導出パスを検索できる——実データ側（`ProofGraphExplorer`）が
 * ヒューリスティックの「未承認の候補」を表示するのとは対照的に、こちらは
 * 手動で明示的に張った射（＝常にAccepted相当）だけを扱う簡易版。
 */
export class Layer35Panel {
  private root: HTMLElement;
  private store: Layer3Store;
  private wellKnownStrategies: string[];
  private judgments: JudgmentView[] = [];
  private view: View = {
    src: null,
    dst: null,
    kind: "implication",
    strategyMorphism: null,
    inferenceFrom: null,
    inferenceTo: null,
    inferenceResult: null,
    inferenceSearched: false,
    error: null,
  };

  constructor(root: HTMLElement, store: Layer3Store, wellKnownStrategies: string[]) {
    this.root = root;
    this.store = store;
    this.wellKnownStrategies = wellKnownStrategies;
  }

  refreshLanguage(): void {
    this.render();
  }

  /** デモの判断ノード一覧が変わった（初期シード完了など）ときに呼ぶ。 */
  setJudgments(list: JudgmentView[]): void {
    this.judgments = list;
    if (this.view.src === null && list.length > 0) {
      this.view = { ...this.view, src: list[0].id, dst: list[Math.min(1, list.length - 1)].id };
    }
    if (this.view.inferenceFrom === null && list.length > 0) {
      this.view = {
        ...this.view,
        inferenceFrom: list[0].id,
        inferenceTo: list[Math.min(1, list.length - 1)].id,
      };
    }
    this.render();
  }

  private morphisms(): MorphismView[] {
    return (this.store.listMorphisms() as MorphismView[] | undefined) ?? [];
  }

  private nameOf(id: number): string {
    return this.judgments.find((j) => j.id === id)?.name ?? `#${id}`;
  }

  private render(): void {
    this.root.innerHTML = "";
    const wrap = document.createElement("div");
    wrap.className = "l35-wrap";

    if (this.judgments.length < 2) {
      wrap.innerHTML = `<p class="pg-empty">${escapeHtml(t("layer35MorphismsEmpty"))}</p>`;
      this.root.appendChild(wrap);
      return;
    }

    if (this.view.error) {
      const err = document.createElement("div");
      err.className = "l35-error";
      err.textContent = this.view.error;
      wrap.appendChild(err);
    }

    try {
      wrap.appendChild(this.renderAddMorphismForm());
      wrap.appendChild(this.renderMorphismList());
      wrap.appendChild(this.renderQuotientClasses());
      wrap.appendChild(this.renderInference());
    } catch (err) {
      // wasm↔JSブリッジ（serde_wasm_bindgenの型・命名規則の食い違い等）で
      // 予期しない例外が起きても、パネル全体を空白のまま残さずエラーとして
      // 見せる。innerHTMLを先に空にしてからDOMを組み立てる設計上、
      // 途中で例外が起きると`wrap`がrootへ一度もappendされず消えてしまう
      // ため（実際に発生した実例: composedKind/camelCase命名の食い違い）。
      console.error("Layer35Panel render failed:", err);
      wrap.innerHTML = `<p class="l35-error">${escapeHtml(String(err))}</p>`;
    }

    this.root.appendChild(wrap);
  }

  private judgmentOptionsHtml(selected: number | null): string {
    return this.judgments
      .map((j) => `<option value="${j.id}" ${j.id === selected ? "selected" : ""}>${escapeHtml(j.name ?? `#${j.id}`)}</option>`)
      .join("");
  }

  private renderAddMorphismForm(): HTMLElement {
    const box = document.createElement("div");
    box.className = "l35-box";

    const heading = document.createElement("h4");
    heading.className = "l35-box-title";
    heading.textContent = t("addMorphismTitle");
    box.appendChild(heading);

    const form = document.createElement("div");
    form.className = "l35-form";

    const srcSelect = document.createElement("select");
    srcSelect.className = "l35-select";
    srcSelect.innerHTML = this.judgmentOptionsHtml(this.view.src);
    srcSelect.onchange = () => {
      this.view = { ...this.view, src: Number(srcSelect.value) };
    };

    const kindSelect = document.createElement("select");
    kindSelect.className = "l35-select";
    kindSelect.innerHTML = KINDS.map(
      (k) => `<option value="${k}" ${k === this.view.kind ? "selected" : ""}>${escapeHtml(t(KIND_LABEL_KEY[k]))}</option>`,
    ).join("");
    kindSelect.onchange = () => {
      this.view = { ...this.view, kind: kindSelect.value as View["kind"] };
    };

    const dstSelect = document.createElement("select");
    dstSelect.className = "l35-select";
    dstSelect.innerHTML = this.judgmentOptionsHtml(this.view.dst);
    dstSelect.onchange = () => {
      this.view = { ...this.view, dst: Number(dstSelect.value) };
    };

    const rationaleInput = document.createElement("input");
    rationaleInput.type = "text";
    rationaleInput.className = "l35-input";
    rationaleInput.placeholder = t("morphismRationalePlaceholder");

    const addBtn = document.createElement("button");
    addBtn.type = "button";
    addBtn.className = "l35-button";
    addBtn.textContent = t("addMorphismButton");
    addBtn.onclick = () => {
      const src = Number(srcSelect.value);
      const dst = Number(dstSelect.value);
      const kind = kindSelect.value;
      const rationale = rationaleInput.value.trim();
      try {
        this.store.addMorphism(src, dst, kind, rationale.length > 0 ? rationale : undefined);
        this.view = { ...this.view, src, dst, kind: kind as View["kind"], error: null };
      } catch (err) {
        this.view = { ...this.view, error: String(err) };
      }
      this.render();
    };

    const srcLabel = document.createElement("label");
    srcLabel.className = "l35-field";
    srcLabel.innerHTML = `<span>${escapeHtml(t("morphismSrcLabel"))}</span>`;
    srcLabel.appendChild(srcSelect);

    const kindLabel = document.createElement("label");
    kindLabel.className = "l35-field";
    kindLabel.innerHTML = `<span>${escapeHtml(t("morphismKindLabel"))}</span>`;
    kindLabel.appendChild(kindSelect);

    const dstLabel = document.createElement("label");
    dstLabel.className = "l35-field";
    dstLabel.innerHTML = `<span>${escapeHtml(t("morphismDstLabel"))}</span>`;
    dstLabel.appendChild(dstSelect);

    form.append(srcLabel, kindLabel, dstLabel, rationaleInput, addBtn);
    box.appendChild(form);
    return box;
  }

  private renderMorphismList(): HTMLElement {
    const box = document.createElement("div");
    box.className = "l35-box";

    const morphisms = this.morphisms();
    const heading = document.createElement("h4");
    heading.className = "l35-box-title";
    heading.textContent = `${t("morphismsLabel")} (${morphisms.length})`;
    box.appendChild(heading);

    if (morphisms.length === 0) {
      const empty = document.createElement("p");
      empty.className = "pg-empty";
      empty.textContent = t("layer35MorphismsEmpty");
      box.appendChild(empty);
      return box;
    }

    const list = document.createElement("div");
    list.className = "pg-morphism-list";
    for (const m of morphisms) {
      const row = document.createElement("div");
      row.className = "pg-morphism-row";

      const head = document.createElement("div");
      head.className = "pg-morphism-head";
      head.innerHTML = `
        <span class="pg-morphism-kind pg-kind-${escapeHtml(m.kind)}">${escapeHtml(t(KIND_LABEL_KEY[m.kind]))}</span>
        <span class="pg-morphism-other">${escapeHtml(this.nameOf(m.src))}</span>
        <span class="pg-morphism-arrow">→</span>
        <span class="pg-morphism-other">${escapeHtml(this.nameOf(m.dst))}</span>
      `;
      row.appendChild(head);

      if (m.rationale) {
        const rationale = document.createElement("div");
        rationale.className = "pg-morphism-rationale";
        rationale.textContent = `${t("morphismRationaleLabel")}: ${m.rationale}`;
        row.appendChild(rationale);
      }

      row.appendChild(this.renderStrategyTags(m));
      list.appendChild(row);
    }
    box.appendChild(list);
    return box;
  }

  private renderStrategyTags(m: MorphismView): HTMLElement {
    const wrap = document.createElement("div");
    wrap.className = "l35-strategy-row";

    if (m.strategies.length > 0) {
      const chips = document.createElement("div");
      chips.className = "l35-strategy-chips";
      chips.innerHTML = m.strategies.map((s) => `<span class="l35-strategy-chip">${escapeHtml(s)}</span>`).join("");
      wrap.appendChild(chips);
    }

    const input = document.createElement("input");
    input.type = "text";
    input.className = "l35-input l35-strategy-input";
    input.placeholder = t("strategyNamePlaceholder");
    input.setAttribute("list", `l35-strategies-${m.id}`);

    const datalist = document.createElement("datalist");
    datalist.id = `l35-strategies-${m.id}`;
    datalist.innerHTML = this.wellKnownStrategies.map((s) => `<option value="${escapeHtml(s)}"></option>`).join("");

    const addBtn = document.createElement("button");
    addBtn.type = "button";
    addBtn.className = "l35-button l35-button-small";
    addBtn.textContent = t("addStrategyButton");
    addBtn.onclick = () => {
      const name = input.value.trim();
      if (!name) return;
      try {
        this.store.tagStrategy(m.id, name);
        this.view = { ...this.view, error: null };
      } catch (err) {
        this.view = { ...this.view, error: String(err) };
      }
      this.render();
    };

    wrap.append(input, datalist, addBtn);
    return wrap;
  }

  private renderQuotientClasses(): HTMLElement {
    const box = document.createElement("div");
    box.className = "l35-box";

    const heading = document.createElement("h4");
    heading.className = "l35-box-title";
    heading.textContent = t("quotientClassesTitle");
    box.appendChild(heading);

    const classes = (this.store.quotientClasses() as QuotientClassView[] | undefined) ?? [];
    if (classes.length === 0) {
      const empty = document.createElement("p");
      empty.className = "pg-empty";
      empty.textContent = t("quotientClassesEmpty");
      box.appendChild(empty);
      return box;
    }

    const list = document.createElement("div");
    list.className = "l35-quotient-list";
    for (const c of classes) {
      const row = document.createElement("div");
      row.className = "l35-quotient-row";
      row.innerHTML = c.members.map((id) => `<span class="l35-strategy-chip">${escapeHtml(this.nameOf(id))}</span>`).join(" = ");
      list.appendChild(row);
    }
    box.appendChild(list);
    return box;
  }

  private renderInference(): HTMLElement {
    const box = document.createElement("div");
    box.className = "l35-box";

    const heading = document.createElement("h4");
    heading.className = "l35-box-title";
    heading.textContent = t("inferenceTitle");
    box.appendChild(heading);

    const form = document.createElement("div");
    form.className = "l35-form";

    const fromSelect = document.createElement("select");
    fromSelect.className = "l35-select";
    fromSelect.innerHTML = this.judgmentOptionsHtml(this.view.inferenceFrom);
    fromSelect.onchange = () => {
      this.view = { ...this.view, inferenceFrom: Number(fromSelect.value) };
    };

    const toSelect = document.createElement("select");
    toSelect.className = "l35-select";
    toSelect.innerHTML = this.judgmentOptionsHtml(this.view.inferenceTo);
    toSelect.onchange = () => {
      this.view = { ...this.view, inferenceTo: Number(toSelect.value) };
    };

    const searchBtn = document.createElement("button");
    searchBtn.type = "button";
    searchBtn.className = "l35-button";
    searchBtn.textContent = t("inferenceSearchButton");
    searchBtn.onclick = () => {
      const from = Number(fromSelect.value);
      const to = Number(toSelect.value);
      const result = (this.store.shortestDerivation(from, to) as InferredPathView | null | undefined) ?? null;
      this.view = {
        ...this.view,
        inferenceFrom: from,
        inferenceTo: to,
        inferenceResult: result,
        inferenceSearched: true,
      };
      this.render();
    };

    const fromLabel = document.createElement("label");
    fromLabel.className = "l35-field";
    fromLabel.innerHTML = `<span>${escapeHtml(t("inferenceFromLabel"))}</span>`;
    fromLabel.appendChild(fromSelect);

    const toLabel = document.createElement("label");
    toLabel.className = "l35-field";
    toLabel.innerHTML = `<span>${escapeHtml(t("inferenceToLabel"))}</span>`;
    toLabel.appendChild(toSelect);

    form.append(fromLabel, toLabel, searchBtn);
    box.appendChild(form);

    if (this.view.inferenceSearched) {
      box.appendChild(this.renderInferenceResult());
    }

    return box;
  }

  private renderInferenceResult(): HTMLElement {
    const el = document.createElement("div");
    el.className = "l35-inference-result";
    const path = this.view.inferenceResult;
    if (!path) {
      el.innerHTML = `<p class="pg-empty">${escapeHtml(t("inferenceNoPath"))}</p>`;
      return el;
    }
    const hopsHtml = path.hops
      .map((h) => `${escapeHtml(this.nameOf(h.src))} --${escapeHtml(t(KIND_LABEL_KEY[h.kind as keyof typeof KIND_LABEL_KEY]))}--> ${escapeHtml(this.nameOf(h.dst))}`)
      .join("<br>");
    el.innerHTML = `
      <div><strong>${escapeHtml(t("inferenceComposedLabel"))}:</strong> ${escapeHtml(t(KIND_LABEL_KEY[path.composedKind as keyof typeof KIND_LABEL_KEY]))}</div>
      <div class="l35-hops"><strong>${escapeHtml(t("inferenceHopsLabel"))}:</strong><br>${hopsHtml}</div>
    `;
    return el;
  }
}

/**
 * TeX入力を扱う層。役割は2つある。
 *
 *  1. **見せる**: 打ち込んだTeXをその場で組版して見せる（`renderTex`）。
 *     数学者が探し物を書き下す自然な記法はTeXであって、`\forall n \in
 *     \mathbb{N}` を `∀ n ∈ ℕ` の見た目で確認しながら打てないと、
 *     入力欄は「何を打てば当たるのか分からない箱」のままになる。
 *  2. **探す**: 打ち込んだTeXを検索語に翻訳する（`texToSearchTerms`）。
 *     ここが本体。組版だけしても検索が当たらなければ意味が無い。
 *
 * 翻訳先は2種類ある——同じ入力を、性質の違う2つの索引にぶつけるため:
 *
 *   - **記号**（Unicode）: Lean由来の判断1,431件の命題は `∀ (n : ℕ),
 *     (0 < unitBallApproxEps n)` のようにUnicodeで書かれている。
 *     `\mathbb{N}` → `ℕ`、`\leq` → `≤` と直せば、命題本体に直接当たる。
 *   - **英単語**: arXiv 10万論文から抽出した80,727概念のフレーズ索引は
 *     英語の名詞句なので、記号では1件も当たらない。`\int` → "integral"、
 *     `\nabla` → "gradient"、`\|·\|` → "norm" と、記号が指す概念の名前へ
 *     翻訳して初めて届く。
 *
 * この2本立てが無いと、TeXを打てるようにしても「組版はされるが検索は
 * 空振りする」ことになり、かえって始末が悪い。
 */

/**
 * temmlは打ち込みが数式らしくなった時点で初めて読み込む（168KB）。
 *
 * 添えるCSSは `Temml-Local.css`——利用者の端末にあるフォント
 * （Cambria Math / STIX Two Math / Noto Sans Math）を使う版で、
 * 外部から取りに行くのは同梱の12KBのwoff2（花文字 𝒜–𝒵 用）だけ。
 * Viteがこれをビルド成果物に取り込むので、配信物は自己完結したまま
 * になる——CDNのフォントを引く版を選ぶと、ネットワークやCSPの都合で
 * 数式だけ字が化ける事故を持ち込むことになる。
 */
type TemmlModule = typeof import("temml");
let temmlPromise: Promise<TemmlModule> | null = null;

function loadTemml(): Promise<TemmlModule> {
  if (temmlPromise === null) {
    temmlPromise = Promise.all([import("temml"), import("temml/dist/Temml-Local.css")]).then(
      ([module]) => module,
    );
  }
  return temmlPromise;
}

/**
 * 入力がTeXらしいか。`\cmd`・`^`・`$…$` のどれかがあればTeXとして扱う。
 * `a + b = b + a` のような素の数式は**TeXではない**と判定する（従来どおり
 * wasmカーネルの正規化に回すため）。
 *
 * `_`（アンダースコア）単独は判定材料に**含めない**。以前は含めていたが、
 * この検索対象そのもの——Leanの識別子1,431件——がsnake_caseで
 * アンダースコアを多用するため、"deGiorgi_energy_estimate_on_concentricBalls"
 * のような、ソースから見たままコピペした識別子が軒並みTeXと誤判定されて
 * いた。TeXとして扱われると `texToSearchTerms` の素の文字列フォールバック
 * （camelCaseを割らない）に通り、"concentricBalls" が1語のまま検索語に
 * なって、本来 `proofSearch.ts::tokenizeIdentifier` が正しく
 * ["concentric","balls"] へ割って一致させるはずの判断ノードを取り逃がして
 * いた（実データで確認済み——証明グラフ区画の検索欄では正しく2件ヒット
 * するのに、ページ最上段の統合検索だけ0件になっていた）。
 * バックスラッシュを伴わない裸の `_` は「TeXの下付き」より
 * 「識別子の区切り」である可能性のほうがこのサイトでは高いと判断した。
 * `\int_\Omega` のようにバックスラッシュコマンドを伴う本物のTeXは、
 * その `\` 自体で引き続き検出される。
 */
export function looksLikeTex(text: string): boolean {
  return /\\[a-zA-Z]|\\[{}|,;!]|\^|\$/.test(text);
}

/** `$…$` / `$$…$$` / `\(…\)` / `\[…\]` の囲みを外す。 */
export function stripTexDelimiters(text: string): string {
  const s = text.trim();
  const m =
    /^\$\$([\s\S]*)\$\$$/.exec(s) ??
    /^\$([\s\S]*)\$$/.exec(s) ??
    /^\\\(([\s\S]*)\\\)$/.exec(s) ??
    /^\\\[([\s\S]*)\\\]$/.exec(s);
  return m ? m[1].trim() : s;
}

export type TexRenderStatus = "ok" | "error";

export interface TexRenderResult {
  status: TexRenderStatus;
  /** `status === "error"` のときだけ、失敗の理由 */
  message?: string;
}

/**
 * `tex` を `target` の中に組版する。temmlはここで初めて動的importされる。
 *
 * `throwOnError: false` にしてあるので、書きかけの `\frac{1}{` でも
 * 例外にはならず、崩れた箇所が赤く出る。打ちながら見る用途では
 * 「打ち終わるまで何も出ない」より「途中まで出る」ほうが遥かに役に立つ。
 */
export async function renderTex(
  target: HTMLElement,
  tex: string,
  displayMode: boolean,
): Promise<TexRenderResult> {
  const source = stripTexDelimiters(tex);
  if (source.length === 0) {
    target.textContent = "";
    return { status: "ok" };
  }
  let temml: TemmlModule;
  try {
    temml = await loadTemml();
  } catch (err) {
    console.error("Failed to load temml:", err);
    target.textContent = source;
    return { status: "error", message: String(err) };
  }
  // temmlのESMビルドが実際に持っている名前付きexportは `default` だけ
  // （同梱の `.d.ts` には `render` などの名前付きexportも書かれているが、
  // `dist/temml.mjs` の中身は `export default Temml` のみ）。型定義を
  // 信じて `temml.render(...)` と書くと、型検査は通るのに実行時は
  // undefined で落ちる——組版が全部素のテキストにフォールバックする。
  const render = temml.default?.render ?? temml.render;
  target.textContent = "";
  try {
    render(source, target, {
      displayMode,
      throwOnError: false,
      strict: false,
      // 幅の狭い画面で1行に収まらない式を、関係子で折り返す。
      wrap: "=",
    });
  } catch (err) {
    target.textContent = source;
    return { status: "error", message: String(err) };
  }
  const errored = target.querySelector("merror, .temml-error");
  return errored === null ? { status: "ok" } : { status: "error" };
}

/** `$$…$$` / `$…$` / `\(…\)` / `\[…\]` のいずれかで囲まれた数式区間を探す。 */
const MATH_SPAN_RE = /\$\$([\s\S]+?)\$\$|\$([^$\n]+?)\$|\\\(([\s\S]+?)\\\)|\\\[([\s\S]+?)\\\]/g;

/**
 * 診断⑥のinformal Statement（arXivのLaTeX原文をそのまま保持した命題文、
 * `parse_status: informal`）用。地の文に埋め込まれた数式区間だけを温MLで
 * 組版し、それ以外（`\label{...}`・`\cite{...}`のような未解決コマンドや
 * 通常の文章）はそのまま生テキストで表示する——`renderTex`（検索欄のTeX
 * 入力用）と違い、入力全体が数式ではなく「数式が混じった地の文」なので、
 * 全体を1つの数式として温MLに渡すと未知のプローズ単語で必ず壊れる。
 *
 * `renderStatement`（`lineageView.ts`）がLean由来の命題（TeXではなく
 * Leanの構文そのもの）には使わないよう、呼び出し側で`parseStatus ===
 * "informal"`のときだけ呼ぶこと——Lean命題をここに通すと、識別子中の
 * `_`や`^`を数式区切りと誤認しかねない。
 *
 * 個々の数式区間の組版が失敗しても（温MLは`throwOnError: false`だが、
 * それでも例外を投げる入力が理論上ありうる）、その区間だけ生テキストに
 * フォールバックし、他の区間・地の文の表示は続ける。
 */
export async function renderStatementWithMath(container: HTMLElement, raw: string): Promise<void> {
  let temml: TemmlModule;
  try {
    temml = await loadTemml();
  } catch (err) {
    console.error("Failed to load temml:", err);
    container.textContent = raw;
    return;
  }
  const render = temml.default?.render ?? temml.render;

  container.textContent = "";
  const re = new RegExp(MATH_SPAN_RE);
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(raw)) !== null) {
    if (m.index > last) container.appendChild(document.createTextNode(raw.slice(last, m.index)));
    const displayMode = m[1] !== undefined || m[4] !== undefined;
    const mathSource = m[1] ?? m[2] ?? m[3] ?? m[4] ?? "";
    const span = document.createElement(displayMode ? "div" : "span");
    span.className = "lin-inline-math";
    try {
      render(mathSource, span, { displayMode, throwOnError: false, strict: false, wrap: "=" });
    } catch (err) {
      span.textContent = raw.slice(m.index, re.lastIndex);
    }
    container.appendChild(span);
    last = re.lastIndex;
  }
  if (last < raw.length) container.appendChild(document.createTextNode(raw.slice(last)));
}

/**
 * TeXコマンド → (Unicode記号, 英語の検索語)。
 *
 * 「その記号を打つ人が探しているもの」を英語で書く。`\int` を打つ人は
 * 積分に関する何かを探しているので "integral"、`\nabla` なら勾配なので
 * "gradient"。記号そのものの読み（"nabla"）も残す——概念フレーズ側に
 * どちらの語で載っているか分からないため。
 */
const TEX_LEXICON: Record<string, { sym?: string; words?: string[] }> = {
  // 数の体系
  "mathbb R": { sym: "ℝ", words: ["real"] },
  "mathbb N": { sym: "ℕ", words: ["natural"] },
  "mathbb Z": { sym: "ℤ", words: ["integer"] },
  "mathbb Q": { sym: "ℚ", words: ["rational"] },
  "mathbb C": { sym: "ℂ", words: ["complex"] },
  "mathbb F": { sym: "𝔽", words: ["field", "finite"] },
  "mathbb P": { sym: "ℙ", words: ["projective", "probability"] },
  "mathbb E": { sym: "𝔼", words: ["expectation"] },
  "mathbb H": { sym: "ℍ", words: ["quaternion", "hyperbolic"] },
  "mathbb A": { sym: "𝔸", words: ["affine"] },
  "mathbb T": { sym: "𝕋", words: ["torus"] },

  // 論理・集合
  forall: { sym: "∀" },
  exists: { sym: "∃", words: ["existence"] },
  nexists: { sym: "∄" },
  in: { sym: "∈" },
  notin: { sym: "∉" },
  ni: { sym: "∋" },
  subset: { sym: "⊂", words: ["subset"] },
  subseteq: { sym: "⊆", words: ["subset"] },
  subsetneq: { sym: "⊊", words: ["proper", "subset"] },
  supset: { sym: "⊃", words: ["superset"] },
  supseteq: { sym: "⊇", words: ["superset"] },
  cap: { sym: "∩", words: ["intersection"] },
  cup: { sym: "∪", words: ["union"] },
  bigcap: { sym: "⋂", words: ["intersection"] },
  bigcup: { sym: "⋃", words: ["union"] },
  setminus: { sym: "∖", words: ["complement"] },
  emptyset: { sym: "∅", words: ["empty"] },
  varnothing: { sym: "∅", words: ["empty"] },
  land: { sym: "∧" },
  wedge: { sym: "∧", words: ["wedge", "exterior"] },
  lor: { sym: "∨" },
  vee: { sym: "∨" },
  neg: { sym: "¬", words: ["negation"] },
  lnot: { sym: "¬", words: ["negation"] },
  implies: { sym: "⟹", words: ["implication"] },
  iff: { sym: "⟺", words: ["equivalence"] },
  vdash: { sym: "⊢", words: ["provable"] },
  models: { sym: "⊨" },

  // 関係子
  leq: { sym: "≤" },
  le: { sym: "≤" },
  geq: { sym: "≥" },
  ge: { sym: "≥" },
  neq: { sym: "≠" },
  ne: { sym: "≠" },
  ll: { sym: "≪" },
  gg: { sym: "≫" },
  approx: { sym: "≈", words: ["approximation"] },
  simeq: { sym: "≃", words: ["equivalence"] },
  sim: { sym: "∼" },
  cong: { sym: "≅", words: ["isomorphic", "isomorphism", "congruent"] },
  equiv: { sym: "≡", words: ["congruence"] },
  propto: { sym: "∝" },
  perp: { sym: "⊥", words: ["orthogonal", "perpendicular"] },
  parallel: { sym: "∥", words: ["parallel"] },

  // 写像
  to: { sym: "→" },
  rightarrow: { sym: "→" },
  longrightarrow: { sym: "⟶" },
  leftarrow: { sym: "←" },
  mapsto: { sym: "↦", words: ["map"] },
  hookrightarrow: { sym: "↪", words: ["embedding", "injection"] },
  twoheadrightarrow: { sym: "↠", words: ["surjection"] },
  circ: { sym: "∘", words: ["composition"] },
  xrightarrow: { sym: "→" },

  // 解析
  int: { sym: "∫", words: ["integral", "integration"] },
  iint: { sym: "∬", words: ["integral"] },
  oint: { sym: "∮", words: ["contour", "integral"] },
  sum: { sym: "∑", words: ["sum", "series"] },
  prod: { sym: "∏", words: ["product"] },
  lim: { words: ["limit", "convergence"] },
  limsup: { words: ["limit", "superior"] },
  liminf: { words: ["limit", "inferior"] },
  sup: { words: ["supremum", "sup"] },
  inf: { words: ["infimum", "inf"] },
  max: { words: ["maximum"] },
  min: { words: ["minimum"] },
  nabla: { sym: "∇", words: ["gradient", "nabla", "derivative"] },
  partial: { sym: "∂", words: ["partial", "derivative", "boundary"] },
  Delta: { sym: "Δ", words: ["laplacian", "laplace"] },
  square: { sym: "□" },
  infty: { sym: "∞", words: ["infinite", "infinity"] },

  // 代数
  otimes: { sym: "⊗", words: ["tensor", "product"] },
  oplus: { sym: "⊕", words: ["direct", "sum"] },
  times: { sym: "×" },
  cdot: { sym: "⋅" },
  pm: { sym: "±" },
  mp: { sym: "∓" },
  rtimes: { sym: "⋊", words: ["semidirect", "product"] },
  ltimes: { sym: "⋉", words: ["semidirect", "product"] },
  triangleleft: { sym: "◁", words: ["normal", "subgroup"] },
  cdots: { sym: "⋯" },
  dots: { sym: "…" },
  ldots: { sym: "…" },
  langle: { sym: "⟨", words: ["inner", "product", "pairing"] },
  rangle: { sym: "⟩" },
  // ノルム記号。Leanの命題では `‖` (U+2016) で書かれているので、
  // 記号側もその文字にする——`\|u\|_{L^\infty}` と打った人が
  // `‖u‖_{L^∞}` を含む命題に届くように。
  lVert: { sym: "‖", words: ["norm"] },
  rVert: { sym: "‖", words: ["norm"] },
  Vert: { sym: "‖", words: ["norm"] },
  // `\|` は字句としてはコマンド名 `|` になる。
  "|": { sym: "‖", words: ["norm"] },
  lfloor: { sym: "⌊", words: ["floor"] },
  rfloor: { sym: "⌋" },
  lceil: { sym: "⌈", words: ["ceiling"] },
  rceil: { sym: "⌉" },

  // ギリシャ文字（記号だけ。1文字の読みは検索語として弱いので語は付けない）
  alpha: { sym: "α" },
  beta: { sym: "β" },
  gamma: { sym: "γ" },
  Gamma: { sym: "Γ" },
  delta: { sym: "δ" },
  epsilon: { sym: "ε" },
  varepsilon: { sym: "ε" },
  zeta: { sym: "ζ" },
  eta: { sym: "η" },
  theta: { sym: "θ" },
  Theta: { sym: "Θ" },
  vartheta: { sym: "ϑ" },
  iota: { sym: "ι" },
  kappa: { sym: "κ" },
  lambda: { sym: "λ" },
  Lambda: { sym: "Λ" },
  mu: { sym: "μ" },
  nu: { sym: "ν" },
  xi: { sym: "ξ" },
  Xi: { sym: "Ξ" },
  pi: { sym: "π" },
  Pi: { sym: "Π" },
  rho: { sym: "ρ" },
  varrho: { sym: "ϱ" },
  sigma: { sym: "σ" },
  Sigma: { sym: "Σ" },
  tau: { sym: "τ" },
  upsilon: { sym: "υ" },
  phi: { sym: "φ" },
  varphi: { sym: "φ" },
  Phi: { sym: "Φ" },
  chi: { sym: "χ" },
  psi: { sym: "ψ" },
  Psi: { sym: "Ψ" },
  omega: { sym: "ω" },
  Omega: { sym: "Ω", words: ["domain"] },
  ell: { sym: "ℓ" },
  hbar: { sym: "ℏ" },

  // 組版のためだけのコマンド。綴りを検索語にすると "mathfrak" や
  // "operatorname" がノイズとして混ざるので、明示的に無視する
  // （引数の中身は素の文字列として別途拾われる——`\operatorname{Ric}`
  // なら "ric" が残る、これが欲しいもの）。
  mathrm: {},
  mathbf: {},
  mathcal: {},
  mathfrak: {},
  mathsf: {},
  mathit: {},
  boldsymbol: {},
  text: {},
  textrm: {},
  operatorname: {},
  left: {},
  right: {},
  big: {},
  Big: {},
  bigg: {},
  Bigg: {},
  frac: {},
  dfrac: {},
  tfrac: {},
  quad: {},
  qquad: {},
  displaystyle: {},
  limits: {},
  nolimits: {},
  begin: {},
  end: {},
  sqrt: { sym: "√", words: ["root"] },
};

/**
 * 関数空間の記法。`H^1`・`L^2`・`W^{1,p}`・`C^\infty` は、記号を1つずつ
 * 訳しても "H" と "1" にしかならず何も当たらないが、数学者にとっては
 * その並び全体が1つの概念の名前になっている。並びとして拾う。
 */
const SPACE_PATTERNS: { re: RegExp; words: string[] }[] = [
  { re: /\bW\s*\^\s*\{?\s*[^},]*,/, words: ["sobolev", "space"] },
  { re: /\bH\s*\^\s*[-{]?\s*\d/, words: ["sobolev", "space"] },
  { re: /\bH\s*\^\s*\{?\s*s\b/, words: ["sobolev", "space"] },
  { re: /\bL\s*\^\s*\{?\s*(\d|p\b|q\b|\\infty)/, words: ["lebesgue", "space", "integrability"] },
  { re: /\bC\s*\^\s*\{?\s*\{?\s*\d\s*,\s*\\?alpha/, words: ["holder", "continuous"] },
  // `C^\infty` / `C^k` は滑らかさの階数。`C^{0,\alpha}`（ヘルダー）は上で
  // 拾い済みなので、指数の直後にカンマが続く形はここでは除く——さもないと
  // ヘルダー連続な関数が「滑らか」としても検索語を持ってしまう。
  { re: /\bC\s*\^\s*\{?\s*(\\infty|\d+|k)\s*\}?(?!\s*,)/, words: ["smooth", "differentiable"] },
  { re: /\\mathcal\s*\{?\s*O/, words: ["sheaf", "structure"] },
  { re: /\\mathcal\s*\{?\s*D/, words: ["distribution", "derived"] },
  { re: /\\mathfrak\s*\{?\s*[a-z]/, words: ["lie", "algebra"] },
  { re: /\\hat\s*\{?\s*[a-zA-Z]|\\widehat/, words: ["fourier", "transform"] },
  { re: /\\overline|\\bar\s*\{?\s*[a-zA-Z]/, words: ["closure", "conjugate"] },
];

export interface TexTerms {
  /** Unicode化した記号列。Lean命題との照合に使う。 */
  symbols: string[];
  /** 英語の検索語。概念フレーズ索引との照合に使う。 */
  words: string[];
  /** 上の2つを繋いだ、実際に検索へ流す文字列。 */
  query: string;
}

/** TeXを字句に割る。`\cmd` / `{` `}` / `^` `_` / それ以外の1文字。 */
function lexTex(tex: string): string[] {
  const out: string[] = [];
  const re = /\\[a-zA-Z]+|\\.|[{}^_&$]|[A-Za-z]+|[0-9]+|\S/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(tex)) !== null) out.push(m[0]);
  return out;
}

/**
 * TeXを検索語に翻訳する。
 *
 * `\mathbb{R}` のように引数を取るコマンドは、次の非空白トークン
 * （`{` を跨いで中身1つ）を見て `"mathbb R"` という鍵で引く。
 * 引けなければコマンド名そのものを語として残す——辞書に無い
 * `\Ric` や `\Div` のような自前マクロも、綴りが概念名に近ければ
 * 当たる見込みがあるため。
 */
export function texToSearchTerms(tex: string): TexTerms {
  const source = stripTexDelimiters(tex);
  const tokens = lexTex(source);
  const symbolSet = new Set<string>();
  const words: string[] = [];
  const seenWord = new Set<string>();

  // `|\nabla u|` のように同じ記号が複数回出る式は珍しくない。検索条件
  // としては1度あれば足りるし、画面に同じ記号の札が並ぶと読みにくい。
  const symbols = {
    push(sym: string): void {
      symbolSet.add(sym);
    },
  };

  const pushWords = (list: string[] | undefined): void => {
    if (list === undefined) return;
    for (const w of list) {
      if (seenWord.has(w)) continue;
      seenWord.add(w);
      words.push(w);
    }
  };

  for (let i = 0; i < tokens.length; i += 1) {
    const tok = tokens[i];

    if (tok.startsWith("\\")) {
      const cmd = tok.slice(1);

      // `\mathbb{R}` / `\mathbb R` — 引数まで含めて1つの鍵にする。
      let arg: string | null = null;
      let consumed = 0;
      if (tokens[i + 1] === "{" && tokens[i + 3] === "}") {
        arg = tokens[i + 2];
        consumed = 3;
      } else if (tokens[i + 1] !== undefined && /^[A-Za-z]+$/.test(tokens[i + 1])) {
        arg = tokens[i + 1];
        consumed = 1;
      }

      if (arg !== null) {
        const keyed = TEX_LEXICON[`${cmd} ${arg}`];
        if (keyed !== undefined) {
          if (keyed.sym !== undefined) symbols.push(keyed.sym);
          pushWords(keyed.words);
          i += consumed;
          continue;
        }
      }

      const entry = TEX_LEXICON[cmd];
      if (entry !== undefined) {
        if (entry.sym !== undefined) symbols.push(entry.sym);
        pushWords(entry.words);
        continue;
      }
      // 辞書に無いコマンド。3文字以上なら綴りをそのまま検索語にする
      // （`\operatorname{Ric}` の中身や、書き手の自前マクロを拾うため）。
      if (cmd.length >= 3) pushWords([cmd.toLowerCase()]);
      continue;
    }

    // 素の文字列。3文字以上の英字の並びだけを検索語にする——`u`・`x`・`n`
    // のような束縛変数を検索語にすると、ほぼ全件に当たって順位が壊れる。
    if (/^[A-Za-z]+$/.test(tok) && tok.length >= 3) {
      pushWords([tok.toLowerCase()]);
      continue;
    }

    // 記号（`=` `<` `+` など）はそのまま記号側へ。
    if (/^[=<>+\-*/|]$/.test(tok)) symbols.push(tok);
  }

  for (const { re, words: w } of SPACE_PATTERNS) {
    if (re.test(source)) pushWords(w);
  }

  const symbolList = [...symbolSet];
  return { symbols: symbolList, words, query: [...words, ...symbolList].join(" ") };
}

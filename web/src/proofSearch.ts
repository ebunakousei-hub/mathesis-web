import type { ExportedGraphDependency, ExportedJudgment, ExportedMorphism } from "./types";

/**
 * 証明グラフ（`judgments.json`、Phase 9で取り込んだLean 4の実データ）に対する
 * 多段階検索。動的タクソノミー区画の `hybridSearch.ts` と同じ「段階ごとに
 * 意味の違うヒットを分けて出す」考え方を、概念フレーズではなく**判断ノード**
 * に対して行う。
 *
 * 旧実装は `judgments.filter(j => j.name.toLowerCase().includes(query))` の
 * 1段階だけで、次の3つが構造的に拾えなかった:
 *
 *   1. Leanの識別子は `measurableSet_unitBallBadAnnulusOne` のような
 *      snake_case + camelCase の合成語なので、部分文字列一致では
 *      「ball measurable」のような複数語クエリが1件も当たらない。
 *      → 識別子を `_` と大文字境界でトークンに割り、クエリ語を
 *        トークンの前方一致で照合する（`name`段階）。
 *   2. 命題本体（`IsCompact (sphereTwoControl d)`）と仮定コンテキストは
 *      検索対象ですらなかった。「`Metric.ball` を含む判断」を探せない。
 *      → 命題＋コンテキストを別の段階として検索する（`statement`段階）。
 *   3. 証明グラフなのに、辺（依存関係4,797本・射2,284本）が検索に
 *      一切使われていなかった。
 *      → ヒットから1ホップで到達する判断を出す（`connected`段階）。これは
 *        `hybridSearch` の related 段階（embedding近傍）に相当する、
 *        この区画なりの「関連」——ただしこちらは推定ではなく実際の
 *        依存辺・射なので、なぜ関連するかを辺の種類として提示できる。
 *
 * `hybridSearch.ts` と同じく、副作用の無い純粋関数として書く（描画は
 * `proofGraph.ts` の仕事）。
 */

export interface ProofSearchFilters {
  /** "definition" / "theorem" / null（絞り込みなし） */
  kind: string | null;
  /** "full" / "partial" / null（絞り込みなし） */
  parseStatus: string | null;
}

export const NO_FILTERS: ProofSearchFilters = { kind: null, parseStatus: null };

/** `connected` 段階で、そのノードへ辿り着いた経路。 */
export interface ConnectionVia {
  relation: "dependsOn" | "usedBy" | "morphism";
  /** 経路の起点になった、直接ヒットした判断のid */
  anchorId: number;
}

export interface JudgmentHit {
  judgment: ExportedJudgment;
  score: number;
  via?: ConnectionVia;
}

export interface ProofSearchResult {
  exact: JudgmentHit[];
  name: JudgmentHit[];
  statement: JudgmentHit[];
  connected: JudgmentHit[];
}

/**
 * 判断ごとに1度だけ作る検索用の索引。1,431件の判断に対してキー入力の
 * たびに識別子を割り直す必要は無いので、`ProofGraphExplorer` の
 * `buildIndices` から1度だけ構築して使い回す。
 */
export interface ProofSearchIndex {
  judgments: ExportedJudgment[];
  /** 判断id → 識別子を割ったトークン列 */
  nameTokens: Map<number, string[]>;
  /** 判断id → 命題+コンテキストを小文字化して連結した検索対象 */
  haystack: Map<number, string>;
  /**
   * 判断id → 命題+コンテキストに現れる数学記号の集合。
   * `haystack` を小文字化しているせいで `Ω`（U+03A9）が `ω`（U+03C9）に
   * 潰れてしまうので、記号は元の大文字小文字のまま別に持つ。
   */
  symbols: Map<number, Set<string>>;
  /**
   * 数学記号 → その記号を含む判断の件数。記号ごとの重みに使う。
   * 実データでは `ℝ` が1,431件中1,000件超に現れる一方 `∫` は13件しかない。
   * 同じ「1記号の一致」でも意味の重さがまるで違うので、件数で割って
   * 効かせる（語に対するIDFと同じ考え方）。
   */
  symbolDocFreq: Map<string, number>;
  /** 判断id → 依存している判断のid */
  dependsOn: Map<number, number[]>;
  /** 判断id → その判断を参照している判断のid */
  usedBy: Map<number, number[]>;
  /** 判断id → 射で結ばれた相手のid */
  morphismPartners: Map<number, number[]>;
}

/**
 * Leanの識別子を検索可能な語に割る。区切りは
 *   - 英数字以外（`_` `.` `'` など）
 *   - camelCase / PascalCase の大文字境界
 *   - 数字と英字の境界
 * 例: `measurableSet_unitBallBadAnnulusOne`
 *     → ["measurable","set","unit","ball","bad","annulus","one"]
 *     `MemW1pWitness` → ["mem","w","1","p","witness"]
 * 連続する大文字（`MSC`・`IsCompactHS` の `HS`）は1語として扱う。
 */
export function tokenizeIdentifier(name: string): string[] {
  const tokens: string[] = [];
  for (const chunk of name.split(/[^A-Za-z0-9]+/)) {
    if (chunk.length === 0) continue;
    const pieces = chunk.match(/[A-Z]+(?![a-z])|[A-Z]?[a-z]+|[0-9]+/g);
    if (pieces === null) {
      tokens.push(chunk.toLowerCase());
      continue;
    }
    for (const piece of pieces) tokens.push(piece.toLowerCase());
  }
  return tokens;
}

/** クエリ文字列を、識別子と同じ規則で語に割る。 */
export function tokenizeQuery(query: string): string[] {
  return tokenizeIdentifier(query);
}

export function buildProofSearchIndex(
  judgments: ExportedJudgment[],
  dependencies: ExportedGraphDependency[],
  morphisms: ExportedMorphism[],
): ProofSearchIndex {
  const nameTokens = new Map<number, string[]>();
  const haystack = new Map<number, string>();
  const symbols = new Map<number, Set<string>>();
  const symbolDocFreq = new Map<string, number>();
  for (const j of judgments) {
    nameTokens.set(j.id, tokenizeIdentifier(j.name ?? ""));
    const raw = `${j.statement} ${j.context.join(" ")}`;
    haystack.set(j.id, raw.toLowerCase());
    const present = new Set(extractMathSymbols(raw));
    symbols.set(j.id, present);
    for (const sym of present) symbolDocFreq.set(sym, (symbolDocFreq.get(sym) ?? 0) + 1);
  }

  const dependsOn = new Map<number, number[]>();
  const usedBy = new Map<number, number[]>();
  for (const dep of dependencies) {
    push(dependsOn, dep.from, dep.to);
    push(usedBy, dep.to, dep.from);
  }

  const morphismPartners = new Map<number, number[]>();
  for (const m of morphisms) {
    push(morphismPartners, m.src, m.dst);
    if (m.dst !== m.src) push(morphismPartners, m.dst, m.src);
  }

  return { judgments, nameTokens, haystack, symbols, symbolDocFreq, dependsOn, usedBy, morphismPartners };
}

function push(map: Map<number, number[]>, key: number, value: number): void {
  const list = map.get(key);
  if (list === undefined) map.set(key, [value]);
  else list.push(value);
}

function passesFilters(j: ExportedJudgment, filters: ProofSearchFilters): boolean {
  if (filters.kind !== null && j.kind !== filters.kind) return false;
  if (filters.parseStatus !== null && j.parseStatus !== filters.parseStatus) return false;
  return true;
}

/**
 * `name`段階の一致判定とスコア。クエリ語のすべてが識別子トークンの
 * どれかに前方一致すること（順不同）を条件にする。加えて、旧実装の
 * 素朴な部分文字列一致で見つかっていたものが見つからなくなると退化に
 * なるので、そちらも一致として認める。
 *
 * スコアは
 *   - 網羅率: クエリ語数 / 識別子トークン数（短い名前ほど「その語の
 *     ための判断」である可能性が高い）
 *   - 厳密さ: 前方一致ではなく完全一致だったクエリ語の割合
 * の平均。0〜1。
 */
function scoreNameMatch(nameTokens: string[], lowerName: string, queryTokens: string[], rawQuery: string): number | null {
  if (queryTokens.length === 0) return null;

  let exactMatches = 0;
  let allMatched = true;
  for (const q of queryTokens) {
    if (nameTokens.includes(q)) {
      exactMatches++;
    } else if (!nameTokens.some((t) => t.startsWith(q))) {
      allMatched = false;
      break;
    }
  }

  if (!allMatched) {
    // トークン境界をまたぐ部分文字列（"ballbad" のような入力）への保険。
    return lowerName.includes(rawQuery) ? 0.1 : null;
  }

  const coverage = nameTokens.length === 0 ? 0 : Math.min(1, queryTokens.length / nameTokens.length);
  const exactness = exactMatches / queryTokens.length;
  return 0.5 * coverage + 0.5 * exactness;
}

/**
 * クエリに含まれる数学記号を拾う。
 *
 * `tokenizeIdentifier` は `[^A-Za-z0-9]+` で切るので、`ℝ` `∀` `≤` `‖` は
 * **1つ残らず捨てられる**。ところがLean由来の命題本文はこれらの記号で
 * 書かれていて、実データでも `ℝ` が6,064回・`ℕ` が1,085回・`≤` が719回・
 * `∀` が635回現れる。TeXで `\forall n \in \mathbb{N}` と打った人が
 * 1件も引けないのはこの取りこぼしのせいなので、記号は記号として
 * 別に拾い、命題本文へ直接あてる。
 *
 * 範囲はギリシャ文字・letterlike（ℝ ℕ ℤ ℓ）・矢印・数学演算子・
 * 数学英数字（𝔽 𝔼）に限る。全角の日本語がクエリに混ざっても
 * 記号として拾わないようにするため、広い「非ASCII」では切らない。
 */
const MATH_SYMBOL_RE =
  /[Ͱ-Ͽ‖℀-⅏←-⇿∀-⋿⟀-⟯⨀-⫿]|[\u{1D400}-\u{1D7FF}]/gu;

const EMPTY_SYMBOLS: ReadonlySet<string> = new Set<string>();

export function extractMathSymbols(query: string): string[] {
  const found = query.match(MATH_SYMBOL_RE);
  return found === null ? [] : [...new Set(found)];
}

/**
 * 記号1つぶんの重み。その記号を含む判断が少ないほど重い。
 */
function symbolWeight(symbol: string, docFreq: Map<string, number>, total: number): number {
  const df = docFreq.get(symbol) ?? 0;
  return Math.log((total + 1) / (df + 1)) + 1;
}

/**
 * `statement`段階のスコア。クエリ語が命題文（＋コンテキスト）にすべて
 * 現れることを条件にした上で、短い命題ほど高くする——同じ語でも、
 * 900文字の命題に紛れて出るより20文字の命題に出る方が「その語について
 * の判断」である度合いが高い。
 *
 * 記号は語とは扱いを変える。理由は2つ:
 *
 *   1. **語と記号は同時に要求できない。** TeXから翻訳したクエリは
 *      "integral gradient ∫ ∇" のように英単語と記号が混ざるが、Leanの
 *      命題に英単語 "integral" は出てこない。両方を要求すると0件になる。
 *   2. **記号は全部揃うことを要求できない。** `\int_\Omega |
abla u|^2 \leq
 *      C\|u\|` と打つと記号は6個になるが、その6個すべてを含む命題は
 *      まず存在しない。全一致を条件にすると、これも0件になる。
 *
 * そこで記号は「揃った割合」で効かせ、しかも**件数の少ない記号ほど重く**
 * 数える。`≤` は1,431件中719件に出るのでほとんど情報が無いが、`∫` は
 * 13件しか無いので、それが一致したことには大きな意味がある。均等に
 * 数えると、`∫` を含む命題を探しているのに `≤` を含むだけの短い命題が
 * 上に来てしまう。
 */
function scoreStatementMatch(
  haystack: string,
  symbols: ReadonlySet<string>,
  queryTokens: string[],
  querySymbols: string[],
  symbolDocFreq: Map<string, number>,
  totalJudgments: number,
): number | null {
  const tokensOk = queryTokens.length > 0 && queryTokens.every((q) => haystack.includes(q));

  let matchedMass = 0;
  let totalMass = 0;
  for (const sym of querySymbols) {
    const weight = symbolWeight(sym, symbolDocFreq, totalJudgments);
    totalMass += weight;
    if (symbols.has(sym)) matchedMass += weight;
  }
  const symbolCoverage = totalMass > 0 ? matchedMass / totalMass : 0;

  if (!tokensOk && symbolCoverage === 0) return null;

  const brevity = 1 / (1 + haystack.length / 120);
  if (!tokensOk) return symbolCoverage * brevity;
  // 語が揃った上で記号も揃っていれば、さらに上へ。
  return querySymbols.length === 0 ? brevity : (0.5 + 0.5 * symbolCoverage) * brevity;
}

export function proofSearch(
  query: string,
  index: ProofSearchIndex,
  filters: ProofSearchFilters,
  topK: number,
): ProofSearchResult {
  const rawQuery = query.trim().toLowerCase();
  const queryTokens = tokenizeQuery(query);
  const querySymbols = extractMathSymbols(query);
  if (rawQuery.length === 0) {
    return { exact: [], name: [], statement: [], connected: [] };
  }

  const shown = new Set<number>();
  const exact: JudgmentHit[] = [];
  const name: JudgmentHit[] = [];
  const statement: JudgmentHit[] = [];

  for (const j of index.judgments) {
    if (!passesFilters(j, filters)) continue;

    const lowerName = (j.name ?? "").toLowerCase();
    if (lowerName === rawQuery || j.statement.toLowerCase() === rawQuery) {
      exact.push({ judgment: j, score: 1 });
      shown.add(j.id);
      continue;
    }

    const nameScore = scoreNameMatch(index.nameTokens.get(j.id) ?? [], lowerName, queryTokens, rawQuery);
    if (nameScore !== null) {
      name.push({ judgment: j, score: nameScore });
      shown.add(j.id);
      continue;
    }

    const statementScore = scoreStatementMatch(
      index.haystack.get(j.id) ?? "",
      index.symbols.get(j.id) ?? EMPTY_SYMBOLS,
      queryTokens,
      querySymbols,
      index.symbolDocFreq,
      index.judgments.length,
    );
    if (statementScore !== null) {
      statement.push({ judgment: j, score: statementScore });
      shown.add(j.id);
    }
  }

  byScoreDesc(exact);
  byScoreDesc(name);
  byScoreDesc(statement);

  const trimmedName = name.slice(0, topK);
  const trimmedStatement = statement.slice(0, topK);

  // connected: 直接ヒットした判断から、実際の辺で1ホップ到達するもの。
  // 上位のヒットだけを起点にする——結果に載らなかった下位のヒットから
  // 広げると、画面に出ていないノード由来の「関連」ばかりになる。
  const anchors = [...exact, ...trimmedName, ...trimmedStatement];
  const connected = collectConnected(anchors, index, shown, filters, topK);

  return { exact: exact.slice(0, topK), name: trimmedName, statement: trimmedStatement, connected };
}

function byScoreDesc(hits: JudgmentHit[]): void {
  hits.sort((a, b) => b.score - a.score || (a.judgment.name ?? "").localeCompare(b.judgment.name ?? ""));
}

function collectConnected(
  anchors: JudgmentHit[],
  index: ProofSearchIndex,
  shown: Set<number>,
  filters: ProofSearchFilters,
  topK: number,
): JudgmentHit[] {
  const byId = new Map(index.judgments.map((j) => [j.id, j]));
  // 到達したノードごとに、何件のヒットから辿り着いたかを数える。多くの
  // ヒットが共通して参照している判断ほど、そのクエリの「土台」に近い。
  const reachedFrom = new Map<number, { count: number; via: ConnectionVia }>();

  const visit = (neighborId: number, via: ConnectionVia): void => {
    if (shown.has(neighborId)) return;
    const target = byId.get(neighborId);
    if (target === undefined || !passesFilters(target, filters)) return;
    const existing = reachedFrom.get(neighborId);
    if (existing === undefined) reachedFrom.set(neighborId, { count: 1, via });
    else existing.count++;
  };

  for (const anchor of anchors) {
    const id = anchor.judgment.id;
    for (const to of index.dependsOn.get(id) ?? []) visit(to, { relation: "dependsOn", anchorId: id });
    for (const from of index.usedBy.get(id) ?? []) visit(from, { relation: "usedBy", anchorId: id });
    for (const other of index.morphismPartners.get(id) ?? []) visit(other, { relation: "morphism", anchorId: id });
  }

  const hits: JudgmentHit[] = [];
  for (const [id, { count, via }] of reachedFrom) {
    const judgment = byId.get(id);
    if (judgment === undefined) continue;
    hits.push({ judgment, score: count / Math.max(1, anchors.length), via });
  }
  byScoreDesc(hits);
  return hits.slice(0, topK);
}

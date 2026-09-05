import type { SearchEntry } from "./types";

/**
 * 概念検索のクエリ解釈層。
 *
 * ここが無かった頃の検索は、4段階すべてが「クエリ文字列と候補フレーズの
 * 完全一致」か「クエリ語列が候補に連続して含まれるか」だけで出来ていた。
 * つまり**正解のフレーズを既に知っている人にしか使えない**——実データ
 * 80,727候補に対して現実的なクエリ30件を流したところ、20件（67%）が
 * 4段階すべて0件で返っていた:
 *
 *   "the moduli space of curves"  余分な機能語が1つ入るだけで全滅
 *   "manifold kahler"             語順が違うだけで全滅
 *   "reimann hypothesis"          1文字のタイポで全滅
 *   "what is a moduli space"      自然文で全滅
 *   "elliptic curves cryptography" 概念を2つ並べると全滅
 *
 * 内部にどれだけ良い構造があっても、利用者の意図を短い文字列から
 * 汲み取れなければ届かない。このモジュールは検索の入口で
 *   正規化（大小文字・ダイアクリティカルマーク・単複）
 *   → 機能語の除去
 *   → 転置索引による語単位の照合（語順non-sensitive）
 *   → 見つからない語の綴り訂正
 * を行い、「完全一致でなくても意図に最も近い概念に着地させる」ことを
 * 目的にする。
 *
 * 索引は `SearchEntry[]`（`taxonomy.json` の `searchIndex`）から1度だけ
 * 構築する。旧実装がキー入力のたびに80,727件を数回フルスキャンし、
 * そのたびに全フレーズを `split(/\s+/)` し直していた（実測29ms/打鍵）
 * のに対し、転置索引ならクエリ語を含む候補にしか触らない。
 */

/**
 * クエリに現れるが概念名の一部にはならない語。数学の内容語
 * （group / set / field / space / order / number / ring / normal 等）は
 * 絶対に入れない——それらは実在の概念名の構成要素である。
 * ここに入れるのは機能語と、「〜を教えて」「〜を証明して」のような
 * 問いかけの動詞だけ。
 */
const QUERY_STOPWORDS = new Set([
  // 冠詞・前置詞・接続詞・代名詞
  "a", "an", "the", "of", "in", "on", "at", "to", "for", "with", "by", "from", "as", "and", "or",
  "but", "into", "onto", "over", "under", "between", "among", "within", "without", "about",
  "this", "that", "these", "those", "it", "its", "their", "his", "her", "my", "our", "your",
  "i", "we", "you", "they", "he", "she", "me", "us", "them",
  "vs", "versus", "via",
  // 疑問詞・助動詞・be動詞
  "what", "which", "who", "whom", "whose", "where", "when", "why", "how",
  "is", "are", "was", "were", "be", "been", "being", "am",
  "do", "does", "did", "can", "could", "should", "would", "will", "shall", "may", "might", "must",
  "have", "has", "had",
  // 問いかけの動詞（"proof"（proof theory 等の実在概念）は落とさない）
  "explain", "define", "tell", "search", "list", "give", "gives", "need", "want", "know",
  "understand", "learn", "study", "studies", "prove", "proves", "proved", "proving",
  "show", "shows", "find", "finds", "please",
]);

/** ダイアクリティカルマークを落とす（Kähler → kahler, Poincaré → poincare）。 */
function foldDiacritics(text: string): string {
  return text.normalize("NFD").replace(/[̀-ͯ]/g, "");
}

/**
 * ドイツ語由来の数学者名・用語で、著者が独自に行うASCII音訳
 * （ä→"ae"等）を、`foldDiacritics`のアクセント落とし（ä→"a"）と
 * 同じ形へ揃える。Rust側`resolve.rs::fold_known_umlaut_transliteration`
 * と同じ対応表・同じ理由——実データで"kaehler manifold"のような音訳が
 * "kähler manifold"（`foldDiacritics`で"kahler manifold"に畳まれる）とは
 * 別の語として残り、索引側は解決済みだが**検索語の側**がまだ畳めていな
 * かった（このファイル冒頭のコメントの「索引構築とクエリ解釈で必ず
 * 同じものを通す」を守るため、索引・クエリの両方が通る`splitWords`に
 * 置く）。
 *
 * 一般的な「ae→a」「oe→o」「ue→u」という文字列置換はしない——
 * "unique"「"frequency"のようにウムラウトとは無関係な本物のae/oe/ueを
 * 含む頻出語まで壊れる（`resolve.rs`側のコメント参照）。安全なのは、
 * 実データで確認できた具体的な語根の接尾辞だけを名指しすることだけ。
 */
function foldKnownUmlautTransliteration(word: string): string {
  const KNOWN_SUFFIXES: [string, string][] = [
    ["kaehler", "kahler"],
    ["schroedinger", "schrodinger"],
    ["goedel", "godel"],
  ];
  for (const [aeForm, aForm] of KNOWN_SUFFIXES) {
    if (word.endsWith(aeForm)) return word.slice(0, word.length - aeForm.length) + aForm;
  }
  return word;
}

/**
 * 単複のゆれを吸収する軽い語形の畳み込み。クエリ側と索引側の**両方**に
 * 同じ関数を掛けるので、"serie(s)" のように言語学的に正しくない畳み方に
 * なる語があっても、両側で同じ形になる限り照合は成立する。
 */
function foldPlural(word: string): string {
  if (word.length > 4 && word.endsWith("ies")) return `${word.slice(0, -3)}y`;
  if (word.length > 4 && word.endsWith("sses")) return word.slice(0, -2);
  if (word.length > 4 && /(ch|sh|x|z)es$/.test(word)) return word.slice(0, -2);
  // "analysis"・"basis"・"locus"・"class" のような単数形を壊さない
  if (word.length > 3 && word.endsWith("s") && !/(ss|us|is|as)$/.test(word)) return word.slice(0, -1);
  return word;
}

/** 1語の正規化。索引構築とクエリ解釈で必ず同じものを通す。 */
export function normalizeWord(word: string): string {
  return foldPlural(foldKnownUmlautTransliteration(foldDiacritics(word.toLowerCase())));
}

/** 文字列を正規化済みの語列に割る（機能語の除去はしない）。 */
export function splitWords(text: string): string[] {
  return foldDiacritics(text.toLowerCase())
    .split(/[^a-z0-9-]+/)
    .filter((w) => w.length > 0 && w !== "-")
    .map((w) => foldPlural(foldKnownUmlautTransliteration(w)));
}

/**
 * クエリを語列にする。機能語は落とすが、**全部が機能語だった場合は
 * 落とさない**——"how to" のような入力でも何かを返せるようにする
 * （空の語列は「該当なし」を意味してしまい、利用者には区別が付かない）。
 */
export function tokenizeQuery(query: string): string[] {
  const all = splitWords(query);
  const content = all.filter((w) => !QUERY_STOPWORDS.has(w));
  return content.length > 0 ? content : all;
}

export interface ConceptSearchIndex {
  entries: SearchEntry[];
  /** entry index → 正規化したフレーズ全体（完全一致の判定用） */
  normalizedPhrase: string[];
  /** entry index → そのフレーズの語数（「どれだけ余計な語が付いているか」の分母） */
  tokenCount: Int32Array;
  /** 正規化フレーズ → entry index（同じ正規形に複数の表記が畳まれうる） */
  byPhrase: Map<string, number[]>;
  /** 正規化語 → その語を含む entry index の列（転置索引） */
  postings: Map<string, number[]>;
  /** クラスタid → そのクラスタに属する entry index（same concept 段階用） */
  byCluster: Map<number, number[]>;
  /**
   * 語の逆文書頻度 IDF。BM25の重み付けに使う。
   *
   * これが無かった頃の採点は、クエリの語をすべて同じ重みで数えていた。
   * つまり "kähler manifold" というクエリで、80,727概念のうち数千件に
   * 現れる "manifold" と、数十件にしか現れない "kähler" が同格に扱われ、
   * 「その語がどれだけ絞り込みに効くか」という情報が完全に捨てられて
   * いた。IDFは稀な語ほど大きくなるので、絞り込みに効く語を優先できる。
   */
  idf: Map<string, number>;
  /** entry index → そのフレーズの語のIDF合計（適合率の分母に使う）。 */
  entryIdfMass: Float64Array;
  /** 綴り訂正の照合先（postings のキー一覧） */
  vocabulary: string[];
}

export function buildConceptSearchIndex(entries: SearchEntry[]): ConceptSearchIndex {
  const normalizedPhrase = new Array<string>(entries.length);
  const tokenCount = new Int32Array(entries.length);
  const byPhrase = new Map<string, number[]>();
  const postings = new Map<string, number[]>();
  const byCluster = new Map<number, number[]>();

  for (let i = 0; i < entries.length; i++) {
    const words = splitWords(entries[i].phrase);
    const normalized = words.join(" ");
    normalizedPhrase[i] = normalized;
    tokenCount[i] = words.length;

    const sameForm = byPhrase.get(normalized);
    if (sameForm === undefined) byPhrase.set(normalized, [i]);
    else sameForm.push(i);

    const clusterId = entries[i].clusterId;
    if (clusterId !== null) {
      const members = byCluster.get(clusterId);
      if (members === undefined) byCluster.set(clusterId, [i]);
      else members.push(i);
    }

    // 同じ語がフレーズ内で2回出ても postings には1度だけ入れる
    // （照合は「その語を含むか」であって出現回数ではない）。
    for (let w = 0; w < words.length; w++) {
      if (words.indexOf(words[w]) !== w) continue;
      const list = postings.get(words[w]);
      if (list === undefined) postings.set(words[w], [i]);
      else list.push(i);
    }
  }

  // BM25のIDF: ln(1 + (N - n_t + 0.5) / (n_t + 0.5))。
  // 稀な語ほど大きく、全候補に出る語は0に近づく。
  const documentCount = entries.length;
  const idf = new Map<string, number>();
  for (const [token, list] of postings) {
    const containing = list.length;
    idf.set(token, Math.log(1 + (documentCount - containing + 0.5) / (containing + 0.5)));
  }
  // 候補ごとの「語の重要度の総量」。クエリがその候補をどれだけ説明し
  // 切っているか（適合率）を測る分母になる。
  const entryIdfMass = new Float64Array(entries.length);
  for (let i = 0; i < entries.length; i++) {
    let mass = 0;
    const seen = new Set<string>();
    for (const w of splitWords(entries[i].phrase)) {
      if (seen.has(w)) continue;
      seen.add(w);
      mass += idf.get(w) ?? 0;
    }
    entryIdfMass[i] = mass;
  }

  return {
    entries,
    normalizedPhrase,
    tokenCount,
    byPhrase,
    postings,
    byCluster,
    idf,
    entryIdfMass,
    vocabulary: [...postings.keys()],
  };
}

/** 綴り訂正の結果（利用者に「◯◯として解釈した」と見せるため記録する）。 */
export interface Correction {
  from: string;
  to: string;
}

/**
 * 索引に1件も無い語を、語彙の中の最も近い語へ寄せる。
 *
 * 語彙は実データで約4万語あるので、全語との編集距離を取ると打鍵ごとに
 * 重くなる。タイポの実態（1〜2文字の挿入・欠落・置換・転置）に合わせて
 *   - 先頭1文字が一致すること
 *   - 長さの差が許容距離以内であること
 * で候補を絞ってから距離を測る。この2条件は "reimann"→"riemann"、
 * "eliptic"→"elliptic"、"banch"→"banach"、"manifod"→"manifold"、
 * "spce"→"space" のような実際のタイポをすべて通す。
 */
function correctWord(word: string, index: ConceptSearchIndex): string | null {
  if (word.length < 4) return null;
  const maxDistance = word.length <= 5 ? 1 : 2;
  const firstChar = word[0];

  let best: string | null = null;
  let bestDistance = maxDistance + 1;
  let bestPostings = 0;

  for (const candidate of index.vocabulary) {
    if (candidate[0] !== firstChar) continue;
    if (Math.abs(candidate.length - word.length) > maxDistance) continue;
    const distance = boundedEditDistance(word, candidate, maxDistance);
    if (distance > maxDistance) continue;
    const frequency = index.postings.get(candidate)?.length ?? 0;
    // 距離が同じなら、より多くの候補に現れる語を採る（"reimann" の距離1に
    // 複数該当したとき、コーパスで実際に使われている方を選ぶ）。
    if (distance < bestDistance || (distance === bestDistance && frequency > bestPostings)) {
      best = candidate;
      bestDistance = distance;
      bestPostings = frequency;
    }
  }
  return best;
}

/** 上限付きレーベンシュタイン距離。上限を超えた時点で打ち切る。 */
function boundedEditDistance(a: string, b: string, limit: number): number {
  const n = a.length;
  const m = b.length;
  if (Math.abs(n - m) > limit) return limit + 1;

  let previous = new Array<number>(m + 1);
  let current = new Array<number>(m + 1);
  for (let j = 0; j <= m; j++) previous[j] = j;

  for (let i = 1; i <= n; i++) {
    current[0] = i;
    let rowMin = current[0];
    for (let j = 1; j <= m; j++) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1;
      current[j] = Math.min(previous[j] + 1, current[j - 1] + 1, previous[j - 1] + cost);
      if (current[j] < rowMin) rowMin = current[j];
    }
    if (rowMin > limit) return limit + 1;
    const swap = previous;
    previous = current;
    current = swap;
  }
  return previous[m];
}

export interface ResolvedQuery {
  /** 訂正後の検索語（正規化済み・機能語除去済み） */
  tokens: string[];
  /** 訂正した語（あれば利用者に提示する） */
  corrections: Correction[];
}

/**
 * ハイフンを含む語が、索引にも訂正候補にも見つからなかったときの保険。
 *
 * `splitWords` はハイフンを語の一部として残す——"k-theory"・"p-adic"・
 * "nash-moser implicit function theorem" のように、ハイフンが接頭辞や
 * 固有の複合語を作る実在の概念があるため、空白と同じ区切り文字として
 * 潰すと "k" + "theory" のような無意味な断片に分解してしまう。
 *
 * ところがこれは、コーパス側のハイフンの切り方と利用者の入力が一致
 * しない複合語クエリで裏目に出る。例えば "De Giorgi-Nash-Moser theory"
 * と打つと、"giorgi-nash-moser" は17文字1語として索引に存在せず、訂正
 * 候補も見つからない——コーパスには "de giorgi"・"nash"・"moser" が
 * それぞれ**独立した**概念として存在するのに、それらを繋ぐハイフンが
 * 検索語を1個の意味を持たない塊にしてしまい、実際にヒットする語
 * （"de"・"theory"、どちらも数百件に現れる低情報語）だけでスコアが
 * 決まる。結果、"De Giorgi-Nash-Moser theory"——このデモの実データが
 * 扱っている理論そのもの——で検索すると、"de rham theory" のような
 * 無関係な候補が返る（実際に確認済みのバグ）。
 *
 * 直接一致にも綴り訂正にも失敗したハイフン語だけ、ハイフンで割った
 * 各断片を独立した語として再解決する。"k-theory" や
 * "nash-moser implicit function theorem" のようにハイフンごと索引に
 * 存在する語は、この関数に来る前の直接一致で解決されるので影響しない。
 */
function resolveHyphenatedFallback(word: string, index: ConceptSearchIndex): { tokens: string[]; corrections: Correction[] } {
  const parts = word.split("-").filter((p) => p.length > 0);
  if (parts.length < 2) return { tokens: [word], corrections: [] };

  const tokens: string[] = [];
  const corrections: Correction[] = [];
  for (const part of parts) {
    if (index.postings.has(part)) {
      tokens.push(part);
      continue;
    }
    const corrected = correctWord(part, index);
    if (corrected !== null) {
      corrections.push({ from: part, to: corrected });
      tokens.push(corrected);
    } else {
      tokens.push(part);
    }
  }
  return { tokens, corrections };
}

/** クエリ文字列を、索引に存在する語だけの列へ解決する。 */
export function resolveQuery(query: string, index: ConceptSearchIndex): ResolvedQuery {
  const corrections: Correction[] = [];
  const tokens: string[] = [];
  for (const word of tokenizeQuery(query)) {
    if (index.postings.has(word)) {
      tokens.push(word);
      continue;
    }
    const corrected = correctWord(word, index);
    if (corrected !== null) {
      corrections.push({ from: word, to: corrected });
      tokens.push(corrected);
      continue;
    }
    if (word.includes("-")) {
      const fallback = resolveHyphenatedFallback(word, index);
      tokens.push(...fallback.tokens);
      corrections.push(...fallback.corrections);
      continue;
    }
    // 訂正先が無い語も落とさずに残す——「その語を含む候補は無い」と
    // いう情報自体が被覆率スコアに効く（全語一致か部分一致かの判定）。
    tokens.push(word);
  }
  return { tokens, corrections };
}

/** 1候補がクエリの何語と一致したか、そのIDF重みの合計はいくらか。 */
export interface EntryMatch {
  entry: number;
  /** 一致したクエリ語の数（段階の振り分けに使う） */
  matched: number;
  /** 一致したクエリ語のIDFの合計（順位付けに使う） */
  matchedIdf: number;
}

/**
 * クエリ語のいずれかを含む候補を、一致語数付きで集める。
 * クエリ語の postings しか走査しないので、候補80,727件のフルスキャンには
 * ならない。
 */
export function collectMatches(tokens: string[], index: ConceptSearchIndex): EntryMatch[] {
  const matchedCount = new Map<number, number>();
  const matchedIdf = new Map<number, number>();
  const seen = new Set<string>();
  for (const token of tokens) {
    if (seen.has(token)) continue; // 同じ語を2回打たれても1回として数える
    seen.add(token);
    const postings = index.postings.get(token);
    if (postings === undefined) continue;
    const weight = index.idf.get(token) ?? 0;
    for (const entry of postings) {
      matchedCount.set(entry, (matchedCount.get(entry) ?? 0) + 1);
      matchedIdf.set(entry, (matchedIdf.get(entry) ?? 0) + weight);
    }
  }
  const matches: EntryMatch[] = [];
  for (const [entry, matched] of matchedCount) {
    matches.push({ entry, matched, matchedIdf: matchedIdf.get(entry) ?? 0 });
  }
  return matches;
}

/**
 * クエリ語全体のIDF合計。BM25スコアを 0〜1 に正規化する分母に使う
 * （「このクエリで達成しうる最大スコア」に対する達成度にすると、
 * クエリをまたいでスコアの意味が揃う）。
 */
export function queryIdfMass(tokens: string[], index: ConceptSearchIndex): number {
  let mass = 0;
  const seen = new Set<string>();
  for (const token of tokens) {
    if (seen.has(token)) continue;
    seen.add(token);
    mass += index.idf.get(token) ?? 0;
  }
  return mass;
}

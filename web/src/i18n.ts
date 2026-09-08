export type Lang = "ja" | "en";

const DICT = {
  title: { ja: "Mathesis", en: "Mathesis" },
  tagline: {
    ja: "数学的対象・定義・定理・証明・計算を構造として結びつける",
    en: "Connecting mathematical objects, definitions, theorems, proofs and computations as one structure",
  },
  globalSearchConcepts: { ja: "概念（arXiv 10万論文から抽出）", en: "Concepts (from 100k arXiv papers)" },
  globalSearchJudgments: { ja: "判断ノード（Lean証明グラフ）", en: "Judgments (Lean proof graph)" },
  globalSearchOpen: { ja: "この区画で詳しく見る →", en: "Open in that section →" },
  globalSearchEmpty: {
    ja: "概念にも判断ノードにも該当がありませんでした。数式（例: a + b = b + a）を入力すると、その場で正規化・ハッシュ化します。",
    en: "No matching concepts or judgments. Type a formula (e.g. a + b = b + a) to see it normalized and hashed instead.",
  },
  wasmUnavailable: {
    ja: "数式カーネル（WebAssembly）を読み込めませんでした。数式の正規化とデモ区画は使えませんが、概念検索と証明グラフはそのまま使えます。",
    en: "Could not load the formula kernel (WebAssembly). Formula normalization and the demo sections are unavailable, but concept search and the proof graph still work.",
  },
  wasmLoading: {
    ja: "数式カーネル（WebAssembly）を読み込んでいます…",
    en: "Loading the formula kernel (WebAssembly)…",
  },
  searchPlaceholder: {
    ja: "概念・定理を検索（例: moduli space / weak harnack）、またはTeXで数式を入力（例: \\int_\\Omega |\\nabla u|^2）",
    en: "Search concepts and theorems (e.g. moduli space, weak harnack), or type a formula in TeX (e.g. \\int_\\Omega |\\nabla u|^2)",
  },
  explorerTitle: { ja: "分野から探す", en: "Browse by field" },
  explorerExplain: {
    ja: "arXivから実際に発見された分野のカード一覧です（件数は下の統計に表示、コーパスの規模で変わる）。",
    en: "A card list of fields actually discovered from arXiv data (the count — shown in the stats below — depends on corpus size).",
  },
  back: { ja: "← 戻る", en: "← Back" },
  liveParseTitle: { ja: "カーネルのライブ解析結果", en: "Live kernel parse result" },
  liveParseEmpty: {
    ja: "上の検索欄に数式を入力すると、Rust製カーネル（WebAssembly）がその場で正規化・ハッシュ化します。",
    en: "Type an expression above — the Rust kernel (compiled to WebAssembly) normalizes and hashes it live.",
  },
  canonicalForm: { ja: "正規化された式", en: "Canonical form" },
  canonicalHash: { ja: "正規化ハッシュ", en: "Canonical hash" },
  parseStatus: { ja: "パース状態", en: "Parse status" },
  statusFull: { ja: "完全", en: "full" },
  statusPartial: { ja: "部分的", en: "partial" },
  statusFailed: { ja: "失敗", en: "failed" },
  statusInformal: { ja: "非形式（LaTeX由来）", en: "informal (from LaTeX)" },
  parseStatusNotVerifiedHint: {
    ja: "これは構文解析がどこまで成功したかを示す状態で、Leanカーネルによる型検査・証明の検証結果ではありません。「完全」であってもLeanが実際にこの証明を検証したことは意味しません。",
    en: "This shows how much of the syntax could be parsed — it is not the result of Lean kernel type-checking or proof verification. Even \"full\" does not mean Lean has actually verified this proof.",
  },
  demoTitle: {
    ja: "同じ名前・別の文脈は別ノード（デモ）",
    en: "Same shape, different context → different node (demo)",
  },
  demoExplain: {
    ja: "以下はカーネルに投入済みの判断ノード（Judgment）です。add_assoc（自然数）と mul_assoc（群）は文脈 Γ が違うため、統計的な文字列比較ではなく別ノードとして区別されています。",
    en: "These are Judgment nodes already loaded into the kernel. add_assoc (naturals) and mul_assoc (a group) are kept as distinct nodes because their context Γ differs — not just distinguished by name.",
  },
  colName: { ja: "名前", en: "name" },
  colKind: { ja: "種別", en: "kind" },
  colContext: { ja: "文脈 Γ", en: "context Γ" },
  colStatement: { ja: "命題", en: "statement" },
  colHash: { ja: "ハッシュ", en: "hash" },
  colFields: { ja: "分野タグ", en: "field tags" },
  generatedAtLabel: { ja: "データ生成日", en: "Data generated" },
  checkerDerivedLabel: { ja: "件がLean検査由来", en: "checker-derived" },
  checkerDerivedHint: {
    ja: "本文中の識別子の名前一致ではなく、Lean elaboratorが実際に型検査した証明項から機械的に取り出した依存関係。",
    en: "Not name-matching in the source text — extracted mechanically from proof terms the Lean elaborator actually type-checked.",
  },
  dynamicTaxonomyExplain: {
    ja: "arXivから実際に収集した論文を、抽出→embedding→クラスタリング→MSC2020とのalignmentまで通したパイプラインの出力です。MSC名は公式データが英語のみのため、分野名・クラスタ名は常に英語表記です。",
    en: "Live output of the pipeline (extract → embed → cluster → MSC2020 alignment) run on papers actually collected from arXiv. Field and cluster names stay in English, since the official MSC2020 vocabulary has no Japanese translation.",
  },
  dynamicTaxonomyLoading: { ja: "読み込み中…", en: "Loading…" },
  dynamicTaxonomyError: {
    ja: "taxonomy.json の読み込みに失敗しました。先に `mathesis-taxonomy export` を実行してください。",
    en: "Failed to load taxonomy.json. Run `mathesis-taxonomy export` first.",
  },
  tabByField: { ja: "MSC分野から探す", en: "Browse by MSC field" },
  tabNovel: { ja: "MSC未収載の新語彙候補", en: "Terminology not yet in MSC2020" },
  clustersLabel: { ja: "クラスタ", en: "clusters" },
  conceptsLabel: { ja: "概念", en: "concepts" },
  confidenceLabel: { ja: "確信度", en: "confidence" },
  membersLabel: { ja: "件", en: "items" },
  novelClusterHint: {
    ja: "このクラスタのメンバーは1件もMSC2020のコードと一致しませんでした——実際に使われているが未収載の可能性がある語彙です。",
    en: "No member of this cluster matched an existing MSC2020 code — likely terminology in active use that MSC hasn't caught up with yet.",
  },
  searchConceptsPlaceholder: {
    ja: "概念名で検索（例: finite field）",
    en: "Search concept names (e.g. finite field)",
  },
  clearSearch: { ja: "検索をクリア", en: "Clear search" },
  tierExact: { ja: "完全一致", en: "Exact match" },
  tierSameConcept: { ja: "同一概念（同じクラスタ）", en: "Same concept (same cluster)" },
  tierSpecialization: { ja: "特殊化（より具体的な語）", en: "Specializations (more specific terms)" },
  tierRelated: { ja: "関連概念（embedding近傍）", en: "Related concepts (embedding neighbors)" },
  tierClosest: {
    ja: "近い概念（入力語の一部が一致）",
    en: "Closest concepts (some of your words matched)",
  },
  didYouMean: { ja: "もしかして", en: "Did you mean" },
  // {q} に解釈後の語列が入る。日本語と英語で語順が逆になるため、
  // 文字列連結ではなくプレースホルダで組む。
  searchedAs: { ja: "「{q}」として検索しました", en: "Searched as “{q}”" },
  searchTierEmpty: { ja: "該当なし", en: "No matches" },
  relatedLoading: {
    ja: "関連概念のデータ（別ファイル、約10MB）を読み込んでいます…。この段階だけ後から埋まります。",
    en: "Loading the related-concept data (a separate ~10MB file) — this tier will fill in shortly.",
  },
  relatedLoadFailed: {
    ja: "関連概念のデータ（taxonomy.related.json）を読み込めませんでした。他の段階はそのまま使えます。",
    en: "Could not load the related-concept data (taxonomy.related.json). The other tiers still work.",
  },
  fullIndexLoadingHint: {
    ja: "全件の索引を読み込み中のため、今はよく使われる概念だけを検索対象にしています。読み込みが終わり次第、この検索結果は自動的に更新されます。",
    en: "The full index is still loading, so only the most common concepts are searchable right now. Results will update automatically once it's ready.",
  },
  showConceptMap: { ja: "地図で見る", en: "View as map" },
  hideConceptMap: { ja: "地図を閉じる", en: "Hide map" },
  conceptMapAriaLabel: {
    ja: "この概念と、コサイン類似度・型付き関係で繋がる近傍概念の地図",
    en: "A map of this concept and its neighbors by cosine similarity and typed relations",
  },
  mapLegendFocus: { ja: "中心（クリックした概念）", en: "Focus (the concept you clicked)" },
  mapLegendNeighbor: { ja: "似ている（コサイン類似度）", en: "Similar (cosine similarity)" },
  mapLegendSpecialization: { ja: "特殊化（矢印の先が広い概念）", en: "Specialization (arrow points to the broader concept)" },
  mapLegendEquivalent: { ja: "同値の可能性", en: "Possibly equivalent" },
  mapClickToRecenter: {
    ja: "ノードをクリックすると、そこを中心に地図を作り直します。",
    en: "Click a node to recenter the map on it.",
  },
  samplePapersLabel: { ja: "この概念を扱う論文:", en: "Papers on this concept:" },
  aliasesLabel: { ja: "同じ概念の別表記:", en: "Also written as:" },
  typedRelationsLabel: {
    ja: "型付き関係（統計と論文本文の一文から推定・要検証）:",
    en: "Typed relations (inferred from statistics + a paper sentence — verify yourself):",
  },
  relationBroader: { ja: "より一般的な概念:", en: "Broader concept:" },
  relationNarrower: { ja: "より特殊な概念:", en: "Narrower concept:" },
  relationEquivalent: { ja: "同値の可能性がある概念:", en: "Possibly equivalent concept:" },
  relationConfirmed: { ja: "統計・本文が一致", en: "stats + text agree" },
  relationGrounded: { ja: "本文の一文のみ", en: "text only" },
  relationBadgeHint: {
    ja: "実測精度は約50%（38件の手動確認、当初36%から改善）。事実の確認ではなく、統計的な傾向と論文中の一文が一致したという意味——下の一文を自分で読んで判断してください。",
    en: "Measured precision ~50% (38 hand-checked samples, up from an initial 36%). This is not a verified fact — it means a statistical signal and one sentence from a paper agree. Read the sentence below and judge for yourself.",
  },
  samplePapersLoading: { ja: "出典論文を読み込み中…", en: "Loading source papers…" },
  concentrationHint: {
    ja: "分野集中度: この語を含む論文の分野分布が、コーパス全体の分野分布からどれだけ離れているか。高いほど特定分野に固有の概念、低いほど分野を問わず使われる語（0.22未満は執筆上の定型句として候補から除外済み）。分野ラベル付き論文が50件に満たない語には付きません。",
    en: "Field concentration: how far the field distribution of papers containing this term departs from the corpus-wide distribution. Higher means more specific to a field; lower means used across all fields (below 0.22 the term is dropped as writing boilerplate). Only computed for terms appearing in at least 50 field-labelled papers.",
  },
  relatedNeedsExactMatch: {
    ja: "この段階は、その検索で最も確からしい候補の事前計算済み近傍から表示します（静的データのみで動くため、クエリ文自体のembedding化はできません）。",
    en: "This tier shows precomputed neighbors of the best-matching concept for your query — the static export can't embed arbitrary query text on the fly.",
  },
  searchNoResults: {
    ja: "どの段階にも該当がありませんでした。入力した語がこのコーパス（arXivの数学論文10万件から抽出した概念）に一度も現れていない可能性があります。英語の数学用語で試してください。",
    en: "No hits in any tier. None of your words appear in this corpus (concepts extracted from 100,000 arXiv mathematics papers). Try English mathematical terminology.",
  },
  proofGraphTitle: {
    ja: "取り込まれた証明グラフ（実データ）",
    en: "Imported proof graph (real data)",
  },
  proofGraphExplain: {
    ja: "mathesis-importerが実際にLean 4コーパスから構文解析した判断ノード（定理・定義・公理等）と、証明・定義本体中の識別子から推定した依存関係です。下の「デモ判断ノード一覧」は手書きの5件のみですが、こちらは実際にインポートされた全件です。注意: これはLeanカーネルによる型検査・証明検証ではありません——構文をどこまで解析できたか（パース状態）と、識別子の一致から推定した参照関係を表示しているだけで、証明が実際に正しいことの確認ではありません。",
    en: "Judgment nodes (theorems, definitions, axioms, etc.) that mathesis-importer actually parsed from a real Lean 4 corpus, along with dependencies inferred from identifiers referenced in their proofs/bodies. Unlike the handful of hand-written demo judgments below, this is the full real import. Note: this is not Lean kernel type-checking or proof verification — it only shows how much of the syntax could be parsed, and dependencies inferred by matching identifier names. It is not a confirmation that any proof is actually correct.",
  },
  proofGraphLoading: { ja: "読み込み中…", en: "Loading…" },
  proofGraphError: {
    ja: "judgments.json の読み込みに失敗しました。先に `mathesis-import ... --export web/public/judgments.json` を実行してください。",
    en: "Failed to load judgments.json. Run `mathesis-import ... --export web/public/judgments.json` first.",
  },
  searchJudgmentsPlaceholder: {
    ja: "判断ノードを検索（名前・命題・文脈／例: ball measurable, Metric.ball）",
    en: "Search judgments — names, statements, contexts (e.g. ball measurable, Metric.ball)",
  },
  pgTierExact: { ja: "完全一致", en: "Exact match" },
  pgTierExactHint: {
    ja: "識別子または命題そのものがクエリと一致した判断。",
    en: "The identifier or the statement itself equals the query.",
  },
  pgTierName: { ja: "識別子の一致", en: "Identifier match" },
  pgTierNameHint: {
    ja: "Leanの識別子を _ と大文字境界で語に割り、クエリの各語を前方一致で照合します（語順は問いません）。短い名前ほど上位。",
    en: "Lean identifiers are split at underscores and capital letters; every query word must prefix-match one of those parts, in any order. Shorter names rank higher.",
  },
  pgTierStatement: { ja: "命題・文脈の一致", en: "Statement / context match" },
  pgTierStatementHint: {
    ja: "命題本体と仮定コンテキストの中身に、クエリの全語が現れる判断。短い命題ほど上位。",
    en: "Every query word appears in the statement body or its hypothesis context. Shorter statements rank higher.",
  },
  pgTierConnected: { ja: "グラフ上の関連（1ホップ）", en: "Connected in the graph (1 hop)" },
  pgTierConnectedHint: {
    ja: "上のヒットから依存辺・射で1ホップ到達する判断。多くのヒットが共通して参照しているものほど上位——クエリの「土台」にあたる判断が浮かびます。",
    en: "Judgments one dependency edge or morphism away from the hits above, ranked by how many hits reach them — this surfaces the lemmas the query's results are built on.",
  },
  viaDependsOn: { ja: "依存元", en: "depended on by" },
  viaUsedBy: { ja: "参照元", en: "uses" },
  viaMorphism: { ja: "射", en: "morphism with" },
  filterAll: { ja: "すべて", en: "all" },
  filterKindLabel: { ja: "種別", en: "Kind" },
  filterParseLabel: { ja: "パース", en: "Parse" },
  judgmentsLabel: { ja: "件", en: "judgments" },
  anonymousLabel: { ja: "‹無名›", en: "‹anonymous›" },
  contextLabel: { ja: "文脈 Γ", en: "Context Γ" },
  dependsOnLabel: { ja: "依存している判断", en: "Depends on" },
  usedByLabel: { ja: "参照している判断", en: "Used by" },
  dependencyInferredHint: {
    ja: "証明・定義本体に現れる識別子の名前を一致させて機械的に検出したもので、証明項上の最小依存関係であることや、参照が実際に意味上の依存であることは確認していません。",
    en: "Detected mechanically by matching identifier names that appear in the proof/definition body. This does not confirm it is a minimal dependency on the proof term, or that the reference is semantically meaningful.",
  },
  sourcePaperLabel: { ja: "由来論文", en: "Source paper" },
  proofGraphNoResults: { ja: "該当する判断ノードが見つかりませんでした。", en: "No matching judgment nodes found." },
  morphismsLabel: { ja: "論理的な射（層3）", en: "Logical morphisms (layer 3)" },
  morphismsEmpty: { ja: "この判断ノードに関わる射はまだありません。", en: "No morphisms involve this judgment yet." },
  morphismStatusProposedBadge: { ja: "未承認", en: "Unreviewed" },
  morphismStatusAcceptedBadge: { ja: "承認済み", en: "Accepted" },
  morphismStatusRejectedBadge: { ja: "却下済み", en: "Rejected" },
  morphismStatusProposed: {
    ja: "未承認の候補（ヒューリスティックが機械的に提案しただけで、人間のレビューはまだ）",
    en: "unreviewed proposal (a heuristic guess, not yet checked by a human)",
  },
  morphismStatusAccepted: {
    ja: "人間がレビューして承認済み",
    en: "reviewed and accepted by a human",
  },
  morphismStatusRejected: {
    ja: "人間がレビューして却下済み（誤った提案だったという判定）",
    en: "reviewed and rejected by a human (judged to be a wrong proposal)",
  },
  morphismRationaleLabel: { ja: "根拠", en: "Rationale" },
  kindImplication: { ja: "含意", en: "Implication" },
  kindSpecialization: { ja: "特殊化", en: "Specialization" },
  kindGeneralization: { ja: "一般化", en: "Generalization" },
  kindEquivalence: { ja: "同値", en: "Equivalence" },
  layer35Title: {
    ja: "層3〜5: 射・戦略・推論（デモ）",
    en: "Layers 3–5: morphisms, strategy & inference (demo)",
  },
  layer35Explain: {
    ja: "上のデモ判断ノード（手書き5件）に、射（含意・特殊化・一般化・同値）を手動で張り、証明戦略タグを付け、推論エンジンで導出パスを検索できます。射は追加した時点で人間による承認済み（Accepted相当）として扱われる簡易版です——実データ側（層3の見出しの「未承認の候補」）とは違い、ここは手動で明示的に張った射だけを扱います。",
    en: "Manually connect the demo judgments above (5 hand-written nodes) with typed morphisms (implication / specialization / generalization / equivalence), tag them with a proof strategy, and query the inference engine for a derivation path. This simplified demo treats every morphism you add as already human-approved (Accepted) — unlike the real corpus above, whose morphisms are unreviewed heuristic proposals.",
  },
  addMorphismTitle: { ja: "射を追加", en: "Add a morphism" },
  morphismSrcLabel: { ja: "始点（src）", en: "Source (src)" },
  morphismDstLabel: { ja: "終点（dst）", en: "Target (dst)" },
  morphismKindLabel: { ja: "種類", en: "Kind" },
  morphismRationalePlaceholder: { ja: "根拠（任意）", en: "Rationale (optional)" },
  addMorphismButton: { ja: "追加", en: "Add" },
  layer35MorphismsEmpty: { ja: "まだ射がありません。上のフォームから追加してください。", en: "No morphisms yet — add one with the form above." },
  strategyTagsLabel: { ja: "戦略タグ", en: "Strategy tags" },
  strategyNamePlaceholder: { ja: "戦略名（例: induction）", en: "Strategy name (e.g. induction)" },
  addStrategyButton: { ja: "タグ付け", en: "Tag" },
  quotientClassesTitle: { ja: "同値類（層3の縮約）", en: "Equivalence classes (layer 3 quotient)" },
  quotientClassesEmpty: { ja: "受理済みの同値射がまだないため、単集合以外の同値類はありません。", en: "No accepted equivalence morphisms yet, so every class is a singleton." },
  inferenceTitle: { ja: "推論エンジン（層5）: 導出パスを探す", en: "Inference engine (layer 5): find a derivation path" },
  inferenceFromLabel: { ja: "from", en: "from" },
  inferenceToLabel: { ja: "to", en: "to" },
  inferenceSearchButton: { ja: "検索", en: "Search" },
  inferenceNoPath: { ja: "単一の導出関係へ還元できるパスは見つかりませんでした。", en: "No path could be reduced to a single derivation relation." },
  inferenceComposedLabel: { ja: "合成結果", en: "Composed relation" },
  inferenceHopsLabel: { ja: "経路", en: "Path" },

  // ── TeX入力（`tex.ts`） ───────────────────────────────────────
  texPreviewLabel: { ja: "入力中の数式", en: "What you are typing" },
  texTermsLabel: { ja: "この式から引いた検索語", en: "Search terms taken from this formula" },
  texTermsEmpty: {
    ja: "この式からは検索語を取り出せませんでした。記号だけの式は、概念名（英語）に翻訳できる部分がありません。",
    en: "No search terms could be taken from this formula — a purely symbolic expression has nothing to translate into concept names.",
  },
  texHint: {
    ja: "TeXで書けます（例: \\int_\\Omega |\\nabla u|^2\\,dx / H^1(\\Omega) / \\forall n \\in \\mathbb{N}）。打つそばから組版され、記号は概念名に翻訳して検索されます。",
    en: "You can type TeX (e.g. \\int_\\Omega |\\nabla u|^2\\,dx, H^1(\\Omega), \\forall n \\in \\mathbb{N}). It is typeset as you type, and the symbols are translated into concept names for the search.",
  },
  texError: {
    ja: "ここまでのTeXは組版できません（書きかけなら、続けて入力してください）。",
    en: "This TeX cannot be typeset yet — keep typing if it is unfinished.",
  },

  // ── 系譜（`lineage.ts` / `lineageView.ts`） ──────────────────
  lineageTitle: { ja: "この定理は何に依拠しているか", en: "What this theorem rests on" },
  lineageSummary: {
    ja: "依存の鎖は最も深いところで{full}段。ここでは{shown}段ぶん・{nodes}件を描いています（{omitted}件は省略）。",
    en: "The dependency chain runs {full} levels deep at its deepest. Showing {shown} levels, {nodes} judgments ({omitted} omitted).",
  },
  lineageDeeper: { ja: "もう一段深く", en: "One level deeper" },
  lineageShallower: { ja: "一段浅く", en: "One level shallower" },
  lineageToggleMorphisms: { ja: "論理的な関係も描く", en: "Show logical relations" },
  lineageToggleTrustedOnly: { ja: "既定トラバース対象だけ", en: "Trusted only" },
  lineageToggleTrustedOnlyHint: {
    ja: "オンにすると、レビュー済み・形式的に確認済みの辺だけに絞る。今のデータでは0件になることがある——それは不具合ではなく、まだレビューが行われていない事実そのもの。",
    en: "When on, shows only reviewed or formally-verified edges. This can show zero edges on today's data — that's not a bug, it reflects that no review has happened yet.",
  },
  lineageReroot: { ja: "ここを起点にして辿り直す", en: "Trace from here" },
  lineageToConcepts: { ja: "関係する概念をarXiv 10万論文から探す →", en: "Find related concepts across 100k arXiv papers →" },
  lineageOutlineTitle: { ja: "証明の概略（背骨をたどる）", en: "Proof outline (following the spine)" },
  lineageOutlineHint: {
    ja: "上の図の太い線＝最も長い依存の鎖を、上から順に読み下したもの。各段を押すと命題と前提が開きます。",
    en: "The thick line above — the longest dependency chain — read from the top down. Click a step to open its statement and premises.",
  },
  lineageStepBecause: {
    ja: "{next} に依拠（ほかに横から刺さる補題 {side} 件）",
    en: "rests on {next} (plus {side} side lemmas)",
  },
  lineageStepBase: { ja: "ここが鎖の底——これ以上は他の判断に依拠しません", en: "The bottom of the chain — this rests on no further judgment" },
  lineageStepSideLabel: { ja: "この段で追加で要るもの", en: "Also needed at this step" },
  lineageOutlineRest: {
    ja: "…この先あと{remaining}件（この鎖は全部で{total}件の判断でできています）。図の「もう一段深く」で先へ辿れます。",
    en: "…{remaining} more below (this chain is {total} judgments long). Use “One level deeper” on the diagram to follow it further.",
  },
  relDependency: { ja: "依拠", en: "depends on" },
  legendSpine: { ja: "最長の依存鎖（証明の背骨）", en: "Longest dependency chain (the spine)" },
  legendDependency: { ja: "依存（証明が実際に参照した判断）", en: "Dependency (judgment the proof actually cites)" },
  legendSpecialization: { ja: "特殊化・一般化（提案）", en: "Specialization / generalization (proposed)" },
  legendEquivalence: { ja: "同値（提案）", en: "Equivalence (proposed)" },
  legendVisibleOnly: {
    ja: "既定トラバース対象外（薄い線） — 表示はされるが、既定の信頼範囲には含まれない",
    en: "Not default-traversal eligible (faded) — shown, but outside the default trust boundary",
  },
  lineageTrustedOnlyEmpty: {
    ja: "「既定トラバース対象だけ」表示では、この判断まわりに辺が1本もありません。この定理の依存はすべて名前一致による抽出（Lean elaboratorの正式exportではない）で、射はまだ人間によるレビューを1件も受けていないためです——不具合ではありません。上のトグルを切ると通常の表示に戻ります。",
    en: "With \"trusted only\" on, there are no edges around this judgment. Every dependency here comes from name-matching extraction (not a formal Lean elaborator export), and no morphism has been human-reviewed yet — this is not a bug. Turn the toggle above off to see the normal view.",
  },
  lineageTabGraph: { ja: "系譜をたどる", en: "Trace the lineage" },
  lineageTabDetail: { ja: "この判断の詳細", en: "Judgment detail" },

  // ── Lean貼り付け区画（`leanPlayground.ts`） ─────────────────
  leanPlaygroundTitle: { ja: "Leanを貼り付けて試す", en: "Paste Lean and try it" },
  leanPlaygroundExplain: {
    ja: "theorem・lemma・def・axiom・instance・exampleの宣言を貼り付けると、この場でCLI版（mathesis-importer）と同じパーサーが判断ノードと依存関係を抽出します。抽出はこの貼り付けの中だけで閉じます——サーバーへは何も送信されません。",
    en: "Paste theorem/lemma/def/axiom/instance/example declarations and the same parser the CLI (mathesis-importer) uses extracts judgments and dependencies right here. Extraction is scoped to this paste — nothing is sent to a server.",
  },
  leanPlaygroundPlaceholder: {
    ja: "Lean 4のtheorem・lemma・def・axiom宣言を貼り付けてください（例のボタンでこのページの実データから短い抜粋を試せます）",
    en: "Paste Lean 4 theorem/lemma/def/axiom declarations here (use the example button to try a short excerpt of this page's own real data)",
  },
  leanPlaygroundExampleButton: { ja: "実データの例を入れる", en: "Load a real example" },
  leanPlaygroundClear: { ja: "クリア", en: "Clear" },
  leanPlaygroundEmpty: {
    ja: "上に貼り付けると、その場で判断ノードと依存関係を抽出します。",
    en: "Paste something above and judgments/dependencies are extracted right here.",
  },
  leanPlaygroundNoDeclarations: {
    ja: "theorem・lemma・def・axiom・instance・exampleのどれも見つかりませんでした。",
    en: "No theorem/lemma/def/axiom/instance/example declarations were found.",
  },
  leanPlaygroundStats: {
    ja: "{judgments}件の判断ノード・{deps}件の依存関係をこの貼り付け内で検出しました。",
    en: "Found {judgments} judgments and {deps} dependency edges within this paste.",
  },
  leanPlaygroundPastedSourceLabel: { ja: "貼り付けたソース", en: "pasted source" },
} as const;

/** 辞書の鍵。他のモジュールが `t()` に渡す鍵を型として持てるように公開する。 */
export type TKey = keyof typeof DICT;

let current: Lang = "ja";

export function getLang(): Lang {
  return current;
}

export function setLang(lang: Lang): void {
  current = lang;
}

export function t(key: TKey): string {
  return DICT[key][current];
}

export function label(l: { ja: string; en: string }): string {
  return l[current];
}

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
  tabPending: { ja: "保留（未確定の候補）", en: "Pending (unconfirmed candidates)" },
  tabNovel: { ja: "未分類（MSC未収載）", en: "Unclassified (not yet in MSC2020)" },
  mscScopeHint: {
    ja: "MSC分野を絞り込んでも0件・少数件になることがあります——それは「この分野に該当する事項が無い」ではなく「このリリースで分類済みの事項が無い」という意味です。分類状況（分類済み・保留・未分類）は必ずしも数学的な関連の有無を意味しません。分類状況に関わらず全事項を探すには、上の検索欄を使ってください。",
    en: "An MSC field can show zero or few results — that means no currently classified item in this release, not that nothing relevant exists. Classification status (classified / pending / unclassified) does not by itself indicate mathematical relevance. To search regardless of classification status, use the search box above.",
  },
  clustersLabel: { ja: "クラスタ", en: "clusters" },
  conceptsLabel: { ja: "概念", en: "concepts" },
  confidenceLabel: { ja: "確信度", en: "confidence" },
  membersLabel: { ja: "件", en: "items" },
  novelClusterHint: {
    ja: "このクラスタのメンバーは1件もMSC2020のコードと一致しませんでした——実際に使われているが未収載の可能性がある語彙です。",
    en: "No member of this cluster matched an existing MSC2020 code — likely terminology in active use that MSC hasn't caught up with yet.",
  },
  pendingClusterHint: {
    ja: "候補となるMSCコードはありますが、メンバーの過半数の一致（2件以上かつ50%超）には届いていません——「分類済み」と表示するには根拠が弱いため、候補のまま保留にしています。",
    en: "There's a candidate MSC code, but it falls short of a clear majority among members (needs 2+ agreeing, over 50%) — too weak to call it classified, so it stays a pending candidate.",
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
  /** P6.3（`docs/P6_3_STATUS.md`）: `mathesis-provenance promote-review`で
   *  本人確認済みレビューにより昇格した意味的関係のバッジ。 */
  relationReviewed: { ja: "本人確認済みレビューで承認", en: "authenticated review" },
  relationBadgeHint: {
    ja: "実測精度は約50%（38件の手動確認、当初36%から改善）。事実の確認ではなく、統計的な傾向と論文中の一文が一致したという意味——下の一文を自分で読んで判断してください。",
    en: "Measured precision ~50% (38 hand-checked samples, up from an initial 36%). This is not a verified fact — it means a statistical signal and one sentence from a paper agree. Read the sentence below and judge for yourself.",
  },
  relationReviewedBadgeHint: {
    ja: "本文からの自動抽出ではなく、人が読んで承認した判断です。詳細（承認者・資格・日付・リリース）は「Provenance」を開いて確認してください。",
    en: "Not an automatic text extraction — a human reviewer read this and accepted it. Open \"Provenance\" for who, under what authority, when, and against which release.",
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
    ja: "多くは証明・定義本体に現れる識別子の名前を一致させて機械的に検出したもの(text-extracted)。「Lean検査由来」の印が付いたものは、型検査済みの証明項からLean elaboratorが直接取り出した参照——それでも証明項上の最小依存であることや意味上の依存であることまでは確認していません（クリックで詳細）。",
    en: "Most are detected mechanically by matching identifier names in the proof/definition body (text-extracted). Ones marked \"checker-derived\" were pulled directly from the type-checked proof term by the Lean elaborator instead — though even those don't confirm minimality or semantic relevance (click for detail).",
  },
  /** P6.1（`docs/LEAN_DEPENDENCY_POLICY.md`）: 依存チップの由来バッジ。 */
  dependencyOriginCheckerBadge: { ja: "Lean検査由来", en: "checker-derived" },
  dependencyOriginDetailHint: { ja: "根拠の詳細を見る", en: "View evidence detail" },
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
  legendCheckerDerived: { ja: "Lean検査由来（証明項から機械抽出）", en: "Checker-derived (pulled from the type-checked proof term)" },
  legendTextExtracted: { ja: "バッジ無し = 本文抽出（識別子の名前一致）", en: "No badge = text-extracted (identifier name-matching)" },
  legendMorphismAccepted: { ja: "射: 承認済み", en: "Morphism: accepted" },
  legendMorphismProposed: { ja: "射: 未承認（ヒューリスティック提案）", en: "Morphism: unreviewed proposal" },
  legendMorphismRejected: { ja: "射: 却下済み", en: "Morphism: rejected" },
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

  // ── データ範囲とライセンス（`main.ts` の #data-scope 節） ─────
  dataScopeLinkText: {
    ja: "↓ このサイトは何を、どこまで載せているか（データ範囲とライセンス）",
    en: "↓ What this site does (and doesn't) cover — data scope & licensing",
  },
  dataScopeTitle: { ja: "データ範囲とライセンス", en: "Data scope & licensing" },
  researchPreviewBadge: { ja: "研究プレビュー", en: "Research preview" },
  dataScopeExplain: {
    ja: "研究プレビューです。「証明を検証した」「網羅的な数学検索」といった主張はしていません——ここに書かれていない保証は存在しないものとして読んでください。",
    en: "This is a research preview. It does not claim to verify proofs or to search mathematics exhaustively — treat any guarantee not written here as not existing.",
  },
  dataScopeBody: {
    ja: `
      <h3>ステータスとスナップショット</h3>
      <p>データのコーパスは <code>v0-baseline-20260905</code> リリース（2026-09-05取得）から件数が変わっていません——直近の <code>mathesis-provenance verify-release</code>（証拠層・Web出力の構造的な再検証ゲート）で確認済みです。コードとスキーマは継続的に更新されており、このページ自体もその一部です。</p>

      <h3>含まれるデータソース</h3>
      <table class="scope-table">
        <thead><tr><th>ソース</th><th>含まれる内容</th><th>ライセンス</th><th>備考</th></tr></thead>
        <tbody>
          <tr>
            <td>Lean 4 / Mathlib 由来の証明グラフ</td>
            <td>判断ノード 4,052件・依存辺 6,185件・射 2,284件（実際のLeanコーパスからのインポート）</td>
            <td><a href="https://www.apache.org/licenses/LICENSE-2.0" target="_blank" rel="noopener">Apache-2.0</a>（Mathlibのソースコード）</td>
            <td>Leanカーネルによる型検査・証明検証ではありません。構文がどこまで解析できたか（パース状態）だけを表示します。</td>
          </tr>
          <tr>
            <td>arXiv概念タクソノミー</td>
            <td>論文14万2,948件のtitle/abstract/categoriesから抽出した候補フレーズ11万3,339件・解決済み概念9万4,278件・クラスタ3万4,083件（うち曖昧1,334件）</td>
            <td>arXivメタデータの利用条件に準拠</td>
            <td>本文（LaTeXソース）からの引用は、関係の根拠として一文のみ・出典arXiv ID明記の上で表示。論文ごとの本文ライセンスは投稿者の選択に依存するため一括のCCではなく、全文は再配布していません。</td>
          </tr>
          <tr>
            <td>MSC2020分類</td>
            <td>タクソノミーのクラスタをMSC2020分野コードへalignment</td>
            <td><a href="https://creativecommons.org/licenses/by-nc-sa/4.0/" target="_blank" rel="noopener">CC BY-NC-SA 4.0</a>（<a href="https://msc2020.org/" target="_blank" rel="noopener">Mathematical Reviews / zbMATH</a>）</td>
            <td>非営利限定のライセンスです。MSC名は公式データが英語のみのため、分野名・クラスタ名は常に英語表記です。</td>
          </tr>
          <tr>
            <td>Math-Graph（外部・パイロット）</td>
            <td>2つのLeanプロジェクトから62宣言・48辺（型クラス階層46件・字面一致2件）</td>
            <td><a href="https://creativecommons.org/licenses/by/4.0/" target="_blank" rel="noopener">CC BY 4.0</a>（<a href="https://huggingface.co/datasets/uw-math-ai/math-graph" target="_blank" rel="noopener">uw-math-ai</a>）</td>
            <td>既定の信頼グラフ・検索・系譜表示には一切含まれません（visible_only）。Mathesis自身による独立検証はしていません。「Math-Graph比較/発見モード」パネルは既定で非表示です。</td>
          </tr>
          <tr>
            <td>OpenAlex</td>
            <td>論文間の引用照合のために取得</td>
            <td><a href="https://creativecommons.org/publicdomain/zero/1.0/" target="_blank" rel="noopener">CC0 1.0</a></td>
            <td>現時点でこのコーパスに解決済みの引用辺は0件（Lean紐付き論文138件のうち）——パイプラインには組み込まれていますが、公開データにはまだ実質的に反映されていません。</td>
          </tr>
        </tbody>
      </table>

      <h3>信頼レベルの用語</h3>
      <ul class="scope-list">
        <li><b>Lean検査由来（checker-derived）</b> — 型検査済みの証明項からLean elaboratorが直接取り出した依存関係。</li>
        <li><b>本文抽出（text-extracted）</b> — 証明・定義本体の識別子名の一致から機械的に検出したもの。elaboratorによる検証は経ていません。</li>
        <li><b>本人確認済みレビュー（reviewed）</b> — 資格を持つ人間が実際にレビューし承認した決定。<b>このリリースでは0件</b>——スキーマとUI（承認バッジ）は実装済みですが、まだどの主張もこの経路を通っていません。</li>
        <li><b>外部・表示専用（external / visible_only）</b> — Math-Graphなど、Mathesisが独立検証していない外部データセット由来。既定のトラバースにも検索結果にも現れません。</li>
        <li><b>統計的示唆（proposed）</b> — Hearstパターンや分布的類似度などのヒューリスティックが機械的に提案しただけの関係。「一致」表示の実測精度は約50%（38件の手動確認、当初36%から改善）——事実の確認ではありません。</li>
      </ul>

      <h3>これは何ではないか</h3>
      <ul class="scope-list">
        <li>網羅的な数学文献検索サービスではありません——arXivの数学カテゴリの一部と、限られたLeanコーパスだけを扱っています。</li>
        <li>Leanカーネルによる証明検証ではありません——パース状態と識別子一致による依存関係の推定を表示しているだけです。</li>
        <li>依存関係が数学的に最小であることは主張していません。</li>
        <li>表示されている辺がすべて証明上の依存や論理的含意であるとは限りません（特殊化・一般化・同値・関連候補は別種の主張です）。</li>
        <li>Math-Graphなど外部データはMathesis自身による独立検証を経ていません。</li>
      </ul>

      <h3>実験的な機能</h3>
      <ul class="scope-list">
        <li>Leanプレイグラウンド（貼り付け即時抽出。ブラウザ内で完結し、型検査は行いません）</li>
        <li>MSC動的タクソノミー（統計的クラスタリング。人手によるキュレーションではありません）</li>
        <li>Math-Graph比較/発見パネル（外部データ、既定で非表示）</li>
        <li>型付き関係の提案（統計的示唆。実測精度・要検証の注記あり）</li>
      </ul>

      <h3>運用について</h3>
      <p>現在は静的な読み取り専用スナップショットとして配信しています。ユーザーアカウント・サーバー側の検索API・このアプリケーション自体によるトラッキングはありません。</p>
    `,
    en: `
      <h3>Status and snapshot</h3>
      <p>The data corpus has not changed in row counts since the <code>v0-baseline-20260905</code> release (captured 2026-09-05) — confirmed by the most recent <code>mathesis-provenance verify-release</code> run (the structural re-verification gate for the evidence layer and Web exports). Code and schema keep changing; this page is part of that ongoing work.</p>

      <h3>Data sources included</h3>
      <table class="scope-table">
        <thead><tr><th>Source</th><th>What's included</th><th>License</th><th>Notes</th></tr></thead>
        <tbody>
          <tr>
            <td>Lean 4 / Mathlib-derived proof graph</td>
            <td>4,052 judgment nodes, 6,185 dependency edges, 2,284 morphisms (a real import of a Lean corpus)</td>
            <td><a href="https://www.apache.org/licenses/LICENSE-2.0" target="_blank" rel="noopener">Apache-2.0</a> (Mathlib source code)</td>
            <td>Not Lean-kernel type-checking or proof verification — this only shows how much of the syntax could be parsed.</td>
          </tr>
          <tr>
            <td>arXiv concept taxonomy</td>
            <td>142,948 papers' title/abstract/categories → 113,339 candidate phrases → 94,278 resolved concepts → 34,083 clusters (1,334 ambiguous)</td>
            <td>Used under arXiv's own terms for metadata reuse</td>
            <td>Excerpts from paper LaTeX source are shown only as a single sentence of evidence for one relation, attributed by arXiv id. Per-paper full-text license varies by submitter and is not blanket CC — full paper text is never republished.</td>
          </tr>
          <tr>
            <td>MSC2020 classification</td>
            <td>Taxonomy clusters aligned to MSC2020 subject codes</td>
            <td><a href="https://creativecommons.org/licenses/by-nc-sa/4.0/" target="_blank" rel="noopener">CC BY-NC-SA 4.0</a> (<a href="https://msc2020.org/" target="_blank" rel="noopener">Mathematical Reviews / zbMATH</a>)</td>
            <td>Non-commercial only. Field and cluster names stay in English since the official MSC2020 vocabulary has no Japanese translation.</td>
          </tr>
          <tr>
            <td>Math-Graph (external, pilot)</td>
            <td>62 declarations / 48 edges (46 typeclass-hierarchy, 2 literal) from 2 Lean projects</td>
            <td><a href="https://creativecommons.org/licenses/by/4.0/" target="_blank" rel="noopener">CC BY 4.0</a> (<a href="https://huggingface.co/datasets/uw-math-ai/math-graph" target="_blank" rel="noopener">uw-math-ai</a>)</td>
            <td>Never part of the default trusted graph, search, or lineage view (visible_only). Not independently verified by Mathesis. The "Math-Graph comparison / discovery mode" panel is hidden by default.</td>
          </tr>
          <tr>
            <td>OpenAlex</td>
            <td>Fetched for cross-paper citation matching</td>
            <td><a href="https://creativecommons.org/publicdomain/zero/1.0/" target="_blank" rel="noopener">CC0 1.0</a></td>
            <td>0 citation edges are currently resolved in this corpus (out of 138 Lean-linked papers) — the pipeline exists, but this release's data doesn't materially reflect it yet.</td>
          </tr>
        </tbody>
      </table>

      <h3>Trust-level vocabulary</h3>
      <ul class="scope-list">
        <li><b>checker-derived</b> — pulled directly from a type-checked proof term by the Lean elaborator.</li>
        <li><b>text-extracted</b> — detected mechanically by matching identifier names in a proof/definition body; not elaborator-verified.</li>
        <li><b>reviewed (authenticated)</b> — an accountable decision by a qualified human reviewer. <b>Zero in this release</b> — the schema and UI (the review badge) exist, but no assertion has gone through this path yet.</li>
        <li><b>external / visible_only</b> — from an external dataset (Math-Graph) that Mathesis has not independently verified. Never appears in default traversal or search results.</li>
        <li><b>proposed (statistical suggestion)</b> — a heuristic's mechanical guess (Hearst patterns, distributional similarity). Measured precision on "agreement"-labeled relations is ~50% (38 hand-checked samples, up from an initial 36%) — not a confirmed fact.</li>
      </ul>

      <h3>What this is not</h3>
      <ul class="scope-list">
        <li>Not an exhaustive mathematics search service — it covers a slice of arXiv's math categories and a limited Lean corpus.</li>
        <li>Not Lean kernel proof verification — it only shows parse status and dependencies inferred from identifier matches.</li>
        <li>Does not claim any dependency set is mathematically minimal.</li>
        <li>Not every displayed edge is a proof dependency or logical implication (specialization/generalization/equivalence/relatedness are different kinds of claims).</li>
        <li>External data such as Math-Graph has not been independently verified by Mathesis.</li>
      </ul>

      <h3>Experimental features</h3>
      <ul class="scope-list">
        <li>Lean playground (extraction happens instantly in the browser on paste; no type-checking)</li>
        <li>Dynamic MSC taxonomy (statistical clustering, not human-curated)</li>
        <li>Math-Graph comparison / discovery panel (external data, hidden by default)</li>
        <li>Typed relation suggestions (statistical, with a measured-precision caveat shown inline)</li>
      </ul>

      <h3>Operationally</h3>
      <p>Mathesis is served today as a static, read-only snapshot — no user accounts, no server-side search API, and no telemetry collected by this application.</p>
    `,
  },
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

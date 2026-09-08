-- Priority 2, step 1（ユーザー指示 2026-09-08）、P6.1で強化（ユーザー指示
-- 2026-09-08 "make checker-derived dependencies precise and explainable"）:
-- 本物のLean elaboratorから依存関係マニフェストを取り出す。
-- `crates/mathesis-provenance/src/lean_manifest_adapter.rs`が読むJSONを
-- 標準出力へ書く。
--
-- P6.1の核心: `Expr.getUsedConstants`は型検査済みの項が実際に参照する
-- 定数を機械的に返すが、その中には利用者が「書いた」依存とは呼べない
-- ものが混じる——自動生成された再帰子・matcher・等式補題・構造体の
-- noConfusion/sizeOf族・private実装詳細。これらを素通しすると
-- 「checker-derived」の看板に見合わない雑音になる
-- （`docs/LEAN_DEPENDENCY_POLICY.md`が定義と根拠を持つ）。
--
-- そのため各宣言について2つの配列を書き出す:
--   `rawConstants`         — フィルタ前、getUsedConstantsが実際に見つけた
--                            全定数（完全修飾名、重複除去・整列済み）。
--                            監査用に破棄しない。
--   `publishedDependencies` — 上記のうち、同じプロジェクト名前空間内・
--                            自己参照でない・生成/private詳細でないものだけ。
--                            各要素は`origin`("type"|"body"|"both")付き。
--
-- `DeGiorgi`名前空間だけを対象にする（Mathlib側の補題は
-- `publishedDependencies`に含めない——比較対象の`judgment_dependencies`が
-- judgment同士の依存だけを見ているのと同じ粒度に合わせる、
-- `docs/LEAN_DEPENDENCY_POLICY.md`決定4）。
import DeGiorgi.BallExtension.ApproximationControl

open Lean

/-- P6.1フィルタリングポリシーのバージョン。下の`isGeneratedOrPrivate`/
    `hasGeneratedSuffix`を変えたら必ず上げる——`verify.rs`のリリースゲートが
    manifest側とビルド側のこの文字列が一致するかを検査する
    (`crates/mathesis-provenance/src/lean_manifest_adapter.rs::FILTERING_POLICY_VERSION`
    と同じ値を手で同期させる必要がある。Lean側のスクリプトから
    Rust側の定数を直接参照する手段が無い——`fixtures/`の独立Lakeプロジェクトも
    同じ制約でこのロジックを複製している理由と同じ、下記コメント参照)。 -/
def filteringPolicyVersion : String := "mathesis-lean-dependency-filter-v1"

/-- この抽出プログラム自身のバージョン。P6.1でraw/published分離・origin・
    生成物フィルタを追加したので v1 → v2。 -/
def extractorVersion : String := "mathesis-lean-extract-v2"

def projectNamespace : Name := `DeGiorgi
def projectLabel : String := "DeGiorgi"
def entryModuleName : String := "DeGiorgi.BallExtension.ApproximationControl"
def leanToolchainStr : String := "leanprover/lean4:v4.29.0-rc6"
def mathlibRevStr : String := "5c8398df528176d9c87ccd9226ba8f7c8852d59c"
/-- P6.2（`docs/P6_2_STATUS.md`）: DeGiorgi wraps every file's declarations in
    a `namespace DeGiorgi ... end` matching its directory layout, so a
    declaration's own qualified name and its module path share the same
    prefix — this is what let the P6.1 module-attribution-leak fix
    (`ContDiffBump.mk.congr_simp`) require *both* module and name to match.
    Mathlib does not follow this convention: files under `Mathlib/X/Y.lean`
    are attributed to module `Mathlib.X.Y`, but their declarations typically
    live in a namespace with no `Mathlib.` prefix at all (`CategoryTheory.
    Category`, not `Mathlib.CategoryTheory.Category`). Requiring a name-prefix
    match against a Mathlib-slice project namespace found zero declarations
    — not a bug, a real convention difference. Keep this `true` for DeGiorgi
    (regression-tested); the P6.2 pilot scripts set it `false`. -/
def requireNamePrefixMatch : Bool := true

def isProjectModule (env : Environment) (projectNs : Name) (n : Name) : Bool :=
  match env.getModuleIdxFor? n with
  | some idx => projectNs.isPrefixOf (env.header.moduleNames[idx.toNat]!)
  | none => false

def moduleNameOf (env : Environment) (n : Name) : String :=
  match env.getModuleIdxFor? n with
  | some idx => (env.header.moduleNames[idx.toNat]!).toString
  | none => "?"

def bareName (n : Name) : String :=
  match n with
  | .str _ s => s
  | _ => n.toString

/-- `docs/LEAN_DEPENDENCY_POLICY.md`決定5: Leanの専用述語(`isAuxRecursor`/
    `Meta.isMatcherCore`/`Meta.isEqnThm`/`isPrivateName`/`Name.isInternal*`)
    が捕まえない、構造体・帰納型が自動生成する残りの定形メンバ。
    小さな`inductive`/`def`を実際にコンパイルし、生成された各名前に対する
    上記5述語の値をすべて出力して観測した実名一覧
    （`docs/LEAN_DEPENDENCY_POLICY.md`の表を参照、使い捨てスクリプトは
    リポジトリに残していない）——網羅的な形式的特徴付けだと主張しない、
    経験的な denylist。
    将来のLean/Mathlibが新しい生成物の形を導入したら、ここを拡張する必要が
    ある(P6.2で新しいプロジェクトを走らせるたびに
    `rawConstants \ publishedDependencies \ 対象外名前空間`を目視確認する
    運用手順を`docs/LEAN_DEPENDENCY_POLICY.md`に明記)。 -/
def knownGeneratedSuffixes : List String :=
  ["noConfusion", "noConfusionType", "ctorIdx", "toCtorIdx", "ctorElim",
   "ctorElimType", "sizeOf_spec", "below", "ibelow", "binductionOn", "injEq",
   -- 実データ(DeGiorgi.Cutoff/DeGiorgi.MemW1pWitness)で発見・追加:
   -- `rec`はプリミティブな再帰子で、`isAuxRecursor`は`recOn`/`casesOn`等の
   -- "補助"再帰子だけを認識し、これ自体は捕まえない(実測で確認済み)。
   -- `inj`/`congr_simp`はコンストラクタ(典型的には`<Struct>.mk.inj`/
   -- `<Struct>.mk.congr_simp`)に対して自動導出される補題で、
   -- どの述語にも引っかからなかった。
   "rec", "inj", "congr_simp"]

def hasGeneratedSuffix (n : Name) : Bool :=
  knownGeneratedSuffixes.contains (bareName n)

/-- P6.1: `n`はコンパイラ生成物か実装詳細か(＝人間が書いた依存ではないか)。
    コンストラクタ自身(`Color.red`等)は対象外——それは型を定義する一部として
    人間が書いたものなので、依存として正当。 -/
def isGeneratedOrPrivate (env : Environment) (n : Name) : CoreM Bool := do
  let eqnThm ← Meta.isEqnThm n
  return isPrivateName n
    || n.isInternal
    || n.isInternalDetail
    || n.isImplementationDetail
    || isAuxRecursor env n
    || Meta.isMatcherCore env n
    || eqnThm
    || hasGeneratedSuffix n

structure PublishedDep where
  name : String
  origin : String -- "type" | "body" | "both"
  deriving Inhabited

structure DeclDeps where
  rawConstants : Array String
  published : Array PublishedDep

/-- 型・値(証明項)双方からgetUsedConstantsで直接参照を集める——`declDeps`は
    再帰的にたどらない(`docs/LEAN_DEPENDENCY_POLICY.md`決定2: 推移的依存は
    意図的に除外。理由はgetUsedConstants自体が対象宣言自身の型・値という
    式だけを見るのであって、そこから参照される宣言の中身までは見ないため
    ——推移的関係が欲しければ`publishedDependencies`をグラフとして辿ればよい)。 -/
def declDeps (env : Environment) (projectNs : Name) (info : ConstantInfo) : CoreM DeclDeps := do
  let typeUsed := info.type.getUsedConstants
  let valueUsed := (info.value?.map Expr.getUsedConstants).getD #[]
  let allRaw := typeUsed ++ valueUsed

  let mut rawSeen : Std.HashSet Name := {}
  let mut rawOut : Array Name := #[]
  for n in allRaw do
    if !rawSeen.contains n then
      rawSeen := rawSeen.insert n
      rawOut := rawOut.push n
  let rawSorted := rawOut.qsort (fun a b => a.toString < b.toString)

  let mut typeSeen : Std.HashSet Name := {}
  for n in typeUsed do typeSeen := typeSeen.insert n
  let mut valueSeen : Std.HashSet Name := {}
  for n in valueUsed do valueSeen := valueSeen.insert n

  let mut published : Array PublishedDep := #[]
  for n in rawSorted do
    if n != info.name && isProjectModule env projectNs n then
      let generated ← isGeneratedOrPrivate env n
      if !generated then
        let inType := typeSeen.contains n
        let inValue := valueSeen.contains n
        let origin := if inType && inValue then "both" else if inType then "type" else "body"
        published := published.push { name := bareName n, origin := origin }

  return { rawConstants := rawSorted.map (·.toString), published := published }

#eval show CoreM Unit from do
  let env ← getEnv
  let mut decls : Array (Name × Json) := #[]
  let mut total : Nat := 0
  for (name, info) in env.constants.toList do
    -- `isProjectModule`だけで選ぶと、Lean が使用時にトリガーされて生成する
    -- 補助補題(`@[congr]`のcongr_simp等)が、定義元とは無関係に「たまたま
    -- 最初にトリガーされたファイル」のモジュールへ帰属することがあり、
    -- `ContDiffBump.mk.congr_simp`のようなMathlib名がDeGiorgiモジュール配下の
    -- 「宣言」として紛れ込む(実データで検出——本物のDeGiorgi宣言は必ず
    -- 名前自体も`DeGiorgi`で始まる)。名前の接頭辞と生成物フィルタの両方を
    -- 満たすものだけを対象にする。
    let isGenerated ← isGeneratedOrPrivate env name
    let namePrefixOk := !requireNamePrefixMatch || projectNamespace.isPrefixOf name
    if isProjectModule env projectNamespace name && namePrefixOk && !isGenerated then
      total := total + 1
      let dd ← declDeps env projectNamespace info
      let declJson := Json.mkObj [
        ("name", Json.str (bareName name)),
        ("qualifiedName", Json.str name.toString),
        ("module", Json.str (moduleNameOf env name)),
        ("rawConstants", Json.arr (dd.rawConstants.map Json.str)),
        ("publishedDependencies", Json.arr (dd.published.map (fun p =>
          Json.mkObj [("name", Json.str p.name), ("origin", Json.str p.origin)]))),
        ("filteredOutCount", toJson (dd.rawConstants.size - dd.published.size))
      ]
      decls := decls.push (name, declJson)
  -- 生成時刻はここでは記録しない(壁時計時刻を素直に取る標準APIが無く、
  -- 偽の値を入れるくらいなら省く——`lean_manifest_adapter.rs`側が
  -- 取り込み時刻をつける)。
  --
  -- byte-stable出力(P6.1 Definition of doneの要求): `env.constants.toList`の
  -- 反復順序はHashMapのものであって決定的だと保証されない——宣言を
  -- 完全修飾名で明示的に整列してから書き出す。
  let sortedDecls := (decls.qsort (fun a b => a.1.toString < b.1.toString)).map (·.2)
  let manifest := Json.mkObj [
    ("project", Json.str projectLabel),
    ("leanToolchain", Json.str leanToolchainStr),
    ("mathlibRev", Json.str mathlibRevStr),
    ("entryModule", Json.str entryModuleName),
    ("extractorVersion", Json.str extractorVersion),
    ("filteringPolicyVersion", Json.str filteringPolicyVersion),
    ("totalDeclarations", toJson total),
    ("declarations", Json.arr sortedDecls)
  ]
  IO.println manifest.compress

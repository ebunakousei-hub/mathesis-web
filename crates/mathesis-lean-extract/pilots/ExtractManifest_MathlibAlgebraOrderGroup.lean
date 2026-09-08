-- P6.2（ユーザー指示2026-09-08「a typeclass-heavy and abstraction-heavy
-- project」）: `../ExtractManifest.lean`と全く同じフィルタリングロジックを、
-- DeGiorgi(1本の論文の定理証明)とは全く違う形状のコードへ向けて走らせる——
-- 型クラス階層(Monoid → Group → OrderedCommGroup...)がインスタンス解決を
-- 通じて大量の間接的な依存を生む領域。「このポリシー/フィルタは
-- DeGiorgiの命名・名前空間・ビルド慣習に過剰適合していないか」を検証する
-- のがP6.2の目的そのもの——コードはここでは変えない、変えたら比較にならない。
--
-- `Mathlib.Algebra.Order.Group`名前空間だけを対象にする。「プロジェクト」は
-- 独立したarXiv論文ではなく、Mathlib自身の1サブツリー——理由は
-- `docs/P6_2_STATUS.md`にある: (a) 既に取り込み済みのMathlibチェックアウトを
-- 再利用でき新規ダウンロードが要らない、(b) それでいて命名規約・スケール・
-- 抽象度がDeGiorgiと全く異なる実在コードである、(c)
-- `mathesis-importer <dir> <db> --paper <合成id>`は実際にはarXiv IDの形式を
-- 検証しないので、テキスト抽出との突き合わせも同じ手法でそのまま行える。
--
-- フィルタリングロジック本体(`isProjectModule`〜`declDeps`、
-- `filteringPolicyVersion`/`extractorVersion`)は`../ExtractManifest.lean`と
-- 一字一句同じに保つこと——変えたら「同じポリシーが別プロジェクトでも
-- 動くか」の検証にならない。
import Mathlib.Algebra.Order.Group.Basic

open Lean

def filteringPolicyVersion : String := "mathesis-lean-dependency-filter-v1"

def extractorVersion : String := "mathesis-lean-extract-v2"

def projectNamespace : Name := `Mathlib.Algebra.Order.Group
def projectLabel : String := "Mathlib.Algebra.Order.Group (P6.2 pilot 2, typeclass-heavy)"
def entryModuleName : String := "Mathlib.Algebra.Order.Group.Basic"
def leanToolchainStr : String := "leanprover/lean4:v4.29.0-rc6"
def mathlibRevStr : String := "5c8398df528176d9c87ccd9226ba8f7c8852d59c"
-- P6.2実データで判明(`../ExtractManifest.lean`参照): Mathlibは
-- ファイルパスと宣言の名前空間を一致させない(`Mathlib.Algebra.Order.Group.
-- Basic`というモジュールの中身は`Mathlib.`接頭辞を持たない名前空間で
-- 宣言される)。名前接頭辞まで要求すると0件になった——DeGiorgi固有の
-- 慣習への過剰適合そのもの。モジュール一致だけに絞る。
def requireNamePrefixMatch : Bool := false

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

def knownGeneratedSuffixes : List String :=
  ["noConfusion", "noConfusionType", "ctorIdx", "toCtorIdx", "ctorElim",
   "ctorElimType", "sizeOf_spec", "below", "ibelow", "binductionOn", "injEq",
   "rec", "inj", "congr_simp"]

def hasGeneratedSuffix (n : Name) : Bool :=
  knownGeneratedSuffixes.contains (bareName n)

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

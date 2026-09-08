-- P6.1（`docs/LEAN_DEPENDENCY_POLICY.md`）: `../ExtractManifest.lean`と同じ
-- フィルタ述語を`Fixtures.lean`の10種の対抗ケースにかけ、期待される
-- publishedDependencies集合と正確に一致するかを検査する。1件でも
-- 食い違えば非ゼロで終了する——CI/手動実行どちらでもゲートとして使える。
--
-- このロジックは`../ExtractManifest.lean`の複製。理由は
-- `docs/LEAN_DEPENDENCY_POLICY.md`「Why the filter logic exists in two
-- places」に記録済み(`lake env lean --run`はsourceファイルの`import`を
-- 解決できない——実測して確認済み)。両者は同じ`filteringPolicyVersion`
-- 文字列を持つ——ここが変わったら向こうも変える。
import Lean
import Fixtures.DependencyFixtures

open Lean

def filteringPolicyVersion : String := "mathesis-lean-dependency-filter-v1"

def projectNamespace : Name := `Fixtures

def isProjectModule (env : Environment) (projectNs : Name) (n : Name) : Bool :=
  match env.getModuleIdxFor? n with
  | some idx => projectNs.isPrefixOf (env.header.moduleNames[idx.toNat]!)
  | none => false

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
  origin : String
  deriving Inhabited, BEq

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

/-- 期待値と実測値を比較する。1件のケースぶん。 -/
def checkCase (env : Environment) (declName : Name) (expected : Array PublishedDep) : CoreM Bool := do
  match env.find? declName with
  | none =>
    IO.println s!"FAIL {declName}: declaration not found in environment"
    return false
  | some info =>
    let dd ← declDeps env projectNamespace info
    let actualSorted := dd.published.qsort (fun a b => a.name < b.name)
    let expectedSorted := expected.qsort (fun a b => a.name < b.name)
    if actualSorted == expectedSorted then
      IO.println s!"PASS {declName}: {dd.published.toList.map (fun p => (p.name, p.origin))}"
      return true
    else
      IO.println s!"FAIL {declName}"
      IO.println s!"  expected: {expectedSorted.toList.map (fun p => (p.name, p.origin))}"
      IO.println s!"  actual:   {actualSorted.toList.map (fun p => (p.name, p.origin))}"
      return false

#eval show CoreM Unit from do
  let env ← getEnv
  let mut allOk := true
  let cases : List (Name × Array PublishedDep) := [
    -- 1. 直接の依存
    (`Fixtures.directDep, #[{ name := "baseFact", origin := "body" }]),
    -- 2. 間接依存は推移的に辿らない(wrapperのみ、baseFactは含まない)
    (`Fixtures.viaWrapper, #[{ name := "wrapper", origin := "body" }]),
    -- 3. 一般的な識別子でも完全修飾名で正しく識別する。`rfl`が
    --    `restrict 3 = 3`を証明する項は`restrict 3`という部分項を
    --    デルタ簡約せずそのまま保持する(defeqの判定はkernelがwhnfで
    --    行うのであって、表示される証明項自体を書き換えない)ので、
    --    `restrict`は型・値の両方に現れる。
    (`Fixtures.usesRestrict, #[{ name := "restrict", origin := "both" }]),
    -- 4. 名前空間で修飾された依存もbareNameへ落ちる
    (`Fixtures.usesInnerFact, #[{ name := "innerFact", origin := "body" }]),
    -- 5. 自動生成されたmatcherは自分自身の依存から除かれるが、`Color`は
    --    (型`Color → Nat`にも、パターンマッチの分岐にも現れる)本物の
    --    依存なので正しく残る——生成物だけを狙い撃ちできていることの確認。
    (`Fixtures.colorCode, #[{ name := "Color", origin := "both" }]),
    -- 6. private実装詳細は除かれる
    (`Fixtures.usesPriv, #[]),
    -- 7. 自己参照(再帰)は依存として数えない
    (`Fixtures.countdown, #[]),
    -- 8. 重複参照は集合として1件にまとまる
    (`Fixtures.pairUse, #[{ name := "baseNum", origin := "body" }]),
    -- 9. プロジェクト外(Lean core)の宣言は除かれる
    (`Fixtures.usesExternal, #[]),
    -- 10. 型だけが参照し、値は参照しない
    (`Fixtures.onlyTypeUse, #[{ name := "marker", origin := "type" }]),
    -- 11. 型と値の両方が参照する
    (`Fixtures.usesBothTypeAndValue, #[{ name := "bothDep", origin := "both" }]),
  ]
  for (declName, expected) in cases do
    let ok ← checkCase env declName expected
    allOk := allOk && ok
  -- `lake env lean --run`はスクリプトの成否に関わらず`(interpreter) unknown
  -- declaration 'main'`を出して終える(`ExtractManifest.lean`と同じ既知の
  -- 挙動、`docs/P6_STATUS.md`参照)——プロセス終了コードでは合否を伝えられない
  -- ので、この確定的な1行を呼び出し側(Rust側テスト/人間)がgrepする。
  if allOk then
    IO.println s!"ALL {cases.length} FIXTURE CASES PASSED (filteringPolicyVersion={filteringPolicyVersion})"
  else
    IO.println "FIXTURE TESTS FAILED"

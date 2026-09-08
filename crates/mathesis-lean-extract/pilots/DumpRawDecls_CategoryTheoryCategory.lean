-- P7.1: 同じ手法をP6.2パイロット3のエントリで。`Category.Basic`だけの
-- importでは`Factorisation.lean`/`RelCat.lean`の宣言がそもそも
-- elaboratorの環境に載るのか(=推移的にimportされているのか)自体を
-- この空リスト/非空リストの結果で直接確かめる——named importなしに
-- 到達できないなら、それは「フィルタで落ちた」のではなく
-- 「そもそも環境に無い」という別のカテゴリ。
import Mathlib.CategoryTheory.Category.Basic

open Lean

def targetModulePrefixes : List Name :=
  [`Mathlib.CategoryTheory.Category.Basic, `Mathlib.CategoryTheory.Category.Factorisation, `Mathlib.CategoryTheory.Category.RelCat]

def moduleNameOf (env : Environment) (n : Name) : Option Name :=
  match env.getModuleIdxFor? n with
  | some idx => some (env.header.moduleNames[idx.toNat]!)
  | none => none

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

#eval show CoreM Unit from do
  let env ← getEnv
  let mut decls : Array Json := #[]
  let mut modulesSeen : Std.HashSet Name := {}
  for (name, _) in env.constants.toList do
    match moduleNameOf env name with
    | some m =>
      modulesSeen := modulesSeen.insert m
      if targetModulePrefixes.any (fun p => p.isPrefixOf m) then
        let generated ← isGeneratedOrPrivate env name
        decls := decls.push (Json.mkObj [
          ("qualifiedName", Json.str name.toString),
          ("module", Json.str m.toString),
          ("isGeneratedOrPrivate", Json.bool generated)
        ])
    | none => pure ()
  -- 直接的な証拠として、Factorisation/RelCatのモジュール自体が
  -- 環境にロードされているかどうかも別途出す。
  let factorisationLoaded := modulesSeen.contains `Mathlib.CategoryTheory.Category.Factorisation
  let relCatLoaded := modulesSeen.contains `Mathlib.CategoryTheory.Category.RelCat
  IO.println (Json.mkObj [
    ("factorisationModuleLoaded", Json.bool factorisationLoaded),
    ("relCatModuleLoaded", Json.bool relCatLoaded),
    ("declarations", Json.arr decls)
  ]).compress

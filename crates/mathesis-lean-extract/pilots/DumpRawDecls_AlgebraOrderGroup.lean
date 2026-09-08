-- P7.1（ユーザー指示2026-09-08「why do 62 of 63 not match」）: P6.2の
-- パイロット2と全く同じエントリ(`import Mathlib.Algebra.Order.Group.Basic`)
-- から、フィルタを一切かけずに`env.constants`を生ダンプする——
-- `isGeneratedOrPrivate`済みかどうかのフラグだけ付けて、判定は後段
-- (Python側の突き合わせ)に委ねる。目的は「Math-Graphが報告する宣言が、
-- Mathesis自身のこのリビジョン・このエントリ経路で本当にelaboratorの
-- 環境に載るか」を実測で確かめること——テキストgrepでは`@[simps]`由来の
-- 自動生成宣言や名前無しinstanceの実在を判定できないと分かったため
-- (`docs/P7_1_STATUS.md`)、`ExtractManifest.lean`と同じ`getUsedConstants`
-- 経由ではなく、環境そのものを見る。
import Mathlib.Algebra.Order.Group.Basic

open Lean

def targetModulePrefixes : List Name := [`Mathlib.Algebra.Order.Group.Synonym, `Mathlib.Algebra.Order.Group.Action.Synonym]

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
  for (name, _) in env.constants.toList do
    match moduleNameOf env name with
    | some m =>
      if targetModulePrefixes.any (fun p => p.isPrefixOf m) then
        let generated ← isGeneratedOrPrivate env name
        decls := decls.push (Json.mkObj [
          ("qualifiedName", Json.str name.toString),
          ("module", Json.str m.toString),
          ("isGeneratedOrPrivate", Json.bool generated)
        ])
    | none => pure ()
  IO.println (Json.arr decls).compress

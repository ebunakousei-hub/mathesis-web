-- Priority 2, step 1（ユーザー指示 2026-09-08）: 本物のLean elaboratorから
-- 依存関係マニフェストを取り出す。`crates/mathesis-provenance/src/
-- lean_manifest_adapter.rs`が読むJSONを標準出力へ書く。
--
-- `DeGiorgi`名前空間で定義された宣言だけを対象にし、各宣言の型・値
-- (証明項)が実際に参照している**同じDeGiorgi名前空間内の**他の宣言だけを
-- `dependsOn`として記録する（Mathlib側の補題は含めない——比較対象の
-- `judgment_dependencies`がjudgment同士の依存だけを見ているのと同じ
-- 粒度に合わせる）。テキスト抽出のような名前の**出現**ではなく、
-- 型検査済みの項が実際に**参照している**定数を`Expr.getUsedConstants`で
-- 機械的に取り出す——ここに推測・ヒューリスティックは無い。
import DeGiorgi.BallExtension.ApproximationControl

open Lean

def isDeGiorgiModule (env : Environment) (n : Name) : Bool :=
  match env.getModuleIdxFor? n with
  | some idx => (`DeGiorgi).isPrefixOf (env.header.moduleNames[idx.toNat]!)
  | none => false

def moduleNameOf (env : Environment) (n : Name) : String :=
  match env.getModuleIdxFor? n with
  | some idx => (env.header.moduleNames[idx.toNat]!).toString
  | none => "?"

def bareName (n : Name) : String :=
  match n with
  | .str _ s => s
  | _ => n.toString

def declDeps (env : Environment) (info : ConstantInfo) : Array Name := Id.run do
  let used := info.type.getUsedConstants ++ (info.value?.map Expr.getUsedConstants).getD #[]
  let filtered := used.filter (fun n => n != info.name && isDeGiorgiModule env n)
  -- 手書きdedup（Std/BatteriesのList.dedup/eraseDupsの版差を避ける）。
  let mut seen : Std.HashSet Name := {}
  let mut out : Array Name := #[]
  for n in filtered do
    if !seen.contains n then
      seen := seen.insert n
      out := out.push n
  return out

#eval show CoreM Unit from do
  let env ← getEnv
  let mut decls : Array Json := #[]
  let mut total : Nat := 0
  for (name, info) in env.constants.toList do
    if isDeGiorgiModule env name then
      total := total + 1
      let deps := declDeps env info
      decls := decls.push (Json.mkObj [
        ("name", Json.str (bareName name)),
        ("qualifiedName", Json.str name.toString),
        ("module", Json.str (moduleNameOf env name)),
        ("dependsOn", Json.arr (deps.map (fun d => Json.str (bareName d))))
      ])
  -- 生成時刻はここでは記録しない(壁時計時刻を素直に取る標準APIが無く、
  -- 偽の値を入れるくらいなら省く——`lean_manifest_adapter.rs`側が
  -- 取り込み時刻をつける)。
  let manifest := Json.mkObj [
    ("project", Json.str "DeGiorgi"),
    ("leanToolchain", Json.str "leanprover/lean4:v4.29.0-rc6"),
    ("mathlibRev", Json.str "5c8398df528176d9c87ccd9226ba8f7c8852d59c"),
    ("entryModule", Json.str "DeGiorgi.BallExtension.ApproximationControl"),
    ("totalDeclarations", toJson total),
    ("declarations", Json.arr decls)
  ]
  IO.println manifest.compress

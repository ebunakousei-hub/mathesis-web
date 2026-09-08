-- P6.1（`docs/LEAN_DEPENDENCY_POLICY.md`）: `RunFixtureTests.lean`が
-- `../ExtractManifest.lean`と同じフィルタ述語を対抗ケースにかけて検証する
-- 対象。Lean core/std のみに依存（Mathlibなし——`lakefile.lean`参照）。

namespace Fixtures

-- 1. 直接の依存
theorem baseFact : (1 : Nat) = 1 := rfl
theorem directDep : (1 : Nat) = 1 := baseFact

-- 2. 定義の裏に隠れた間接依存(推移的には辿らない、という決定の確認)
def wrapper : (1 : Nat) = 1 := baseFact
theorem viaWrapper : (1 : Nat) = 1 := wrapper

-- 3. 一般的すぎて衝突しうる識別子("restrict"はSet.restrict等とも衝突する
--    名前——だが完全修飾名で識別するので取り違えない)
def restrict (n : Nat) : Nat := n
theorem usesRestrict : restrict 3 = 3 := rfl

-- 4. 名前空間で修飾された依存
namespace Inner
theorem innerFact : True := trivial
end Inner
theorem usesInnerFact : True := Inner.innerFact

-- 5. 自動生成された宣言(matchコンパイラの補助関数)。colorCode自身の
--    elaborated valueが`colorCode.match_1`を直接参照する——これを
--    publishedDependenciesへ漏らしてはいけない。
inductive Color where
  | red | green | blue

def colorCode : Color → Nat
  | .red => 0
  | .green => 1
  | .blue => 2

-- 6. private実装詳細
private def helperPriv : Nat := 42
theorem usesPriv : helperPriv = 42 := rfl

-- 7. 自己参照(再帰定義) — 自分自身への依存として数えてはいけない
def countdown : Nat → Nat
  | 0 => 0
  | n+1 => countdown n

-- 8. 重複参照 — publishedDependenciesは集合として1件にまとまる
def baseNum : Nat := 7
def pairUse : Nat × Nat := (baseNum, baseNum)

-- 9. インポートされた外部(このプロジェクト名前空間の外)の宣言
--    (Lean core自身の`Nat.le_refl`——Mathlibを引かずに検証できる)
theorem usesExternal : (1 : Nat) ≤ 1 := Nat.le_refl 1

-- 10. 型だけが参照し、値は参照しない。`def`の値がラムダなら引数の型注釈が
--     値の項構造にも入り、`theorem`の証明も暗黙引数として言明の型を
--     引きずることが多い(`Or.inr`の暗黙のleft-disjunct引数等)ため、
--     確実に型だけになるのは値そのものを持たない`axiom`のケース
--     (`ConstantInfo.value?`が`none`——決定3「axiom/opaqueはvalueを
--     持たない」の直接の帰結)。
def marker : Nat := 11
axiom onlyTypeUse : marker = marker

-- 11. 型と値の両方が参照する("both"タグの確認)
def bothDep : Nat := 9
theorem usesBothTypeAndValue : bothDep = bothDep := rfl

end Fixtures

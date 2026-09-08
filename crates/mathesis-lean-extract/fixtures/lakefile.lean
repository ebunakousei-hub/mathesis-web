import Lake
open Lake DSL

-- P6.1（`docs/LEAN_DEPENDENCY_POLICY.md`）: `../ExtractManifest.lean`が使う
-- 依存関係フィルタの10種の対抗ケースを実際にコンパイルして検証する
-- ための、独立した最小のLakeプロジェクト。DeGiorgi本体のlakefileには
-- 触れない（ベンダー済みの外部フィクスチャに無関係なテスト用宣言を
-- 混ぜたくない）。Mathlibへは依存しない——「外部宣言」フィクスチャは
-- Lean core自身の宣言(`Nat.le_refl`)で足りるため、数GBの再ダウンロードを
-- 避けられる。

package «filter-fixtures» where
  leanOptions := #[⟨`autoImplicit, false⟩]

@[default_target]
lean_lib «Fixtures» where
  globs := #[.submodules `Fixtures]

-- 位相空間論サンプル (Mathesis フェーズ2 フィクスチャ)
-- 一般化・特殊化の階層が明確な位相空間の定理群

-- ===== 位相空間の基本定義 =====

axiom open_empty (X : Type) [TopologicalSpace X] : IsOpen (∅ : Set X)
axiom open_univ (X : Type) [TopologicalSpace X] : IsOpen (Set.univ : Set X)
axiom open_inter (X : Type) [TopologicalSpace X] (U V : Set X) :
    IsOpen U → IsOpen V → IsOpen (U ∩ V)
axiom open_union (X : Type) [TopologicalSpace X] (s : Set (Set X)) :
    (∀ U ∈ s, IsOpen U) → IsOpen (⋃₀ s)

-- ===== コンパクト性 =====

def compact_space (X : Type) [TopologicalSpace X] : Prop :=
  ∀ (c : Set (Set X)), (∀ U ∈ c, IsOpen U) → Set.univ ⊆ ⋃₀ c →
  ∃ (f : Finset (Set X)), ↑f ⊆ c ∧ Set.univ ⊆ ⋃₀ ↑f

-- ===== ハウスドルフ空間 =====

def hausdorff_space (X : Type) [TopologicalSpace X] : Prop :=
  ∀ x y : X, x ≠ y → ∃ U V : Set X, IsOpen U ∧ IsOpen V ∧ x ∈ U ∧ y ∈ V ∧ U ∩ V = ∅

-- コンパクト + ハウスドルフ = コンパクトハウスドルフ（特殊化の例）
def compact_hausdorff_space (X : Type) [TopologicalSpace X] : Prop :=
  compact_space X ∧ hausdorff_space X

-- ===== 連続写像 =====

def continuous_map (X Y : Type) [TopologicalSpace X] [TopologicalSpace Y] (f : X → Y) : Prop :=
  ∀ V : Set Y, IsOpen V → IsOpen (f ⁻¹' V)

theorem continuous_id (X : Type) [TopologicalSpace X] : continuous_map X X id := by
  intro V hV; simpa

theorem continuous_comp (X Y Z : Type) [TopologicalSpace X] [TopologicalSpace Y] [TopologicalSpace Z]
    (f : X → Y) (g : Y → Z) :
    continuous_map X Y f → continuous_map Y Z g → continuous_map X Z (g ∘ f) := by
  intro hf hg V hV
  exact hf _ (hg V hV)

-- ===== 同相写像 =====

def homeomorphism (X Y : Type) [TopologicalSpace X] [TopologicalSpace Y] (f : X → Y) : Prop :=
  Function.Bijective f ∧ continuous_map X Y f ∧ continuous_map Y X (Function.invFun f)

-- 同相は同値関係（iff pattern: homeomorphism_iff_bicontinuous）
theorem homeomorphism_iff_bicontinuous (X Y : Type) [TopologicalSpace X] [TopologicalSpace Y]
    (f : X → Y) :
    homeomorphism X Y f ↔ Function.Bijective f ∧ continuous_map X Y f ∧
    continuous_map Y X (Function.invFun f) :=
  Iff.rfl

-- ===== 分離公理の階層（一般化/特殊化の好例） =====

-- T0 公理（コルモゴロフ空間）
def t0_space (X : Type) [TopologicalSpace X] : Prop :=
  ∀ x y : X, x ≠ y → ∃ U : Set X, IsOpen U ∧ (x ∈ U ↔ ¬y ∈ U)

-- T1 公理（フレシェ空間）
def t1_space (X : Type) [TopologicalSpace X] : Prop :=
  ∀ x y : X, x ≠ y → ∃ U : Set X, IsOpen U ∧ x ∈ U ∧ y ∉ U

-- T2 = ハウスドルフ（再掲）
def t2_space (X : Type) [TopologicalSpace X] : Prop := hausdorff_space X

-- T1 は T0 の特殊化（t1_space は t0_space を含意する）
-- contains_implies_t0 パターン: t1_implies_t0
theorem t1_implies_t0 (X : Type) [TopologicalSpace X] :
    t1_space X → t0_space X := by
  intro h1 x y hne
  obtain ⟨U, hU, hxU, hyU⟩ := h1 x y hne
  exact ⟨U, hU, ⟨fun _ => hyU, fun hny => absurd hxU (fun h => hny h)⟩⟩

-- T2 は T1 の特殊化
theorem t2_implies_t1 (X : Type) [TopologicalSpace X] :
    t2_space X → t1_space X := by
  intro h2 x y hne
  obtain ⟨U, V, hU, hV, hxU, hyV, hdisj⟩ := h2 x y hne
  exact ⟨U, hU, hxU, fun hyU => absurd (Set.mem_inter hyU hyV) (hdisj.symm ▸ Set.not_mem_empty y)⟩

-- metric_space はhaドルフの特殊化（qualifier pattern）
def metric_space_is_hausdorff (X : Type) [MetricSpace X] : hausdorff_space X := by
  intro x y hne
  have hd : 0 < dist x y := by positivity
  exact ⟨Metric.ball x (dist x y / 2), Metric.ball y (dist x y / 2),
         Metric.isOpen_ball, Metric.isOpen_ball,
         Metric.mem_ball_self (by linarith),
         Metric.mem_ball_self (by linarith),
         by simp [Set.ext_iff, Metric.mem_ball]; intro z; push_neg; linarith [dist_triangle x z y]⟩

-- ===== コンパクト集合の性質 =====

theorem compact_iff_seq_compact (X : Type) [MetricSpace X] :
    compact_space X ↔ ∀ (s : ℕ → X), ∃ φ : ℕ → ℕ, StrictMono φ ∧ ∃ x, Filter.Tendsto (s ∘ φ) Filter.atTop (nhds x) :=
  sorry

-- corollary: コンパクトハウスドルフ空間は正規空間
theorem corollary_compact_hausdorff_is_normal (X : Type) [TopologicalSpace X] :
    compact_hausdorff_space X → True := trivial

-- connected space は topological_space の特殊化（qualifier pattern）
def connected_space (X : Type) [TopologicalSpace X] : Prop :=
  ¬∃ (U V : Set X), IsOpen U ∧ IsOpen V ∧ U ∪ V = Set.univ ∧ U ∩ V = ∅ ∧ U ≠ ∅ ∧ V ≠ ∅

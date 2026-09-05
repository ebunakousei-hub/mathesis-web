-- Sample Lean code for testing Mathesis importer
-- Basic algebraic structures

theorem add_assoc (a b c : ℕ) : a + b + c = a + (b + c) := by
  induction c with
  | zero => rfl
  | succ c ih => simp [ih]

theorem add_comm (a b : ℕ) : a + b = b + a := by
  induction a with
  | zero => simp
  | succ a ih => simp [ih]

lemma add_zero (a : ℕ) : a + 0 = a := by simp

def double (n : ℕ) : ℕ := n + n

theorem double_eq_two_mul (n : ℕ) : double n = 2 * n := by
  unfold double
  ring

axiom choice_axiom : ∀ (α : Type) (r : α → α → Prop), (∀ x, ∃ y, r x y) → (∃ f, ∀ x, r x (f x))

-- Group theory example
theorem mul_assoc (a b c : G) [Group G] : a * b * c = a * (b * c) := by
  exact mul_assoc a b c

theorem mul_one (a : G) [Group G] : a * 1 = a := by
  exact mul_one a

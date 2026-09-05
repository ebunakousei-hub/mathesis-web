-- Advanced Lean code to test parser robustness
-- Logic and Set Theory examples

theorem De_Morgan_law (P Q : Prop) : ¬(P ∧ Q) = (¬P ∨ ¬Q) := by
  constructor
  · intro h
    by_cases hp : P
    · right
      intro hq
      exact h ⟨hp, hq⟩
    · left
      exact hp
  · intro h
    intro ⟨hp, hq⟩
    cases h with
    | inl hnp => exact hnp hp
    | inr hnq => exact hnq hq

theorem subset_antisymmetry (A B : Set α) : A ⊆ B → B ⊆ A → A = B := by
  intro hab hba
  ext x
  exact ⟨hab x, hba x⟩

def image (f : α → β) (s : Set α) : Set β := { y | ∃ x ∈ s, f x = y }

lemma image_empty (f : α → β) : image f ∅ = ∅ := by
  unfold image
  simp

theorem composition_assoc (f : α → β) (g : β → γ) (h : γ → δ) :
    (h ∘ g) ∘ f = h ∘ (g ∘ f) := by
  rfl

axiom excluded_middle (P : Prop) : P ∨ ¬P

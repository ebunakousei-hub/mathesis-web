# Phase 8.5 status — hierarchy-classification spot-check against real Lean source

> Written 2026-09-10, in response to the user's review of P8.1–P8.4
> ("Start the hierarchy spot-check first"). This is an empirical
> investigation, not a new feature — no application code changed.
> Verdict up front: **the classification rule is a real, useful signal
> but not the "trivial/content-free" guarantee its name implies** —
> confirmed by reading actual, current Lean source, not by re-reading
> the pipeline code.

## The question being tested

P7.3's rule (`kind ∈ {inst, instance}` AND zero outgoing `proof`-type
dependency edges → `typeclass_hierarchy`) was derived and validated
against Mathlib specifically. P8.1 and P8.4 explicitly flagged applying
it to FLT/carleson/PrimeNumberTheoremAnd/pfr as an "unvalidated
extrapolation" rather than a confirmed fact
(`docs/P8_1_STATUS.md`/`docs/P8_4_STATUS.md`). This pass checks that
extrapolation against something the pipeline itself never had access
to: the real, current Lean source of the classified declarations.

## Method

Sampled 5 declarations per non-Mathlib project (20 total) from the
already-generated `safe_statements_p8_1.json`/`safe_statements_p8_4.json`
(the exact declarations P8.1/P8.4 imported as `typeclass_hierarchy`),
random seed 42, no cherry-picking. For each, fetched the real file at
`filePath` from the project's actual public GitHub repo (`gh api
repos/<owner>/<repo>/contents/<path>`) — these are the projects' own
open-source Lean code, not the licensed Math-Graph dataset, so no
licensing concern in reading them — and read the declaration's real
body to judge: is this a short, structural composition (the rule's
implicit claim), or does it contain a substantive tactic proof?

Repos used (found via `gh search repos`, not guessed):
`ImperialCollegeLondon/FLT`, `fpvandoorn/carleson`,
`AlexKontorovich/PrimeNumberTheoremAnd`, `teorth/pfr`.

## Finding 1 (the one being tested): the rule is a real signal, not a guarantee

Of 20 sampled names, 14 were found in the live repo (6 not found — see
"a second finding" below for why). Most matches were exactly what the
rule's name implies — short, term-mode delegations to an existing
instance or lemma, no tactic proof at all:

```lean
-- FLT: FLT/Mathlib/NumberTheory/NumberField/InfiniteAdeleRing.lean
instance : T2Space (InfiniteAdeleRing K) := inferInstanceAs <| T2Space (Π _, _)

-- carleson: Carleson/TileStructure.lean
instance : PartialOrder (𝔓 X) := PartialOrder.lift toTileLike toTileLike_injective

-- PrimeNumberTheoremAnd: PrimeNumberTheoremAnd/Wiener.lean
instance instMeasurableSpace : MeasurableSpace Circle := inferInstanceAs <| MeasurableSpace <| Subtype _

-- pfr: PFR/ForMathlib/FiniteRange/Defs.lean
instance FiniteRange.div' ... := FiniteRange.div ..
```

But **two of the matched declarations have real, substantive tactic
proofs** despite being classified `typeclass_hierarchy` — direct
counterexamples to reading the label as "content-free":

```lean
-- FLT: FLT/Deformations/Algebra/InverseLimit/Basic.lean
instance : Group (InverseLimit G f) where
  inv x := ⟨x⁻¹, by simp⟩
  div a b := ⟨a.1 / b.1, by simp⟩
  zpow n x := ⟨x^n, by simp⟩
  div_eq_mul_inv a b := by ext i; ...
  zpow_succ' n x := by ext i; rw [mul_def]; exact (instGroupG i).zpow_succ' n (x.1 i)
  zpow_neg' n x := by ext i; exact (instGroupG i).zpow_neg' n (x.1 i)
  inv_mul_cancel a := by ext i; simp

-- pfr: PFR/Mathlib/Probability/Kernel/Composition/Comp.lean (likely match for
-- "instIsMarkovKernelProdDeleteRight" — see caveat below)
instance ... : IsMarkovKernel (deleteRight κ) := by
  rw [deleteRight_eq]; apply IsMarkovKernel.map _ (by fun_prop)
```

**Reading**: `zero outgoing proof-type edges` in Math-Graph's own
extraction apparently does not mean "no tactic proof was written" — a
`by simp`/`by ext i`/`by fun_prop`-style proof that doesn't cite
another *named declaration* as an explicit term-level dependency can
still elaborate with zero recorded `proof`-type edges. The rule is
measuring something narrower and more technical ("no recorded
proof-edge dependency") than what `typeclass_hierarchy` as a label
suggests to a reader ("structural, doesn't need independent
verification"). This doesn't make the rule useless — most of the
sample really is structural — but the label oversells what's actually
guaranteed. `docs/DATA_DICTIONARY.md` and the P8.1/P8.4 manifests
should describe this status as "schema-classified as
non-substantive-proof, per Math-Graph's own edge extraction — not
independently confirmed content-free," not as "structural."

## Finding 2 (unexpected): a chunk of "PrimeNumberTheoremAnd" isn't about the prime number theorem

4 of the 5 sampled PrimeNumberTheoremAnd declarations had `filePath`s
under `LeanCert/` or `Architect/` — top-level directories that **do
not exist anywhere in the current repo** (confirmed via the full repo
file tree, 340 files, not just a single missing file — `gh api
repos/.../git/trees/main?recursive=true`). The one genuine
`PrimeNumberTheoremAnd/` file in the sample (`Wiener.lean`) matched
fine.

This means Math-Graph's LeanGraph snapshot of "PrimeNumberTheoremAnd"
included substantial content that, at least under the project's
current structure, isn't part of the prime-number-theorem
formalization at all — apparently auxiliary tooling (an
interval-arithmetic certification engine under `LeanCert`, a
JSON/build tool under `Architect`). This wasn't something the pipeline
could have caught (it never reads file content or project structure
beyond scope/dependency CSVs) — it only surfaced by fetching the real
repo. It doesn't invalidate P8.1's per-project declaration counts
(they're accurate counts of *what the dataset attributes to this
project*), but it means "PrimeNumberTheoremAnd" as a label in
`docs/P8_1_STATUS.md`'s "topical variety" framing overstates how much
of its 109 `typeclass_hierarchy` / 5,108 total declarations are
actually about the PNT — a real portion is unrelated tooling code.
Whether this is Math-Graph's dataset construction pulling in a
dependency's content, or the repo having since split these directories
out into something else, wasn't investigated further (out of scope for
a spot-check; would need Math-Graph's own commit-pinning info, which
this pilot doesn't retain per-file).

## Caveats on the check itself

- **6 of 20 sampled names weren't found** in the live repos at all
  (2 FLT, 1 carleson, 3 more effectively unreachable PrimeNumberTheoremAnd
  paths beyond the LeanCert/Architect ones already counted). These are
  fast-moving research repos; Math-Graph's snapshot is from a fixed
  revision, live `main`/`master` has moved since. Where a name wasn't
  found verbatim, a plausible renamed/refactored successor was searched
  for by topic and reported as such, not presented as a certain match —
  every "likely match" above is labeled as such.
- This is a spot-check (20 of 704 typeclass_hierarchy declarations,
  ~2.8%), not an exhaustive audit. It's enough to establish that the
  rule has a real, non-zero false-classification rate for "content-free"
  and that PrimeNumberTheoremAnd's content mix needs a caveat — it is
  not enough to state a precise false-positive rate.
- Fetched via each project's own public GitHub repo under its own
  open-source license (code, not the Math-Graph processed dataset) —
  no licensing question distinct from reading any other public
  open-source project's code.

## What this changes

- `docs/P8_1_STATUS.md`/`docs/P8_4_STATUS.md`'s existing "open
  methodological question" framing was correct to flag but is now
  backed by concrete counterexamples rather than an abstract concern —
  worth a forward-reference to this document.
- Any future UI/export text describing `typeclass_hierarchy` records
  should avoid language implying "verified structural, no proof
  needed" — "schema-classified, not independently content-verified" is
  the honest framing.
- PrimeNumberTheoremAnd's description in prior docs should note that a
  meaningful fraction of its declarations are unrelated tooling, not
  prime-number-theorem content.

## Not done in this pass

- No code or schema changes — this was purely an empirical check.
- No exhaustive audit (704 declarations, not 20).
- No investigation into *why* Math-Graph's proof-edge extraction misses
  proofs like the two found here (would require understanding its own
  elaborator-level extraction methodology, which this project has
  never had access to beyond the dataset's own CSVs).
- No re-classification or re-import — the existing pilot DB and its
  `typeclass_hierarchy` labels are unchanged; this document adds a
  caveat, it doesn't retract the data.

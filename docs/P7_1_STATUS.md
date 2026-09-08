# Phase 7.1 status — why 62 of 63 declarations don't match, and a source-attribution UI

> Written 2026-09-08, executing the user's P7.1 instruction: before
> broadening the Math-Graph pilot, determine *why* P7's declaration-name
> overlap with P6.2 was 1/63, classify the mismatches by cause, produce a
> mapping table, and add source-specific attribution to the UI/provenance
> records. No new data was downloaded — this reuses the pilot files from
> P7 (`scratch/math_graph_pilot/`) plus Mathesis's own already-vendored
> Mathlib checkout (`fixtures/arxiv/DeGiorgi/.lake/packages/mathlib/`).

## Method: stop trusting text search, ask the elaborator directly

P7's own overlap check compared declared *names* as strings. That answers
"do the label strings match," not "does this declaration exist in
Mathesis's pinned revision" — grepping Mathesis's checkout for Math-Graph's
declaration names (tried first, see below) turned out to be unable to
answer that question at all for a large share of cases, because typeclass
`instance` declarations and `@[simps]`-generated lemmas are often
**elaborator-synthesized names that never appear as literal text in the
source**. So this pass wrote two new one-off Lean scripts
(`crates/mathesis-lean-extract/pilots/DumpRawDecls_AlgebraOrderGroup.lean`,
`DumpRawDecls_CategoryTheoryCategory.lean`) that enter through the
**exact same imports P6.2's own pilots used, unchanged**
(`import Mathlib.Algebra.Order.Group.Basic`,
`import Mathlib.CategoryTheory.Category.Basic`) and dump every constant
Lean's own elaborator actually has for the target modules — ground truth,
not a proxy for it.

## Revisions: what could be confirmed, and what could not

| | Mathesis (P6.2) | Math-Graph (P7) |
| --- | --- | --- |
| Lean toolchain | `leanprover/lean4:v4.29.0-rc6` | `v4.2.9` (per `paper_lean_repo.csv`) |
| Mathlib revision | `5c8398df528176d9c87ccd9226ba8f7c8852d59c` (2026-03-10) | not recorded (`mathlib_rev` empty) |
| Git commit | same as above | not recorded (`git_commit` empty) |

The `v4.2.9`/`v4.2.8`/`v4.2.7` toolchain strings for repo slugs
`Mathlib_v429`/`v428`/`v427` are almost certainly `v4.29`/`v4.28`/`v4.27`
with a formatting artifact (a dot inserted before the last digit) — not
literally the historical Lean `v4.2.9` release from 2023, which would be
a completely different scale of drift. Reasoned from the naming pattern,
not confirmed against Math-Graph's own code. Either way, **Math-Graph's
own metadata cannot pin an exact Mathlib commit** — revision mismatch
cannot be ruled out from their side, and is addressed empirically below
by testing whether each declaration exists in Mathesis's own pinned
commit, rather than by trying to compare git hashes that don't exist.

## The actual classification (63 declarations, ground-truth checked)

| Category | Count | % |
| --- | --- | --- |
| (a) File-path membership vs. transitive-import reachability | 18 | 29% |
| Genuine match | 1 | 2% |
| (b)/(c)/(d)/(e) name normalization / namespace / revision / generated-private filtering | 0 | 0% |
| **New: declaration-identity-model mismatch** (see below) | 44 | 70% |

**(a) Reachability — proven, not inferred (18 declarations).** All 10
`Mathlib.Algebra.Order.Group.Action.Synonym` declarations and all 8
`Mathlib.CategoryTheory.Category.{Factorisation,RelCat}` declarations
Math-Graph reports are **not in Mathesis's elaborator environment at
all** when entered through P6.2's own unmodified entry points. The raw
dump script's own `factorisationModuleLoaded`/`relCatModuleLoaded` flags
came back `false`, and `Action.Synonym` is entirely absent from the 486
declarations the algebra script did load. This is not a filtering
decision Mathesis's own pipeline made — those files are simply never
imported, directly or transitively, by `Mathlib.Algebra.Order.Group.Basic`
or `Mathlib.CategoryTheory.Category.Basic`. Math-Graph's own selection
evidently works by file-path membership under the namespace
(whatever `.lean` files exist under `Mathlib/Algebra/Order/Group/` and
`Mathlib/CategoryTheory/Category/`), independent of any one entry
point's import graph.

**Genuine match (1 declaration).**
`LibraryNote.universe_output_parameters_and_typeclass_caching` — a
documentation library-note, present, non-generated, in both P6.2's
published set and Math-Graph's list, in Mathesis's pinned revision. Not
a theorem or a definition, but a real, correctly-identified shared
artifact.

**Filtering, normalization, namespace, and (in the narrow "exact commit
differs" sense) revision mismatch: not observed as a cause here (0
declarations).** Checked directly: of the 45 remaining declarations whose
module *is* loaded in Mathesis's environment, **zero** exist in the raw,
unfiltered dump under `isGeneratedOrPrivate: true` — meaning P6.1/P6.2's
generated/private filter never had the chance to exclude any of them,
because they were never in the raw environment to begin with. This rules
out "Mathesis filtered them out" as an explanation for this dataset.

**New category — declaration-identity-model mismatch (44
declarations, the dominant cause).** Not one of the user's six
categories; closest is "genuinely different extraction scope," but the
mechanism is more specific and worth naming precisely. Concrete evidence:

- Math-Graph's 63-declaration slice includes **50 `OrderDual.*`
  entries** — `instAdd`, `instMul`, `instMonoid`, `instSemigroup`,
  `instCommGroup`, `instMulOneClass`, `instLeftCancelSemigroup`,
  `instIsCancelMul`, `instDivisionMonoid`, and so on — effectively one
  entry per typeclass in the `Add`/`Mul` → ... → `Group`/`CommGroup`
  hierarchy.
- Mathesis's raw, unfiltered ground-truth dump for the same file
  (`Mathlib/Algebra/Order/Group/Synonym.lean`) has exactly **12 real
  top-level `OrderDual.*` instances**: `instAddCancelCommMonoid`,
  `instAddCommMonoid`, `instAddGroup`, `instCancelCommMonoid`,
  `instCommMonoid`, `instGroup`, `instPow`, `instPow'`, `instSMul`,
  `instSMul'`, `instVAdd`, `instVAdd'` (plus their auto-generated
  `.eq_1` equation lemmas). The source literally declares a handful of
  *bundled* instances (`instance : Group (OrderDual α) := ...`); every
  lower typeclass in the hierarchy (`Monoid`, `Semigroup`,
  `MulOneClass`, ...) is reached in real Lean via the `extends` chain's
  projection functions, not as its own separately-named top-level
  declaration.
- Same pattern in CategoryTheory: Math-Graph reports
  `CategoryTheory.Category.mk'` (with a trailing prime). Mathesis's raw
  dump of `Mathlib.CategoryTheory.Category.Basic` (113 declarations,
  checked in full) has `CategoryTheory.Category.mk` — no primed variant
  anywhere.

The most plausible explanation, given this pattern: Math-Graph's
LeanGraph node model for typeclass-heavy code appears to represent the
**resolved/derived typeclass-instance graph** (one node per typeclass a
type is shown to satisfy, matching the project's own stated interest in
"indirect dependencies produced by typeclass elaboration" — this exact
framing was the reason this namespace was picked as a P6.2 pilot in the
first place), rather than **literal declarations read off `env.constants`**
the way Mathesis's own `mathesis-lean-extract` does. This is inferred
from the data, not confirmed by reading Math-Graph's own extraction
code or a methods section — the arXiv PDF's available text doesn't spell
out node-construction mechanics precisely enough to confirm it directly,
and that gap is stated here rather than papered over.

## Mapping table (representative sample; full data in `scratch/math_graph_pilot/`, git-ignored)

| Math-Graph `declName` | Module | Category | Mathesis ground truth |
| --- | --- | --- | --- |
| `LibraryNote.universe_output_parameters_and_typeclass_caching` | `...Category.Basic` | genuine match | present, non-generated |
| `OrderDual.instMulAction_1` | `...Action.Synonym` | (a) reachability | module not loaded from `Category.Basic`/`Group.Basic` entry |
| `CategoryTheory.Factorisation.id_h` | `...Category.Factorisation` | (a) reachability | module not loaded (`factorisationModuleLoaded: false`) |
| `CategoryTheory.instInhabitedRelCat` | `...Category.RelCat` | (a) reachability | module not loaded (`relCatModuleLoaded: false`) |
| `OrderDual.instMonoid` | `...Group.Synonym` | declaration-identity-model mismatch | module loaded (486 decls), name absent; real instance is `OrderDual.instGroup` (Monoid is derived via `extends`) |
| `OrderDual.instPow_1` | `...Group.Synonym` | declaration-identity-model mismatch | module loaded, name absent; closest real name is `OrderDual.instPow'` |
| `CategoryTheory.Category.mk'` | `...Category.Basic` | declaration-identity-model mismatch | module loaded (113 decls), only bare `CategoryTheory.Category.mk` exists |
| `CategoryTheory.Factorisation.instQuiver` | `...Category.Factorisation` | (a) reachability (also would be model-mismatch even if loaded — file has an anonymous `instance : Quiver ...`, not one named `instQuiver`) | module not loaded |

## Source-specific attribution: implemented, in the shared code path

Added `assertion_export::source_kind_label(provider: &str) -> String` —
a small, explicit lookup from `SourceRecord.provider` to a human-readable
explanation, covering every provider that actually exists in this
codebase (`lean-elaborator` → Mathesis's own Lean build,
`mathesis-legacy-snapshot` → Mathesis's own text extraction, `math-graph`
→ "External dataset: Math-Graph (uw-math-ai, CC BY 4.0) — not
independently verified by Mathesis", `openalex`, `msc2020`,
`manual-review`; anything unrecognized passes the raw provider string
through unchanged rather than guessing). Wired into
`EvidenceDetail.sourceKindLabel` (`assertion_export.rs`,
`web/src/types.ts`) and rendered in the provenance panel
(`web/src/provenancePanel.ts`) directly above the raw
`source: provider:id` line, so a reader sees the explanation first.

**Verified in the browser, not just compiled**: injected one synthetic
Math-Graph-sourced assertion into a local copy of `assertions.json`
(never committed, restored via `git checkout` immediately after), called
`showAssertionDetail` directly through the dev server's module loader,
and confirmed the panel renders:

> **formal_export · uw-math-ai/math-graph LeanGraph**
> "Math-Graph formal_dependency.csv: OrderDual.instMonoid -> Monoid (edge_type=sig, role=fn)"
> **External dataset: Math-Graph (uw-math-ai, CC BY 4.0) — not independently verified by Mathesis** (source: math-graph:demo-statement-id)

This is infrastructure, not a claim that Math-Graph edges appear in
production today — the P7 pilot database remains isolated
(`scratch/math_graph_pilot/pilot_provenance.db`), so no real assertion
currently carries `sourceProvider: "math-graph"` in `scratch/provenance.db`
or `web/public/assertions.json`. The labeling is ready for whenever that
changes, and was verified against a realistic synthetic record rather
than left untested because production has nothing to show yet.

## Answering the user's stop/continue framing

The alignment question was: does the methodology mismatch make Math-Graph's
LeanGraph unsuitable for Mathesis's graph semantics? The honest answer,
based on what was actually measured:

- **Reachability differences (29% of the mismatch) are not a
  methodology problem** — they're a scope choice (P6.2 entered through
  one file; Math-Graph indexes by path). A broader pilot that imports
  more entry points, or accepts file-path-scoped selection instead of
  entry-point-transitive selection, would close this gap without
  changing what "declaration" means.
- **The declaration-identity-model mismatch (70% of the mismatch) is a
  real methodology concern, not a data-quality bug.** Mathesis's entire
  architecture — `EntityKind::Judgment`, `judgment_id_for_entity`'s
  numeric-id contract, the P6.1 checker-derived-dependency policy — is
  built around "a declaration is one literal named entry in the Lean
  elaborator's environment." If a large share of Math-Graph's LeanGraph
  nodes for typeclass-heavy code represent something else (resolved
  hierarchy positions, not literal declarations), merging them 1:1 into
  Mathesis's own judgment/entity model at scale would misrepresent what
  a "declaration" is for that fraction of the graph — not merely produce
  a lower match rate. This was inferred from data, not confirmed against
  Math-Graph's own methodology section, and is exactly the kind of open
  question this alignment pass was meant to surface before scaling up,
  not resolve unilaterally.

This is reported as evidence for the user's own stop/continue decision,
not as a recommendation either way — P7.1 was scoped as an alignment
study, not a green light.

## Verified

- `cargo test -p mathesis-provenance --lib`: 55 passed, 0 failed (clean
  exit code, not a truncated/piped one this time).
- `npx tsc --noEmit` in `web/`: clean.
- Source-attribution UI verified live in the browser against a
  synthetic record; production `web/public/assertions.json` restored
  via `git checkout` immediately after, confirmed clean (`git status`
  empty for `web/public/`).
- Ground-truth Lean declaration dumps re-ran the exact same P6.2 entry
  imports unmodified; both scripts completed (`lake env lean --run`),
  non-empty JSON output, no compile errors beyond the expected benign
  `unknown declaration 'main'` warning `#eval`-only scripts always emit.

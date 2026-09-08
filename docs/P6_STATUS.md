# Phase 6 status — a legitimate trusted-edge pipeline

> Written 2026-09-08, executing the user's "Priority 2: establish a
> legitimate trusted-edge pipeline" instruction, in the order given:
> (1) formal Lean dependency evidence, (2) a release gate requiring
> qualifying evidence for anything trusted, (3) a small real-data pilot
> measuring agreement/disagreement against the existing text extraction,
> (4) UI labels distinguishing the two sources. Item "then improve the
> review workflow" (reviewer identity/rationale/scope for *reviewed*
> semantic relations) is not attempted here — this pass is scoped to
> `depends_on`, matching "keep mathematical implication distinct from
> dependency and citation."

## What changed

### 1. A real Lean elaborator, not a text parser

`crates/mathesis-lean-extract/ExtractManifest.lean` is a genuine Lean 4
program, run via `lake env lean --run` against a project actually
compiled by `lake build` (Lean 4.29.0-rc6, mathlib
`5c8398df528176d9c87ccd9226ba8f7c8852d59c` — both pinned by the target
project's own `lake-manifest.json`, fetched via `lake exe cache get` so
no source rebuild of mathlib was needed). For every declaration whose
originating module is under the `DeGiorgi` namespace
(`Environment.getModuleIdxFor?`), it walks the *type-checked* term —
`ConstantInfo.type` and, when present, `.value?` — through
`Lean.Expr.getUsedConstants`, and keeps only the referenced constants
that are themselves DeGiorgi-defined (Mathlib lemmas used are not
"dependencies" in this project's sense — matching the exact granularity
`judgment_dependencies` already uses). There is no heuristic here: a
name appears in `dependsOn` if and only if the elaborated term the Lean
kernel already accepted actually contains it.

`crates/mathesis-provenance/src/lean_manifest_adapter.rs::import_lean_manifest`
imports this JSON as `depends_on` assertions with **`epistemic_state:
observed`** — the first real use of that state, reserved for exactly
this since `docs/DATA_DICTIONARY.md`'s original design decision
("`observed` is reserved for a real Lean-exported manifest, not yet
built") and named verbatim as the canonical example in
ARCHITECTURE_NEXT.md §4.2 ("a Lean-exported dependency"). Evidence is
recorded as `evidence_kind: formal_export`, with the source record
carrying the manifest's own content hash, the Lean toolchain, and the
mathlib revision. Matching a manifest declaration to an existing
judgment is scoped to that one paper's judgments only (`arXiv
2604.05984`, `mathesis-graph::judgments_of_paper`) and requires the bare
name to be unambiguous within that scope — an ambiguous or unmatched
name is skipped and counted, never guessed.

**Deliberately additive, not a replacement.** The existing text-extracted
`depends_on` assertions (`legacy_ref: "judgment_dependency:..."`,
`epistemic_state: extracted`, unchanged) are untouched. The new
assertions use a distinct `legacy_ref` namespace
(`"lean-manifest:..."`), so both can and do coexist for the same
`(subject, object)` pair — this is what makes the comparison in step 3
possible at all.

### 2. Release gate: `default_traversal` requires qualifying evidence

`crates/mathesis-provenance/src/verify.rs::verify_trusted_assertions_have_qualifying_evidence`
(new, called unconditionally from `verify_release`, independent of
whether a catalog is present): for every assertion whose
`relation_policy::traversal_policy` computes to `DefaultTraversal`, it
must have either an Evidence row with `evidence_kind: formal_export`, or
a `ReviewDecision{decision: accept}` row that also has a **non-null
`reviewer_id`** — an accepted review with no recorded reviewer does not
count as "authenticated." An assertion meeting neither bar fails the
release with `trusted_assertion_missing_qualifying_evidence`.

This is defense-in-depth, not the primary safety mechanism —
`traversal_policy` itself already refuses `Extracted`/`Proposed` states
by construction. What this catches is a *different* failure mode:
`epistemic_state` getting bumped to `observed`/`verified`/`reviewed` by
mistake (a bad migration, a manual DB edit, a future adapter bug)
without the evidence to back it up. Three adversarial tests in
`crates/mathesis-provenance/tests/verify_test.rs` prove this directly:
an `observed` `depends_on` backed only by `source_span` evidence is
rejected; a `reviewed` semantic relation whose accept decision has no
`reviewer_id` is rejected; the same two cases *with* qualifying evidence
are not.

### 3. A real pilot, with a real (and revealing) comparison

Ran end to end against the actual, already-imported DeGiorgi project
(`fixtures/arxiv/DeGiorgi/`, arXiv `2604.05984`, 1,431 of the corpus's
4,052 judgments) — not a toy example. Built just the
`DeGiorgi.BallExtension.ApproximationControl` module and its transitive
DeGiorgi-internal imports (~7 files, 562 declarations) rather than the
full 92-file project, matching "a limited Lean project."

**`lean_manifest_adapter::compare_dependency_sources`** restricts
comparison to only the `(subject, object)` pairs where the *subject* is
one of the 562 declarations the manifest actually analyzed — comparing
against the full corpus-wide text-extracted set would have buried the
result under thousands of pairs Lean never had the chance to confirm or
deny. Caught and fixed this scoping bug before trusting the first
number that came out (a dedicated test,
`comparison_only_counts_pairs_the_manifest_actually_analyzed`, pins it).

**Real result** (`scratch/provenance.db`, backed up first):

| | count |
| --- | --- |
| Checker-derived `depends_on` imported | 680 |
| Declarations in scope, text-extracted deps | 643 |
| Agree (both sources found the same edge) | 611 |
| Text-only (text extraction claims it, Lean does not confirm) | 32 |
| Checker-only (Lean confirms it, text extraction missed it) | 69 |

Both disagreement classes have a real, checked explanation, not just a
number:

- **Text-only false positives are a generic-identifier problem.**
  Inspecting the examples: many distinct subjects were all recorded as
  depending on judgment `restrict` — a name so generic it also exists as
  a common Mathlib/`Set` method, unrelated to the local declaration the
  text matcher happened to latch onto. This is the same class of
  problem this project's own memory already names for other layers
  (register-phrases-are-whack-a-mole) — a name-matching heuristic over
  raw text cannot tell "the local judgment named X" from "some unrelated
  API also named X."
- **Checker-only misses are indirection the text matcher can't see.**
  E.g. `unitBallRetraction` is a real dependency of several declarations
  per the elaborated term, but the identifier's occurrence in those
  proofs is indirect enough (via `unfold`, dot notation, or a chain of
  intermediate lemmas) that simple substring/identifier matching over
  the raw source text never picks it up.

Neither number was massaged to look good; both are reported as measured.

### 4. Exposed in the UI, not just the JSON

- `DependencyEdge` (`web_export.rs`) gained an `origin` field
  (`"checker-derived"` / `"text-extracted"`), derived from the
  assertion's own Evidence exactly like `MorphismEdge.origin` already
  does for morphisms — no new derivation logic invented, the existing
  pattern reused.
- `ProofGraphExplorer`'s existing stats line
  (`4,052 judgments · N dependency edges · ...`) now also shows
  "680 checker-derived" when the count is non-zero, with a hint
  explaining what that means. **Fixed a real, adjacent staleness bug
  while doing this**: the dependency count shown was
  `judgments.json`'s own `dependencyCount` field, which does not (and
  structurally cannot) know about provenance-layer-only assertions —
  after this import it was silently under-counting (5,634 shown vs.
  6,314 actually loaded). Switched the display to count the edges
  `dependencies.json` actually delivers.
- Browser-verified live, not just via the JSON: navigated to
  `unitBallApproxEps_pos`'s lineage view. With "Trusted only" off: 19
  edges, 18 dependency edges rendered solid (this specific judgment's
  neighborhood sits almost entirely inside the pilot's scope) and one
  morphism rendered faded. With "Trusted only" on: **18 real edges
  remain and the empty-state notice does not appear** — the first time
  since the toggle was built (P5 Item 1) that it has ever shown anything
  at all. This is the literal success criterion the user set: "a small
  but real set of edges backed by formal... evidence, while extracted
  edges remain visible-only."

## What this pass deliberately does not do

- **Does not touch the review workflow** (reviewer identity, rationale,
  timestamp, dataset scope for *reviewed* semantic relations) — the
  user's own instruction placed this after the pilot, as a later step;
  not started.
- **Does not extend the pilot beyond `ApproximationControl`'s import
  chain.** The other ~85 DeGiorgi files, and the other 137 papers in the
  corpus (all LaTeX-bridged, not Lean-derived — no elaborator applies to
  them), are untouched. Scaling this up is a distinct, larger decision
  (see `docs/P4_STATUS.md`'s equivalent judgment about OpenAlex's seed
  set) than "does the pipeline work at all."
- **Does not drop or weaken `judgment_dependencies`' text extraction.**
  Both sources coexist by design — the comparison is only possible
  because neither was replaced.
- **Does not change `RelationKind::Cites`/`Imports`'s `Observed`
  eligibility** or anything about citations — this pass is `depends_on`
  only, per the user's own "keep mathematical implication distinct from
  dependency and citation."

## Verified

- 88/88 Rust tests pass workspace-wide, including 4 new
  `lean_manifest_adapter` tests (idempotent reruns, unmatched
  declarations/dependencies counted not fabricated, ambiguous same-paper
  names skipped, scoped comparison correctness) and 3 new adversarial
  `verify_test.rs` tests for the trusted-evidence gate.
- Real run, backed up first (`scratch/provenance.db.bak-pre-lean-manifest`):
  680/680 checker-derived dependencies imported and idempotent on rerun
  (`+0 (skip 680)`); `verify-release` — including the new
  `trusted_assertion_missing_qualifying_evidence` check — passes cleanly
  against the regenerated `web-export`/`reconcile` output (6,314
  dependencies, 2,284 morphisms, 1,051 relations).
- `npm run build`/`npx tsc --noEmit`/`npm run eval` all clean, no
  regression (MRR@10 0.9667, Top-1 95%, dead 0%, all `relations.json`
  contract checks pass, all `dependencies.json`/`morphisms.json`
  `assertionId`s unique even with 680 new rows).
- Browser-verified end to end on real data, not assumed: the stats line,
  and the "Trusted only" toggle actually rendering the 680 real edges,
  as described above.
- A real, unplanned bug found and fixed mid-implementation: `Id.run do`
  was needed for `mut`/`for` syntax outside a top-level `do` block in
  the Lean script — a genuine Lean4 syntax lesson, not a design issue.

## Where to look

- `crates/mathesis-lean-extract/ExtractManifest.lean` — the extraction
  program itself.
- `crates/mathesis-provenance/src/lean_manifest_adapter.rs` —
  `import_lean_manifest`, `compare_dependency_sources`, `ComparisonReport`.
- `crates/mathesis-provenance/src/verify.rs::verify_trusted_assertions_have_qualifying_evidence`.
- `crates/mathesis-provenance/src/web_export.rs::build_dependency_edges`
  — the `origin` field.
- `web/src/proofGraph.ts` — `checkerDerivedDependencyCount`,
  `dependencyEdgeCount`, the stats-line rendering.
- `mathesis-provenance import-lean-manifest --db <path> --graph-db <path>
  --release <tag> --arxiv-id <id> --manifest <manifest.json>` — the CLI
  command; prints the comparison report immediately after importing.

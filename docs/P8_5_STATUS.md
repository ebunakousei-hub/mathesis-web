# Phase 8.5 status — source validation and publishability of the Math-Graph pilot

> Written 2026-09-10. Two rounds: **Phase A** (a spot-check of the
> hierarchy-classification rule against real Lean source, done first,
> in response to "Start the hierarchy spot-check first") directly
> motivated **Phase B** (a full validation pass — renamed classification
> semantics, per-project source-path auditing, a corrected re-import,
> adversarial fixtures, and real benchmarks — done in response to a
> follow-up directive: "validate the pilot's source identity and
> publishability before exposing it publicly").
>
> **Verdict up front — Option B, keep it offline for now.** Real
> progress was made (project attribution is now verified and enforced
> structurally, not just by a Python script's good behavior;
> terminology no longer overclaims; performance is excellent), but two
> of the directive's own gating conditions for publishing are honestly
> unmet: classification semantics remain ambiguous (Phase A's own
> finding, not resolved by renaming it), and exact source revisions
> cannot be reconstructed for 4 of 5 projects (Phase B, item 3). This
> is not a failure — see "Publication decision" at the end.

## Phase A — hierarchy-classification spot-check against real Lean source

### The question being tested

P7.3's rule (`kind ∈ {inst, instance}` AND zero outgoing `proof`-type
dependency edges → the classification that was then called
`typeclass_hierarchy`) was derived and validated against Mathlib
specifically. P8.1 and P8.4 explicitly flagged applying it to
FLT/carleson/PrimeNumberTheoremAnd/pfr as an "unvalidated
extrapolation" rather than a confirmed fact. This check tests that
extrapolation against something the pipeline itself never had access
to: the real, current Lean source of the classified declarations.

### Method

Sampled 5 declarations per non-Mathlib project (20 total) from
`safe_statements_p8_1.json`/`safe_statements_p8_4.json` (the exact
declarations P8.1/P8.4 imported), random seed 42, no cherry-picking.
For each, fetched the real file at `filePath` from the project's
actual public GitHub repo (`gh api repos/<owner>/<repo>/contents/<path>`
— the projects' own open-source Lean code, not the licensed Math-Graph
dataset, so no licensing concern) and read the declaration's real body.
Repos used (found via `gh search repos`, not guessed):
`ImperialCollegeLondon/FLT`, `fpvandoorn/carleson`,
`AlexKontorovich/PrimeNumberTheoremAnd`, `teorth/pfr`.

### Finding 1: the rule is a real signal, not a guarantee

Of 20 sampled names, 14 were found in the live repo. Most matches were
exactly what the rule's name implied — short, term-mode delegations,
no tactic proof at all:

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

But **two matched declarations have real, substantive tactic proofs**
despite carrying the classification — direct counterexamples to
reading it as "content-free":

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

-- pfr: PFR/Mathlib/Probability/Kernel/Composition/Comp.lean (likely match)
instance ... : IsMarkovKernel (deleteRight κ) := by
  rw [deleteRight_eq]; apply IsMarkovKernel.map _ (by fun_prop)
```

**Reading**: "zero outgoing proof-type edges" in Math-Graph's own
extraction apparently does not mean "no tactic proof was written" — a
`by simp`/`by ext i`/`by fun_prop` proof that doesn't cite another
*named declaration* as an explicit term-level dependency can still
elaborate with zero recorded `proof`-type edges. The rule measures
something narrower than its old name implied. This directly motivated
Phase B item 1 (the rename).

### Finding 2: a chunk of "PrimeNumberTheoremAnd" isn't about the prime number theorem

4 of the 5 sampled PrimeNumberTheoremAnd declarations had `filePath`s
under `LeanCert/` or `Architect/` — directories that **do not exist
anywhere in the current repo** (confirmed via the full repo file tree,
340 files, not a single missing file). This directly motivated Phase B
item 4 (full project-path validation, which found the problem is far
larger than this 5-declaration sample suggested — see below).

### Caveats on the spot-check itself

6 of 20 sampled names weren't found in the live repos at all (fast-moving
research repos; Math-Graph's snapshot predates `main`'s current state).
20 of 704 is ~2.8% — enough to establish a real, non-zero
false-classification rate, not enough to state a precise rate.

## Phase B, item 1+2 — renamed classification semantics, four concepts separated

`ExternalClassification::ExternalTypeclassHierarchy` →
`ExternalStructuralCandidate` (serialized string:
`external_typeclass_hierarchy` → `external_structural_candidate`),
renamed everywhere: the Rust enum, `DiscoverySource::MathGraphHierarchy`
→ `MathGraphStructuralCandidate`, `pilot_artifact.rs`'s
`declarations_typeclass_hierarchy` → `declarations_external_structural_candidate`,
the TypeScript `DiscoverySource`/`DiscoveryCounts` types, and every CSS
class/label string that named it. New mandatory user-facing caveat
(`web/src/mathGraphDiscovery.ts::STRUCTURAL_CANDIDATE_CAVEAT`), shown
wherever the badge appears in the UI — not hover-only:

> External structural candidate — Math-Graph contains no recorded
> proof-edge for this record. This does not establish that the
> declaration has no proof or that it is merely a typeclass hierarchy
> node.

`docs/DATA_DICTIONARY.md` gained a new section laying out the four
concepts that must not be conflated (proof-edge absent / proof absent /
hierarchy position / unresolved external semantics) and a table stating
plainly that this classifier only ever proves the first one. Full
detail and reasoning there, not duplicated here.

**A bug this rename caused, found and fixed via end-to-end browser
verification**: the two already-committed `math-graph-discovery-project{2,3}.json`
files (P7.4, never regenerated — their original scoping revision can't
be reconstructed, see `docs/P8_2_STATUS.md`) still carry the *old*
field name (`mathGraphHierarchy`) in their JSON. Reading the new field
name directly against those objects renders the literal string
"undefined Math-Graph structural candidate" — confirmed live in the
browser before the fix. `mathGraphDiscovery.ts::structuralCandidateCount()`
now falls back to the legacy field name, then to 0, matching the same
tolerance-for-older-export-shapes pattern already used for
`byProject`/`mscClassificationNote` since P8.2.

## Phase B, item 3 — source revision validation

Checked `paper_lean_repo.csv`'s own recorded fields for all 4
non-Mathlib projects: `gitCommit`, `mathlibRev`, and `repoUrl` are
**empty strings for every one of them.** Only `updatedAt`
(`2026-05-23` for all 4, apparently when Math-Graph's own scrape ran,
not a source-repo commit date) is available. Per the directive's own
fallback, this is the honest reading:

> Current repository source inspected; exact dataset-generation
> revision unavailable.

**Went further than that minimum**: found a best-effort candidate
commit near each project's `updatedAt` via `gh api
repos/.../commits?until=...&per_page=1` (FLT `dbd675b`, carleson
`a3e427d`, PrimeNumberTheoremAnd `e74ad33`, pfr `901bc69` — dates
2026-05-05 to 2026-05-23, not a single shared date, so this is a
*per-project estimate*, not one global snapshot date). Re-checked the
LeanCert/Architect question against PrimeNumberTheoremAnd's own
estimated-commit tree specifically (not just current `main`): **the
directories still don't exist there either** — stronger evidence this
isn't recent repo reorganization, the attribution was already wrong
around the time Math-Graph's snapshot was taken. Re-fetched the 2
still-unresolved FLT declaration names against the estimated commit
too — still not found (files differ from `main`, confirming the
estimate does move the source, but 2 of 20 names remain genuinely
unresolvable even with it). Mathlib_v429 is the one partial exception:
it carries a real `leanToolchain: v4.2.9` value, a weaker but genuine
anchor the other 4 projects don't have at all.

**Conclusion**: source revision is not reconstructable to a precise
degree for the 4 non-Mathlib projects. A best-effort estimate is
useful for corroborating findings (it strengthened Finding 2) but
doesn't rise to "exact provenance."

## Phase B, item 4 — full per-project path validation (not just the 20-sample spot-check)

Built `scratch/math_graph_pilot/validate_project_paths_p8_5.py`:
declaration counts by top-level `filePath` directory, over the
**entire scanned scope** of all 5 projects (11,464 declarations), not
just the 704 already-classified or the 20 sampled ones.

| Project | Total scanned | Under its own directory | Under something else |
| --- | ---: | ---: | ---: |
| Mathlib_v429 | 63 | 63 (100%) | 0 |
| FLT | 2,368 | 2,359 (99.6%) | 9 under a bare `Mathlib/` — not a real FLT path (confirmed: FLT's own repo tree has no top-level `Mathlib/` at all) |
| carleson | 2,852 | 2,852 (100%) | 0 |
| **PrimeNumberTheoremAnd** | **5,108** | **2,551 (50.0%)** | **2,557 (50.0%)**: 1,946 `LeanCert/`, 498 `PrimeCert/`, 113 `Architect/` |
| pfr | 1,073 | 1,073 (100%) | 0 |

PrimeNumberTheoremAnd's problem is **far larger than the 20-declaration
spot-check suggested** — the 5-sample check found "4 of 5 affected";
the full audit shows the true rate is exactly 50% of everything
Math-Graph attributes to this project, and of the 109 declarations
*already imported* as `typeclass_hierarchy` in the pre-P8.5 pilot DB,
**92 (84%)** were from these unrelated directories — only 17 were
genuine PrimeNumberTheoremAnd content. FLT's problem is real but tiny
by comparison (9 of 2,368 = 0.38%), and — checked directly — none of
those 9 had made it into FLT's imported safe set anyway (they're all
`kind: thm`, already excluded by the existing proof/kind rule).

Per the directive's own vocabulary, these are now classified
`project_attribution_unresolved` (not `out_of_scope_candidate` — that
would imply knowing where they *do* belong, which isn't established) —
**retained in the scope report, never silently deleted**
(`scratch/math_graph_pilot/classify_report_p8_5.json`,
`scratch/math_graph_pilot/scope_report_p8_5.json`).

## Phase B, item 5 — re-run the safe-subset importer

**Made the guarantee structural, not just a Python habit.**
`math_graph_adapter::import_pilot` gained a new parameter,
`expected_top_level_dir: Option<&str>` — when `Some(dir)` is passed, any
declaration whose `filePath` doesn't start with `dir` is refused at
import time (counted as `declarations_project_attribution_unresolved`,
never inserted), regardless of whether the Python pre-filtering script
ran correctly or was skipped entirely. `None` preserves old behavior
for existing call sites/tests. The CLI (`import-math-graph
--expected-top-level-dir <dir>`) always passes `Some`; omitting it
prints an explicit warning that the guard did not run.

New `scratch/math_graph_pilot/classify_pilot_p8_5.py`: re-classifies
all 5 projects with the attribution check applied *before* the
kind/proof-edge rule. Mathlib_v429 is special-cased — its real
`literal` bucket (cross-referenced against Mathesis's own independently
extracted corpus back in P6.2/P7, recorded in `pilot_statements_p7_4.json`)
is reused unchanged rather than recomputed from kind/proof-edge alone,
which has no way to distinguish `literal` from `external_structural_candidate`
and would have silently lost 20 real literal-matched declarations.

**Applied for real to the actual pilot DB** (not just a dry run):

```
declarations scanned (all 5 projects, full scope): 11,464
  literal:                              20
  external_structural_candidate:       654
  excluded (kind/proof-edge rule):   8,224
  project_attribution_unresolved:    2,566
edges: scanned 703,244 -> safe 889, excluded_proof 474,087, excluded_outside_safe_set 229,316
```

Before/after on the real DB (`scratch/p8_1/pilot_provenance.db`):

| | Before (pre-P8.5) | After |
| --- | ---: | ---: |
| Entities (declarations) | 766 | **674** |
| Assertions (edges) | 895 | **889** |
| — of which PrimeNumberTheoremAnd | 109 | **17** |
| — FLT / carleson / Mathlib_v429 / pfr | 498 / 54 / 62 / 43 | **unchanged** |

A real bug was caught applying this: after removing and re-importing
just PrimeNumberTheoremAnd, `export-discovery` reported 885 of 889
edges as `mathesis-checker` (should be impossible — this isolated DB
has never held any Mathesis-own data). Cause: the *already-imported*
rows for the other 4 projects still carried the pre-rename string
`external_typeclass_hierarchy` in their `evidence.external_classification`
column — renaming the Rust enum doesn't retroactively rewrite already-stored
database rows. Since the pilot DB is fully regenerable (disposable
scratch data, not production), fixed by wiping and rebuilding it fresh
from `safe_statements_p8_5.json`/`safe_edges_p8_5.json` in one pass,
rather than patching around stale strings. Re-verified after rebuild:
`mathesis-checker 0, mathesis-text 0, math-graph-literal 2,
math-graph-structural-candidate 887` — correct.

**Determinism, proven on the real DB, not just a synthetic test**: ran
a second remove→reimport round-trip on the live pilot DB after the
rebuild — entity/assertion counts (674/889) were bit-for-bit identical
before and after. Per-project breakdown after rebuild, queried directly
from `source_records.reproducibility_json`: `FLT 498, Mathlib_v429 62,
carleson 54, pfr 43, PrimeNumberTheoremAnd 17` — confirms the other 4
projects were completely untouched by the PrimeNumberTheoremAnd
correction. Production `scratch/provenance.db` was never opened this
pass (confirmed: unchanged mtime).

Regenerated `scratch/p8_1/pilot_artifact_manifest_p8_5.json` for real
against the rebuilt DB: `declarationEntityCountInDb: 674`,
`edgeAssertionCountInDb: 889`, `mscClassificationCounts.unavailable: 674`
— all internally consistent. The manifest's hardcoded `scopeNotes`
(`pilot_artifact.rs::build_manifest`) were also updated to name pfr
(previously said "3 non-Mathlib projects," missed pfr since P8.4) and
to state the new classification caveat and the project-attribution
exclusion, so the notes travel with the artifact itself, not just this
document.

## Phase B, item 6 — adversarial fixtures

7 scenarios were requested; 5 are genuinely testable in this codebase
and now have real Rust tests
(`crates/mathesis-provenance/src/math_graph_adapter.rs`, `mod tests`,
5 new functions). 2 are **not** encodable as Rust fixtures and that
limitation is stated directly in the test file's own comment rather
than faked:

- **Not testable in Rust**: "source path absent at the recorded
  revision" and "a source revision mismatch" — this codebase holds no
  revision-tracking machinery at all (`PilotStatement.mathlibRev`/
  `gitCommit` exist as fields but Math-Graph populates them as empty
  strings for every non-Mathlib project). These were investigated for
  real instead, by hand, against live GitHub data (Phase B item 3
  above) — a fixture asserting against fabricated revision data would
  test nothing real.
- **Tested for real**:
  1. `a_declaration_with_no_recorded_proof_edge_is_only_ever_labeled_a_candidate_never_confirmed_content_free`
     — asserts the classification's entire string surface never
     contains "verified"/"confirmed"/"proven".
  2. `a_trivial_delegation_and_a_substantive_proof_are_structurally_indistinguishable_to_this_classifier`
     — imports two declarations shaped identically at the schema level
     (the only level this system can see) and confirms both import
     identically — proving, not just asserting, that the classifier
     structurally cannot tell them apart.
  3. `a_declaration_with_a_named_proof_dependency_is_excluded_not_labeled_a_candidate`
     — confirms the existing proof-edge exclusion still works under the
     renamed vocabulary.
  4. `a_project_with_unrelated_tooling_directories_has_those_declarations_excluded_not_silently_imported`
     — a scaled-down reproduction of the real PrimeNumberTheoremAnd
     finding; confirms `expected_top_level_dir` actually blocks import.
  5. `an_unknown_project_path_does_not_crash_edges_referencing_it_count_as_outside_scope`
     — confirms a filtered declaration's own edges degrade to "outside
     pilot scope" instead of triggering `import_pilot`'s pre-existing
     hard `bail!` (meant for genuine `scope_pilot.py` bugs) —
     necessary so the attribution filter doesn't crash imports it's
     supposed to make safer.

`cargo test -p mathesis-provenance math_graph_adapter`: 14 passed
(9 pre-existing + 5 new), 0 failed.

## Phase B, item 7 — benchmarks (on the P8.5-validated, corrected subset)

All measured directly, not estimated.

**Storage / file sizes**:

| | Size |
| --- | ---: |
| Raw Math-Graph snapshot (2 CSVs, shared across all projects) | 1,143,503,342 bytes (≈1.07 GiB) |
| Validated pilot DB (`pilot_provenance.db`, 674 decl / 889 edges) | 1,667,072 bytes (≈1.6 MiB) |
| Derived discovery index (browser download) | 581,971 bytes (≈568 KiB) |
| Pilot manifest | 6,490 bytes |
| *For scale* — canonical production `scratch/provenance.db` | 92,979,200 bytes (≈88.7 MiB) |
| *For scale* — deployed `web/dist` bundle | ≈53 MiB |

**CLI timing** (release binary, real commands, not estimates):

| Operation | Time |
| --- | ---: |
| `remove-math-graph-project` (PrimeNumberTheoremAnd, 17 decl) | 55ms |
| `import-math-graph` (re-import same 17 decl / 2 edges) | 62ms |
| `import-math-graph` (fresh full import, 674 decl / 889 edges) | 49ms |
| `export-discovery` (full 5-project pilot, 889 edges) | 369ms |
| `export-pilot-manifest` | 33ms |
| `classify_pilot_p8_5.py` (Python reclassification, 11,464 decl scanned) | 1.09s |

**Browser (Vite dev server, local — real numbers, but not a production/CDN network proxy)**:

| Metric | Value |
| --- | ---: |
| Page load / DOMContentLoaded | 140.5ms / 136.1ms |
| Pilot discovery JSON fetch (582KB) | 35.6ms |
| "Show external" toggle render (166 edge items) | 17.4ms |
| Pagination ("Show more", 50→100 of 775) | 12.7ms |
| One-hop graph render (P8.3 local dependency view) | 13.3ms |
| JS heap (used), observed range across the session | 42–80 MiB |

**Combined view** (canonical Mathesis release + the optional pilot
discovery panel loaded together, as a user actually experiences it):
confirmed live in the browser — no console errors, no layout breakage,
mobile viewport (375px) renders and functions correctly with the pilot
panel present.

**Honest limitations on this section**:
- JS heap via `performance.memory` is noisy (GC-dependent — two
  readings taken seconds apart under identical state differed by
  ~14MB) and cannot isolate the pilot panel's specific marginal cost
  from the rest of the page (9MB taxonomy search index, WASM module,
  etc.) — reported as an observed range, not a precise attribution.
- "Memory usage on desktop and mobile" — mobile viewport emulation
  (375×812) was tested for functional correctness, but this
  environment cannot run a real mobile device's JS engine, so no
  genuine mobile memory number exists. The viewport-emulated reading
  was identical to desktop (same engine), which is not evidence about
  real mobile memory behavior.
- All CLI/DB timings are on this machine's local SSD with a warm OS
  file cache — not a controlled benchmark environment, but the
  operations are fast enough (all under 400ms, most under 100ms) that
  this doesn't change the practical conclusion (storage and latency
  are not blockers).

## Publication decision

Weighed against the directive's own two option definitions:

**Option A's conditions, checked one by one**:
- Exact or adequately documented source provenance — **honestly
  unmet**: not exact for 4/5 projects (item 3), though now
  *adequately documented as absent* rather than glossed over.
- Suspicious project paths excluded/labeled — **met** (item 4/5).
- Conservative classification vocabulary in place — **met** (item 1/2).
- Licensing and attribution recorded — **met** (since P7, CC-BY-4.0).
- Performance acceptable — **clearly met** (item 7: sub-second CLI,
  <600KB payload, <20ms interaction latency).
- UI states external/not independently verified — **met**, reinforced
  this pass with the new caveat text.

**Option B's conditions**:
- Source revisions cannot be reconstructed — **true**, for 4 of 5
  projects, confirmed by direct inspection of the dataset's own
  recorded fields plus a real best-effort reconstruction attempt that
  still left 2/20 sampled declarations unresolved.
- Classification semantics remain too ambiguous — **true**, and this
  is the harder problem: renaming the label (item 1/2) makes the
  *claim* honest, but does not make the *underlying data* any more
  able to distinguish "trivial" from "substantive" — that would
  require reading proof-body text, which this project has deliberately
  never ingested (licensing/scope reasons, P7 onward). The four-way
  split in `docs/DATA_DICTIONARY.md` puts "unresolved external
  semantics" forward as the honest reading precisely because no
  renaming closes this gap.
- Validated subset too small / payload too large — **not** the
  blocker here; 674 declarations and 568KB are both small and
  performance is fine.

**Decision: Option B — keep the pilot offline for now.** Two of the
gating conditions are genuinely, not just cautiously, unmet, and
neither is fixable by more work of the same kind already done in
P8.1–P8.5 (more classification renaming or more path auditing won't
produce a revision Math-Graph never recorded, or let this system read
proof bodies it deliberately doesn't ingest). This is exactly the
directive's own framing: *"That is not a failure. The pilot still
served its purpose by revealing that dataset-level labels and project
names are insufficient for trustworthy integration."* Concretely, this
pass **did** deliver a materially more trustworthy artifact even
though it stays offline: attribution is now structurally enforced (not
just a script's good behavior), the classification claim is accurate
rather than overclaiming, and the whole pipeline (import → validate →
remove → reimport → export) is proven deterministic on real data.

**What would change this**: a real, licensed, scope-appropriate way to
verify proof content (not full body/proof text ingestion — that's a
separate, larger scope decision) would resolve the classification
ambiguity; better provenance from Math-Graph itself (a future dataset
revision with real `gitCommit` values) would resolve the revision
problem. Neither is this pass's call to pursue further without new
instruction.

## What was deliberately not done

Per the directive's own list: did not reclassify all 704 records as
confirmed hierarchy positions (674 of the original 766 are now
`external_structural_candidate`/`literal`, honestly labeled, not
"confirmed"); did not remove PrimeNumberTheoremAnd's tooling records
without an exclusion report (they're in `classify_report_p8_5.json`,
excluded with a named reason, not deleted from the record); did not
publish the pilot to `web/public/` (temporary copies used for browser
verification were deleted after, matching every prior P8.x pass); did
not merge external records into the canonical Mathesis graph; did not
connect to the live TheoremGraph API; did not download the full 13.6GB
dataset; did not treat current GitHub source as a substitute for the
dataset's own generation revision (used it as a labeled, caveated
*estimate* — see item 3 — never presented as ground truth).

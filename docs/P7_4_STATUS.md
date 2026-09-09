# Phase 7.4 status — a bounded, production-safe Math-Graph discovery layer

> Written 2026-09-09, executing the user's P7.4 instruction: build a
> bounded, production-facing (not merely isolated-study) presentation of
> the "proven-safe" Math-Graph subset P7.1–P7.3 established, with a
> separate relation model, only the validated declarations, an explicit
> UI toggle, evaluation metrics, and a systematic fix for the artifact-
> freshness class of bug P7.2 found. Still no full dataset download, no
> live API, no merge into Mathesis's canonical trusted graph.

## 1. A separate external relation model

Added `Evidence.external_classification: Option<String>` (additive
migration, `ensure_p7_4_columns`, same idempotent-`ALTER TABLE` pattern
as every prior column addition in this crate) — `"external_literal_
dependency"` or `"external_typeclass_hierarchy"` for Math-Graph evidence,
`None` for everything Mathesis's own adapters produce. This is a
separate axis from `dependency_origin` (the raw Math-Graph `edge_type`:
`sig`/`def`/...): `external_classification` answers "what *is* this
declaration," `dependency_origin` answers "where in this one edge did
the reference appear."

Every record retains (verified against the actual exported JSON, not
just the schema): Math-Graph source ID (`SourceRecord.provider_id` =
Math-Graph `statement_id`), dataset revision + content hash
(`provider_revision`, verified byte-identical against HuggingFace's own
reported hash back in P7), source project (`reproducibility_json.
repoSlug`), source/target declaration (`subject_ref`/`object_ref` via
`judgment:mathgraph:<uuid>`), `edge_type` (`dependency_origin`),
original Math-Graph metadata (`reproducibility_json`: toolchain,
`mathlib_rev`/`git_commit` — `null` where Math-Graph's own data has no
value, never guessed), attribution + license (`SourceRecord.attribution`/
`licence`, "CC-BY-4.0"), evidence locator (`Evidence.locator`),
`epistemic_state: extracted` (not `observed` — P7's own deliberate
policy choice, unchanged), `traversal_policy: visible_only` (derived,
not stored — `relation_policy::traversal_policy(DependsOn, Extracted)`,
untouched code).

## 2. Only the proven-safe subset — re-derived precisely, correcting a P7.3 estimate

P7.3 informally said "41 hierarchy + 2 excluded." Re-deriving the exact,
reproducible rule for an actual import (`scratch/math_graph_pilot/
scope_pilot_p7_4.py`) found the real split is **42 hierarchy + 1
excluded** — recorded here rather than silently using the rounder,
slightly wrong earlier number. The rule:

- **literal** (20): P7.2's exact matches — reachability-fixed (7),
  prime-normalized (11), canonicalized (1, `RelCat.inhabited`), genuine
  exact match (1, `LibraryNote...`).
- **hierarchy** (42): not already literal, **and** `kind ∈ {inst,
  instance}` (i.e. Math-Graph itself is claiming this is a typeclass
  instance), **and** zero `proof`-type outgoing edges (P7.3's schema
  signal, recomputed fresh from the raw pilot data here rather than
  reused as a cached number).
- **excluded** (1): `CategoryTheory.Category.mk'` — `kind: def`, not
  claiming to be a typeclass instance at all, so the hierarchy rule
  doesn't apply to it either; no literal match was found. This is a
  cleaner, more principled reason to exclude it than P7.3's vaguer
  "without a clean hierarchy explanation."

Edges: only kept where **both** endpoints are in the 62-declaration safe
set, **and** `edge_type != "proof"` (excluded per instruction — its
cross-system meaning isn't separately defined yet). Of 290 originally
pilot-scoped edges: 27 excluded as `proof`-type, 215 excluded because an
endpoint falls outside the 62-declaration safe set, **48 imported**.

Confirmed excluded from this pass, unchanged from P7: theorem/proof/
context text (never imported at any point since P7), the remaining 24
Lean projects and the informal/arXiv side (no new download — reused
`scratch/math_graph_pilot/` files from P7), the live API (never touched).
"Records whose source revision cannot be reproduced" — the dataset
revision *can* be reproduced (HF commit `ced4ca9d...`, byte-verified in
P7); the per-project `mathlib_rev`/`git_commit` gap is a known,
documented limitation carried forward honestly (`null`), not grounds to
exclude the whole pilot.

**Real import**, two isolated per-project databases
(`scratch/p7_4/project{2,3}_provenance.db`, copied from P7.2's own
already-verified study databases, not production
`scratch/provenance.db`):

```
Math-Graph pilot: declarations +53 (skip 0), dependencies +46 (skip 0), 0 outside pilot scope   [project 2]
Math-Graph pilot: declarations +9 (skip 0), dependencies +2 (skip 0), 0 outside pilot scope      [project 3]
```

`stats` confirms the arithmetic: project 2's `extracted` count is 519 =
473 (Mathesis's own P7.2 text-extraction, unchanged) + 46 (Math-Graph);
project 3's is 599 = 597 + 2. `mathesis-checker` counts (59, 111) are
exactly P7.2's own unchanged numbers. `build-catalog` was run against
these two databases (previously skipped in P6.2/P7.2's pilot-only
databases) specifically so the discovery export could show real
declaration labels instead of raw `judgment:N` refs — 100% entity
reference coverage confirmed for both (1156/1156, 1420/1420).

## 3. The UI

New, fully separate view: `web/src/mathGraphDiscovery.ts` +
`docs/`-linked `discovery_export.rs` (`crates/mathesis-provenance/src/
discovery_export.rs`). Deliberately **not** wired into the existing
`ProofGraphExplorer`/`LineageView` — those are built around a numeric
judgment-id contract (`judgment_id_for_entity`, P7.1's finding) that
`judgment:mathgraph:` refs structurally cannot satisfy, and retrofitting
that contract was explicitly out of scope for a bounded pilot. Instead,
`export-discovery` is a new, independent CLI command reading directly
from `subject_entity_id`/`object_entity_id` display labels — it does not
call, modify, or share code with `web_export.rs`'s `dependencies.json`
generation at all.

Verified live in the browser (not just compiled): a new section renders
below the existing "層3〜5" panel. Default state — external edges
**hidden**, 4-way count chips visible (`59 checker-derived · 473
text-extracted · 0 Math-Graph literal · 46 Math-Graph hierarchy` for
project 2, etc.), a hint stating how many are hidden. Toggling "Show
external Math-Graph discoveries" reveals all 48 external edges (checked:
`document.querySelectorAll('.mgd-edge-item').length === 48`), each
tagged `Math-Graph typeclass-hierarchy discovery (external)` or `...
literal dependency (external)`, each carrying the exact required
attribution line: *"External dataset: Math-Graph — CC-BY-4.0 — not
independently verified by Mathesis."* Toggling off returns to 0 visible
external edges. No console errors either state.

## 4. Evaluation

| Metric | Value |
| --- | --- |
| External discoveries (safe subset) | 48 (46 hierarchy + 2 literal) |
| Duplicates of Mathesis's own edges | 2 — both literal-category (`Factorisation.comp_h`/`comp_h_assoc` → `instQuiver`, independently found by Mathesis's own P7.2 checker extraction too) |
| Genuinely new hierarchy discoveries | 42 — "new" here means Mathesis's own declaration model has **no node at all** to compare against (P7.3's finding), not "a new edge on an existing node"; not directly pair-comparable to Mathesis's own data |
| Unresolved records excluded | 1 (`CategoryTheory.Category.mk'`) |
| Endpoint-resolution rate (this layer) | 100% by construction — edges are only imported when both endpoints are in the 62-declaration safe set |
| Endpoint-resolution rate (of the original P7 pilot scope) | 48/290 = 16.6% — most of the original scope is excluded by the `proof`-edge and safe-declaration-set rules, honestly, not hidden |
| Discovery export size | 163 KB + 186 KB = 349 KB total (both projects; includes all edge sources for completeness, though the UI only lists the 48 external ones individually) |
| Search result changes | None — this panel is not wired into search at all |
| Trusted/default-lineage-count changes | None — `traversal_policy: visible_only` on every external edge, confirmed; `dependencies.json`/production `scratch/provenance.db` untouched (see §6) |
| User-visible false joins | 0, structurally — `judgment:mathgraph:` is a disjoint ref namespace from Mathesis's own `judgment:<id>`, never merged/aliased to any Mathesis entity |

The duplicate/new split (2 vs. 42, not directly comparable) is the
honest answer to "does this add discovery coverage or just a
differently-normalized universe": for the literal-declaration category
specifically, it's 100% redundant with what Mathesis's own checker
already has (0 new pairs found); the entire discovery value of this
layer is in the 42 hierarchy-position entries, which are additive by
construction, not by having out-searched Mathesis's own pipeline.

## 5. Artifact freshness — fixed systematically, not just patched again

The detection mechanism (`release_gate::verify_web_export`'s structural
JSON diff, `check_output_file`) already existed and had already worked
once (it's exactly what caught P6.3's stale `relations.json` during
P7.2's own Definition-of-Done check). The actual gap was that this class
of bug — same row count, same content, but a field silently added to
the shape — had no dedicated regression test proving the detector
catches it independent of any hash coincidence. Added
`detects_a_field_added_to_the_row_schema_even_when_the_stale_files_own_
hash_is_self_consistent` (`release_gate_test.rs`): writes a real export,
strips `traversalPolicy` from a copy of `relations.json`, **recomputes
that copy's own hash correctly** (so `output_hash_mismatch` cannot be why
it's caught — isolating the exact failure mode: "I regenerated the file,
just with an old build"), and asserts `verify_web_export` still reports
`web_export_stale`. Passes. Auto-regeneration (`web-export` running
itself as part of `verify-release`) was considered and not built this
pass — it changes the release workflow's shape, a bigger decision than
this pass's scope; the test is the concrete, load-bearing deliverable
the "add a test" instruction asked for.

## 6. No production trusted edges changed — verified, not assumed

`git status` on `web/public/` before any write confirmed only the two
new files (`math-graph-discovery-project{2,3}.json`) were added — no
existing production export was modified. `verify-release` against
production `scratch/provenance.db` (same command P7.2 fixed) passes
cleanly. `npm run eval` numbers unchanged (MRR@10 0.9667, 1051 relations
/ 36 confirmed / 1015 grounded, 0 contradictory cycles). Production
`scratch/provenance.db` itself was never opened for writing this pass —
all Math-Graph import work happened in `scratch/p7_4/project{2,3}_
provenance.db`, copies of P7.2's own already-isolated study databases.

## Definition of done

- [x] External Math-Graph records isolated from canonical trusted
  assertions — separate study databases, `epistemic_state: extracted`,
  `traversal_policy: visible_only`, new `external_classification` axis.
- [x] Only the validated pilot subset imported (62/63, precisely
  re-derived; 1 excluded).
- [x] Every record has source attribution and revision metadata
  (verified against real exported JSON, §1).
- [x] External edges remain `visible_only` (verified in the export
  output, not merely asserted).
- [x] UI distinguishes and toggles them explicitly (verified live in
  the browser, both states, no console errors).
- [x] No default search or trusted-traversal behavior changes
  unexpectedly (`git status`, `verify-release`, `npm run eval` all
  confirm this directly).
- [x] Stale generated artifacts detected automatically (pre-existing
  mechanism, now regression-tested against the exact bug class that
  slipped through once).
- [x] Rust tests, release verification, web build, and evaluation pass.
- [x] This document records the exact inclusion/exclusion rules,
  including a correction to P7.3's own informal estimate.

## What comes next is a decision, not a default

Whether the external layer is worth expanding project-by-project is the
user's call, per the task's own closing framing. One data point worth
weighing: for the one project (3) where declarations mostly *do* match
Mathesis's own model, the discovery layer found zero new information
(100% duplicate); all of its value came from project 2's typeclass-
hierarchy entries, which are additive precisely *because* they don't fit
Mathesis's declaration model — a narrower value proposition than "more
projects, more coverage" would suggest.

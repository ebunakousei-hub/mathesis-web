# Phase 5 status — trust-aware traversal and staged EntityId integrity

> Written 2026-09-07, executing `docs/P5_PLAN.md`'s Item 1 (recommended
> first: zero schema risk, purely additive on the client). Prompted by an
> external review whose top recommendation matched this plan's own Item 1
> exactly. This document reports what was built, a real finding the review
> did not anticipate (today's data has zero edges at the trusted tier), and
> the design decision made in response — plus a genuine pre-existing bug
> this work exposed and fixed.
>
> **Updated 2026-09-08**: Item 2's additive migration stage (schema
> columns, backfill, drift check) landed in the working tree and is now
> covered by adversarial tests, verified against real data, and had two
> real bugs (a 15-minute performance regression, a reporting-honesty gap)
> found and fixed before being treated as done — see the dedicated section
> below.

## What changed

- `crates/mathesis-provenance/src/web_export.rs`: `DependencyEdge` and
  `MorphismEdge` now carry `traversalPolicy` — the same string
  `relation_policy::traversal_policy()` already computed for
  `assertions.json`, now attached directly to every edge in
  `dependencies.json`/`morphisms.json`. This lets the client filter the
  main graph by trust tier without fetching the much larger
  `assertions.json` (4.4+ MB of full evidence/review detail) just to
  render the lineage view — a real performance consideration, not a
  hypothetical one, given this project's established discipline of
  keeping the initial graph load cheap (`taxonomy.related.json`'s lazy
  load, `searchWorker.ts`'s move off the main thread).
- `web/src/lineage.ts`: `LineageGraph` gained `dependencyPolicy` (a
  `"from->to"` → policy map — dependency edges are still stored as plain
  id arrays for compatibility with all existing callers).
  `LineageOptions` gained `trustedOnly` (**default `false`** — see below
  for why). New exported helpers `traversableChildren`/`traversableUsedBy`
  filter by trust tier when the option is on, used consistently by
  `findSpine`, `collectSubgraph`, `buildLineage`'s edge collection,
  `spineOutline`'s "side lemmas" list, *and* `lineageView.ts`'s judgment
  detail panel ("depends on (N)" / "used by (N)" / morphism chips) — every
  place the old, unfiltered `graph.dependsOn.get(id)` was read now goes
  through the same filter, so the graph picture and the text detail panel
  can never disagree about what's currently visible.
- `web/src/lineageView.ts`: a new "Trusted only" toggle (`lin-ctl`,
  matching the existing morphism-visibility toggle's own pattern),
  visually faded (`opacity: 0.5`) edges below the trusted tier when the
  toggle is off (ARCHITECTURE_NEXT.md §7's edge-visual table, applied via
  opacity rather than dash pattern since dash is already used to encode
  *predicate kind*, not trust — the two are orthogonal and needed separate
  visual channels), and an explicit, honest empty-state message when
  "trusted only" is on and produces zero edges (see below).
- `web/src/provenancePanel.ts`: the assertion detail dialog's badge now
  shows the actual 4-value `traversalPolicy` (`default_traversal` /
  `visible_only` / `formal_only` / `excluded`) with a one-line explanation
  of what each means, instead of a bare "opt-in only" boolean — directly
  answering "explain why each edge is or is not traversable."
- `web/src/types.ts`: added the new fields, and separately fixed a real
  **pre-existing type/reality drift**: `EvidenceDetail`/`AssertionDetail`
  were missing `locatorPrecision`, `traversalPolicy`,
  `subjectLabelOrigin`/`objectLabelOrigin` — fields the Rust side
  (`assertion_export.rs`) has been serving in `assertions.json` since P3's
  stabilization pass, silently absent from the TypeScript type the whole
  time. Not a runtime bug (JS doesn't enforce the type), but exactly the
  kind of gap this project's own memory has flagged before in other
  layers.
- `web/src/leanPlayground.ts`: the in-browser pasted-Lean-snippet lineage
  view also needed a `dependencyPolicy` map to satisfy the now-required
  `LineageGraph` field — given `visible_only` (name-matched extraction,
  the same epistemic tier as the CLI's `judgment_dependencies` import, not
  a Lean elaborator export), consistent with the server-side data it
  mirrors.
- **A genuine pre-existing bug, found and fixed while building this**:
  `web/src/util.ts::escapeHtml` escaped `<`/`>`/`&` (via
  `div.textContent`/`innerHTML`) but never `"`, even though four existing
  call sites (`dynamicTaxonomy.ts`, `lineageView.ts` ×2, and this
  increment's new one in `provenancePanel.ts`) embed its output inside an
  HTML `title="..."` attribute. It never broke before because no
  translated hint string happened to contain a literal `"`. This
  increment's new traversal-policy hint text (`...the lineage view's
  "trusted only" filter...`) does, and it broke the attribute exactly as
  expected when tested live in the browser (confirmed via DOM inspection:
  the `title` attribute terminated early and the remainder of the string
  became garbage attributes). Fixed at the root — `escapeHtml` now also
  escapes `"`/`'` — rather than working around it locally, since the same
  latent bug sat in three other call sites.

## The finding the review didn't anticipate: zero edges reach the trusted tier today

Checked directly against real production data before deciding how
"default" should behave: **100% of the 5,634 dependency edges and 100%
of the 2,284 morphisms are `visible_only`; none are `default_traversal`.**

This is not a bug — it follows directly from decisions this project made
deliberately, earlier, for good reasons:

- `depends_on` is `extracted`, never `observed`, because
  `judgment_dependencies` comes from name-matching within raw Lean text,
  not a real Lean-elaborator-exported dependency manifest (`observed` is
  reserved for that, per `docs/DATA_DICTIONARY.md`'s decision — no such
  manifest exists yet).
- Every morphism (implies/specializes/generalizes/equivalent_to) is
  `proposed` — real production data has **zero** `ReviewDecision` rows
  (`review_decisions: 0` in `stats`), confirmed again this round; no
  morphism has ever been human-reviewed.

Given `relation_policy::traversal_policy()`'s existing rules (DependsOn
needs Observed/Verified; the semantic kinds need Reviewed/Verified), this
means a literal "hide everything except `default_traversal` by default"
implementation would have emptied the entire lineage view — the site's
core feature — on every single page, for every judgment, today. That is
a severe, immediately visible regression, not the safety improvement the
review intended.

**Design decision made in response**: default (`trustedOnly: false`)
changes nothing about what's shown — zero regression, matching today's
behavior exactly. The new toggle is opt-in, and turning it on shows
today's honest reality (usually zero edges) with a clear explanation
rather than a confusing blank canvas. This differs from a literal reading
of the external review's "default traversal show only trusted/default
edges," and that divergence is deliberate, not an oversight — recorded
here so it isn't silently re-decided differently later without re-reading
this reasoning.

## What this increment deliberately does not do

- **Does not hide anything by default.** See above — this was a
  considered choice, not a shortcut.
- **Does not touch `relations.json` (concept relations)**, which has its
  own separate confirmed/grounded status vocabulary already serving an
  analogous purpose in `dynamicTaxonomy.ts`. `docs/P5_PLAN.md`'s own Item
  1 scope named `proofGraph.ts`/`lineageView.ts`/`provenancePanel.ts`
  specifically, not the concept explorer.
- **Does not add a citations.json consumer** — unrelated to this item;
  `docs/P4_STATUS.md` already covers why `Cites` isn't in the web export
  yet.
- **Item 2 is now in its additive migration stage.** Existing databases
  receive nullable `subject_entity_id`/`object_entity_id` columns and
  indexes on open. Assertion insertion populates them whenever the catalog
  already contains the endpoint; `build-catalog` backfills all existing
  assertions and fails if any endpoint remains unresolved. Release
  verification compares both IDs with the current string-reference
  resolution, so drift cannot be published.
- The tagged strings remain in the schema and on the wire for display and
  compatibility. The IDs are authoritative for catalog integrity, but the
  final removal of string authority is intentionally deferred until a
  complete real-data release has passed this invariant.
- **Item 3 (the declarative `relation_schema` table) remains deferred.**

## Item 2's additive migration, verified against real production data

The additive-stage code (nullable `subject_entity_id`/`object_entity_id`
columns, `backfill_assertion_entity_ids`, `verify_release`'s drift check)
arrived in the working tree without having been run against real data or
covered by adversarial tests yet. Before treating it as done:

- **Added the adversarial tests the staged work was missing**: a missing
  subject FK and a missing object FK are both exercised (realistically —
  inserting an assertion before its endpoints are cataloged, then
  cataloging them, reproduces exactly the `import-legacy`-before-
  `build-catalog` ordering that happens in practice) and confirmed to
  surface as `entity_endpoint_drift` in `verify_release`
  (`crates/mathesis-provenance/tests/verify_test.rs`); a dangling FK
  (pointing at an entity id that doesn't exist) is confirmed **rejected by
  SQLite's own foreign-key constraint**, not just by application logic
  (`crates/mathesis-provenance/tests/entity_id_migration_test.rs` —
  proven by attempting the corrupting `UPDATE` directly and asserting it
  errors); opening a database created before this migration is confirmed
  to add the new columns without losing the existing row, and reopening
  an already-migrated database is confirmed idempotent (same file).
  `crates/mathesis-provenance/src/entity.rs` also gained direct unit tests
  for `backfill_assertion_entity_ids`'s all-or-nothing contract and
  `assertion_entity_id_coverage`. 78/78 tests pass.
- **Found and fixed a real performance bug before it finished a first
  real-data run**: `backfill_assertion_entity_ids` originally issued one
  `UPDATE` per assertion with no surrounding transaction — SQLite's
  default auto-commit mode fsyncs every statement, the exact `mathesis-
  graph::store`-documented pitfall this project already knows about
  ("1行=1トランザクションだとfsync待ちが支配的、実測9.8ms/行"). Against
  the real 104,708-assertion table this made `build-catalog` run past 15
  minutes before being stopped. Wrapped the update loop in
  `self.transaction(...)`, matching `legacy_adapter::import_graph`'s own
  existing pattern; the real run then completed in **53 seconds**.
- **Found and fixed a second issue while checking rerun behavior**: the
  first version of `backfill_assertion_entity_ids` unconditionally
  reported every resolved assertion as newly "backfilled" on every run,
  including reruns where nothing had actually changed — the exact
  reporting-honesty bug this project already caught once in P3's
  `catalog_adapter.rs` ("+N (skip M)" vs. a misleading bare "+N" every
  time). Fixed by returning a `BackfillStats{newly_backfilled,
  already_correct, unresolved}` struct that distinguishes an actual write
  from a no-op confirmation, verified by an explicit rerun assertion in
  the test suite.
- **Real-data result, backed up first**
  (`scratch/provenance.db.bak-pre-entityid-migration`): `build-catalog`
  reports `assertion entity endpoints: +104708 backfilled (already
  correct: 0)` on the first run and `+0 backfilled (already correct:
  104708)` on rerun — **100% of the 104,708 real assertions** now carry
  correct `subject_entity_id`/`object_entity_id`, matching the
  209,416/209,416 catalog-reference coverage already proven in P3.
  `verify-release` (P1 sidecar completeness + P2 web-export integrity,
  now also checking `entity_endpoint_drift`/`unresolved_entity_reference`
  since the catalog is present) passes cleanly against this backfilled
  database with no new failures.

## Item 2 step 4: readers switched to EntityId as the authoritative join key

With the backfill proven at 100% real coverage, switched the three named
readers from parsing `subject_ref`/`object_ref` strings to resolving
through `subject_entity_id`/`object_entity_id` (the FK). The tagged
strings remain in the schema and on the wire for display/debugging, as
`docs/P5_PLAN.md` always said they would — only their role as the
*authoritative* source of truth changes.

- `web_export.rs`: `build_dependency_edges`/`build_morphism_edges` derive
  `from`/`to`/`src`/`dst` via a new `judgment_id_for_entity` reverse
  lookup (judgment entities always have exactly one ref, so this is
  unambiguous) instead of stripping a `"judgment:"` prefix off the raw
  string. `build_relation_edges` now emits `subject`/`object` as the
  resolved entity's `display_label` (the Entity Resolution representative
  phrase) instead of the raw recorded phrase. All three now require both
  endpoints to have a populated FK to be included at all — an assertion
  with an unresolved endpoint no longer sneaks through on a
  coincidentally well-formed string. `build_web_export` fails loudly
  (`entity_count() == 0`) rather than silently writing empty files if
  `build-catalog` hasn't run yet — `build-catalog` is now a de facto
  prerequisite for `web-export`, and that needs to be an explicit error,
  not a quiet empty export.
- `catalog_adapter.rs::assertion_reference_coverage`: reads
  `subject_entity_id`/`object_entity_id` directly instead of re-resolving
  209,416 refs through `resolve_entity_ref` on every call.
- `assertion_export.rs::entity_label_with_origin`: now takes
  `Option<EntityId>` (the assertion's own FK) instead of a ref string to
  re-resolve. The unused sibling `entity_label_for` (dead code — never
  called anywhere) was deleted rather than updated.

**A real bug found and fixed, not just an architectural nicety.**
Diffing live output against what `web/public/` was already serving
turned up exactly 3 (of 1,051) concept relations whose recorded
`subject_ref`/`object_ref` phrase was a spelling *alias*, not the
Entity-Resolution representative: `"pull back"` vs. the representative
`"pull-back"`, `"one dimensional"` vs. `"one-dimensional"`, `"dg module"`
vs. `"dg-modules"`. `web/src/dynamicTaxonomy.ts`'s `expandRelations`
keys its lookup map by whatever phrase `relations.json` gives it, and the
concept-detail view only ever looks a relation up by the *representative*
phrase (`h.phrase`, sourced from the search index) — so these 3 relations
were filed under a map key nobody ever queries and were **invisible on
their own concept's page** in the live, already-published site. Switching
`build_relation_edges` to emit the entity's `display_label` fixes this:
diffed `dependencies.json`/`morphisms.json` byte-identical before/after
(no other change), `relations.json` differs in exactly these 3 rows, and
`assertions.json`'s `subjectLabel`/`objectLabel` (already FK-derived
since P3) are identical across all 8,969 entries — confirming the FK and
string paths always agreed except in these 3 cases, which is exactly the
class of divergence this migration exists to eliminate. Browser-verified
live: the "pull-back" concept page now shows its relation to "finite
fourier series" with evidence sentence and provenance link, which it did
not before.

**Incidental finding while regenerating for real-data verification**:
`web/public/assertions.json` had been stale since the P5 Item 1 round —
`traversalPolicy`/`locatorPrecision`/`subjectLabelOrigin`/
`objectLabelOrigin` were added to the Rust struct and TS type then, but
`reconcile` (the command that writes `assertions.json`) was never rerun
afterward, only `web-export` was. Regenerated and promoted it as part of
this round; confirmed the only differences from the stale file were the
missing fields themselves — zero label or ref content actually changed
(checked all 8,969 entries programmatically before promoting).

Test fallout from making `web-export` require a populated catalog:
`release_gate_test.rs`'s fixtures never registered catalog entities
(they predate P3), so all 7 of its tests started failing with the new
`entity_count() == 0` guard. Fixed by registering judgment/concept
entities before inserting assertions, matching the real
`import-legacy` → `build-catalog` ordering — including one test
(`detects_stale_export_after_the_store_changes`) whose whole premise (a
newly-added, uncataloged morphism should make the live store diverge
from an old export) silently stopped working once uncataloged
assertions became invisible to `web_export.rs`; fixed by cataloging the
new morphism's endpoints too, so the test still exercises a *valid* new
edge rather than accidentally testing nothing.

## Verified

- 6 new/updated Rust tests (`web_export.rs`) confirming `traversalPolicy`
  is computed correctly per edge; 67 pre-existing tests still pass.
  `verify-release` re-run against the regenerated
  `dependencies.json`/`morphisms.json` (now carrying the new field) passes
  cleanly — P1/P2 gates unaffected.
- Item 2 additive stage: 78/78 Rust tests pass workspace-wide, including
  10 new adversarial tests covering every scenario named above; real
  `build-catalog` run backfills 104,708/104,708 assertions in 53 seconds
  (down from a 15+-minute run that was stopped after finding the
  missing-transaction bug); real `verify-release` passes cleanly with
  the new drift checks active.
- Item 2 step 4 (readers switched to EntityId): 80/80 Rust tests pass
  (2 new: `judgment_id_for_entity` on a judgment and on a concept, plus
  fixes to 7 pre-existing `release_gate_test.rs` tests whose fixtures
  predated the catalog). Real `web-export`: `dependencies.json`/
  `morphisms.json` diffed byte-identical against what was already live;
  `relations.json` differs in exactly the 3 rows named above, confirmed
  by a full programmatic diff, not spot-checking. `reconcile`'s
  `assertions.json` diffed with zero label/ref changes across all 8,969
  entries once the incidental staleness fields are excluded.
  `verify-release` passes cleanly against the fully-updated real
  database and files. `npm run eval` unchanged. Browser-verified live:
  navigated to the "pull-back" concept page and confirmed its relation
  to "finite fourier series" — with evidence sentence and provenance
  link — now renders, which it did not on the previously-published site.
- `npm run build` and `npx tsc --noEmit` clean; `npm run eval` unchanged
  (MRR@10 0.9667, Top-1 95%, dead 0% — same numbers as before this
  change, confirming no regression to search).
- **Browser-verified end-to-end on real data**, not just unit tests: the
  known `unitBallApproxEps`/`deGiorgiTail` example (memory from P2)
  renders correctly with the new policy badge; a 47-level chain
  (`holder_Moser`) showed 75 edges with the toggle off, correctly dropped
  to exactly 0 with the toggle on, the graph and the "depends on (10)" →
  "depends on (0)" detail-panel count changing together; toggling back
  off restored all 75 edges. The `escapeHtml` bug above was caught this
  way — a unit test would not have exercised the actual attribute-parsing
  behavior a real browser applies.

## Where to look

- `crates/mathesis-provenance/src/web_export.rs` — `traversal_policy`
  field, `dependency_edge_carries_the_traversal_policy_computed_from_its_epistemic_state`.
- `web/src/lineage.ts` — `dependencyPolicy`, `trustedOnly`,
  `traversableChildren`/`traversableUsedBy`.
- `web/src/lineageView.ts` — the toggle, the empty-state notice, the
  faded-edge styling.
- `web/src/provenancePanel.ts` — `TRAVERSAL_POLICY_LABEL`.
- `web/src/util.ts::escapeHtml` — the quote-escaping fix.
- `docs/P5_PLAN.md` — the plan this executed; its own risk framing did
  not anticipate the zero-trusted-edges finding, recorded honestly here
  rather than glossed over.
- `crates/mathesis-provenance/src/entity.rs` — `BackfillStats`,
  `backfill_assertion_entity_ids` (now transaction-wrapped), the
  all-or-nothing contract and its idempotent-rerun tests.
- `crates/mathesis-provenance/src/store.rs::ensure_assertion_entity_columns`
  — the on-open migration for pre-existing databases.
- `crates/mathesis-provenance/src/verify.rs::verify_catalog_assertions`
  — `entity_endpoint_drift`/`unresolved_entity_reference` detection.
- `crates/mathesis-provenance/tests/entity_id_migration_test.rs` — the
  schema-level guarantees (pre-migration DB, idempotent reopen, dangling
  FK rejected by SQLite itself) that need raw `rusqlite` access and so
  don't fit `verify_test.rs`'s public-API-only convention.
- `crates/mathesis-provenance/src/entity.rs::judgment_id_for_entity` —
  the `EntityId` → judgment-numeric-id reverse lookup `web_export.rs`
  now uses instead of string parsing.
- `crates/mathesis-provenance/src/web_export.rs::build_dependency_edges`/
  `build_morphism_edges`/`build_relation_edges` — all three now
  FK-authoritative; `build_web_export`'s `entity_count() == 0` guard.
- `crates/mathesis-provenance/src/assertion_export.rs::entity_label_with_origin`
  — now takes the FK directly.
- `crates/mathesis-provenance/src/catalog_adapter.rs::assertion_reference_coverage`
  — now reads the FK columns instead of re-resolving.

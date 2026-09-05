# Phase 5 plan — trust-aware traversal and policy-as-data (not yet started)

> Written 2026-09-06. Status: **plan only, no code changes**. Scoped
> alongside `docs/P4_PLAN.md` while the user is away pending a department
> reply on external-dataset licensing. This document does not depend on
> that reply at all — everything here is groundwork named in
> ARCHITECTURE_NEXT.md §12 Phase 3 ("Curation and trust-aware
> exploration") and in `docs/P3_STATUS.md`'s own "deliberately does NOT
> do" list, using only data the system already has.

## Why this is next (after P4, or in parallel — no dependency between them)

`docs/P3_STATUS.md`'s stabilization pass computed a `TraversalPolicy` per
assertion (`crates/mathesis-provenance/src/relation_policy.rs`) and
exposed it in `assertions.json` via `traversalPolicy`. Checked directly
against the current `web/src` tree: **no client file references
`traversalPolicy` today.** The field is computed and shipped but not yet
consumed — the exact "compute it but don't wire it in" gap this project's
own memory has flagged before in other layers
([[fulltext-bridge-already-built-before-graph-wiring]],
[[stale-未実装-claims-verify-by-reading-code]]). This plan closes that
gap first, then addresses the two items `docs/P3_STATUS.md` named
explicitly as deferred.

Three independently-scoped items, ordered by how much they depend on one
another — not a mandate to do all three in one sitting, matching this
project's established one-increment-at-a-time discipline. The user should
pick where to start.

## Item 1 — wire `traversalPolicy` into the client (recommended first)

**Smallest, purely additive, zero schema/backend risk.** The data already
exists in `web/public/assertions.json`; only `web/src` changes.

- `web/src/proofGraph.ts` / `lineageView.ts` currently render every
  dependency/morphism edge unconditionally once assertionId resolves.
  Add a default filter: only assertions whose `traversalPolicy ===
  "default_traversal"` populate the graph used for lineage traversal by
  default.
- Add an explicit, visible toggle ("proposed/visible-only edges" — exact
  label TBD by the user) that reveals `visible_only`/`formal_only` edges
  on demand. This is ARCHITECTURE_NEXT §7 rule 3 verbatim: *"Traverse only
  edge states allowed by the user's trust policy... proposed edges are
  opt-in."*
- `web/src/provenancePanel.ts` already renders assertion detail; it should
  additionally show *why* an edge is excluded from default traversal
  (state + predicate), reusing the same explanation the server-side
  `traversal_policy()` function encodes, so the UI never contradicts the
  policy that produced the badge.
- No `RelationAssertion`/entity schema change. No new Rust code except
  possibly exposing `traversal_policy()`'s reasoning string if the current
  export doesn't already carry enough to explain it in words — check
  `assertion_export.rs` before assuming a Rust change is needed at all.

**Exit criterion:** the lineage view's default rendering matches
ARCHITECTURE_NEXT §12 Phase 3's own exit line: *"Show proposed relations
only behind an explicit UI control."*

## Item 2 — `EntityId` foreign-key cutover

Named explicitly in `docs/P3_STATUS.md` as the deliberately-deferred,
larger, breaking migration: *"Cutting `RelationAssertion` over to
reference `EntityId` directly ... touches `insert_assertion`'s
validation, every adapter, every export."*

Proposed shape, following this project's own established pattern for
migrations (P1's provenance sidecar → P2's direct generation: prove
additively, then cut over, never a big-bang rewrite):

1. Add nullable `subject_entity_id INTEGER REFERENCES entities(id)` /
   `object_entity_id INTEGER REFERENCES entities(id)` columns to
   `relation_assertions`, alongside the existing `subject_ref`/
   `object_ref` TEXT columns — additive, no existing row invalidated.
2. Backfill both columns via `resolve_entity_ref` for every existing
   assertion. `docs/P3_STATUS.md` already proved 209,416/209,416 (100%)
   of existing references resolve — the backfill should not lose a single
   row, and the release gate should fail loudly if it ever does.
3. Add an invariant check to `verify-release`: every assertion's
   `subject_entity_id`/`object_entity_id` (once populated) must equal
   `resolve_entity_ref(subject_ref)`/`resolve_entity_ref(object_ref)` —
   catches drift between the string and the FK during the transition
   period when both exist.
4. Only after that invariant holds cleanly across a full real-data run:
   switch readers (`web_export.rs`, `assertion_export.rs`,
   `catalog_adapter.rs`'s coverage report) to the FK columns, then in a
   later, separate release drop the TEXT columns. This document does not
   propose doing steps 1–4 in one sitting.

**Not proposed:** changing the wire format of `subject_ref`/`object_ref`
strings themselves (e.g. `"judgment:5635"`) — they remain a legitimate,
human-readable display/debug format; only their role as the *authoritative*
join key changes.

## Item 3 — declarative `RelationSchema` table

`relation_policy::valid_entity_kinds` is correct today but is a hardcoded
Rust `match` — policy as code, not policy as data. `docs/P3_STATUS.md`
already flagged this: *"a real declarative schema table is not built
here."*

Proposed shape:

- A `relation_schema` table: `(predicate TEXT, allowed_subject_kind TEXT,
  allowed_object_kind TEXT)`, seeded with **exactly** the pairs
  `valid_entity_kinds` encodes today — a direct, verifiable transcription
  (a test that asserts the table's contents equal the function's truth
  table for every `(RelationKind, EntityKind, EntityKind)` combination),
  not a chance to quietly change policy while "just" moving it to data.
- `valid_entity_kinds` becomes a query against this table (or the table
  becomes the source of truth loaded once at `ProvenanceStore::open`,
  with the Rust function kept only as a compiled-in fallback/default seed
  for a fresh database).
- This is prep work: a table only pays off once something needs to read
  or extend policy without a code change — most plausibly a future
  curator-facing admin view. It is not proposed as a behavior change by
  itself, and should not be built as speculative infrastructure if no
  such reader is imminent (this project's own working discipline argues
  against building it before item 1 or item 2, unless the user has a
  concrete near-term reason to want policy editable as data).

## Explicitly out of scope for this plan

- **The review workbench itself** (ARCHITECTURE_NEXT §10 / Phase 3's
  headline feature: reviewer roles, accept/reject/split/merge UI,
  per-scope publication). Still blocked on the same infrastructure gap
  `docs/P2_STATUS.md` and `docs/P3_STATUS.md` both name: no backend, no
  auth, a static site. Items 1–3 here are the trust-and-policy groundwork
  a future workbench would need, not the workbench.
- **`Statement`/`ProofArtifact` distinction** — still blocked on the
  absence of checker-verified proof artifacts in real data, unchanged
  reasoning from `docs/P1_STATUS.md`/`docs/P3_STATUS.md`.
- **Semantic relation sub-typing** (`semantic_level`/`witness_type`/
  `formal_system`) — unchanged reasoning from `docs/DATA_DICTIONARY.md`'s
  "Known limitations."

## Recommended order

1. Item 1 (client wiring) — ships a real, user-visible ARCHITECTURE_NEXT
   §7/§12 guarantee with the data that already exists, no migration risk.
2. Item 2 (`EntityId` FK cutover) — the biggest lift; also the
   prerequisite for item 3 mattering beyond decoration (a `RelationSchema`
   table is much less useful while the thing it would validate is still a
   loosely-typed string).
3. Item 3 (`RelationSchema` table) — only once there's a concrete reader
   for policy-as-data, per the note above.

## Where to look when this starts

- `crates/mathesis-provenance/src/relation_policy.rs` — `traversal_policy`,
  `valid_entity_kinds`, the exact truth tables item 1 and item 3 depend on.
- `crates/mathesis-provenance/src/assertion_export.rs` — confirm what
  `traversalPolicy` reasoning is already exported before assuming new
  Rust is needed for item 1.
- `web/src/proofGraph.ts`, `lineageView.ts`, `provenancePanel.ts` — the
  client files item 1 touches.
- `crates/mathesis-provenance/src/entity.rs`, `release_gate.rs` — item 2's
  `resolve_entity_ref` and the invariant-check pattern to extend.
- `docs/P3_STATUS.md` — the two items (2 and 3) named there as deferred,
  verbatim.

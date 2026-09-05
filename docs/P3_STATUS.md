# Phase 3 status — a typed entity catalog, additively

> Written 2026-09-05, immediately after the P1/P2 stabilization pass. The
> user's instruction at hand-off: "stop patching P1/P2, do P3 with using
> the lessons learned so far." P3, per ARCHITECTURE_NEXT.md §5.2 and this
> project's own `docs/DATA_DICTIONARY.md`'s "Known limitations" (which
> named this exact gap on 2026-09-05), is the typed entity catalog:
> `subject_ref`/`object_ref` becoming real records instead of tagged
> strings. This document covers P3's first increment.

## What changed

Before this increment, `RelationAssertion.subject_ref`/`object_ref` were
`"kind:id"` tagged strings (`"judgment:5635"`, `"concept:elliptic curve"`,
`"paper:math/0001"`) with no independent record behind them — no stable
numeric id, no canonical display name, no way to tell whether two
different tagged strings (e.g. a concept phrase and one of its spelling
variants) actually referred to the same real-world object.

`crates/mathesis-provenance/src/entity.rs` + `catalog_adapter.rs` add:

- Two new tables, `entities` (one row per real judgment/concept/paper,
  with a stable `EntityId` and a `display_label`) and `entity_refs` (every
  known way to refer to that entity — for concepts, the representative
  phrase *and* every alias `mathesis-taxonomy`'s own Entity Resolution
  (`resolve.rs`) already computed, mapped to the same entity).
- `mathesis-provenance build-catalog --graph-db <path> --taxonomy-db <path>
  --db <path>` populates it from the exact same legacy sources
  `legacy_adapter.rs` already trusts — no new heuristics, no invented
  labels. A judgment's label is its own `name` (a generic
  `"(anonymous theorem)"`-style placeholder if it has none — never a
  guessed title); a paper's is its own `title` (its arXiv id if none was
  ever ingested); a concept's is the representative phrase from the
  *same* `resolve::resolve()` call `mathesis-taxonomy export` itself
  makes, so the catalog can never disagree with what the search index
  calls something.
- Idempotent and honestly reported: re-running prints `+0 (skip N)` the
  second time, matching `import_graph`'s own `+N (skip M)` convention —
  not `+N` again, which would misleadingly look like growth.
- `mathesis-provenance stats` now prints entity counts by kind and an
  **assertion reference coverage** figure: how many of the *existing*
  assertions' `subject_ref`/`object_ref` values actually resolve to a
  cataloged entity. On real `v0-baseline-20260905` data: **209,416/209,416
  (100%)** — every subject and object of every one of the 104,708
  assertions already in the store resolves cleanly, with zero
  hand-tuning. (`94,278` concepts is the exact "resolved concepts" figure
  independently recorded in `docs/RELEASES.md`'s baseline — a real
  cross-check, not a coincidence.)
- `assertions.json` (the detail-panel export) now carries `subjectLabel`/
  `objectLabel` alongside the existing `subjectRef`/`objectRef` — `null`
  when the catalog has no entry for that reference, never a guess.
  `web/src/provenancePanel.ts` shows the label next to the tagged
  reference when one exists. Verified in the browser on real data: the
  detail panel for a real morphism now reads *"unitBallApproxEps
  judgment:1 —equivalent_to→ deGiorgiTail judgment:403"* instead of just
  the two opaque `judgment:N` tags.

## What this increment deliberately does NOT do

- **`subject_ref`/`object_ref` are still tagged strings, not real foreign
  keys.** The catalog is additive — every existing assertion, the entire
  `web-export` pipeline, and the P1/P2 release gate are byte-for-byte
  unchanged by this increment (confirmed: `verify-release` still passes
  cleanly against the same real data after `build-catalog` runs).
  Cutting `RelationAssertion` over to reference `EntityId` directly is a
  separate, larger, and *breaking* migration (it touches
  `insert_assertion`'s validation, every adapter, every export) that this
  increment does not attempt — matching this project's own established
  discipline of proving a piece additively before cutting anything over
  (the same shape P1→P2 took with the provenance sidecar → direct
  generation transition).
- **No `RelationSchema` table.** ARCHITECTURE_NEXT.md §5.2 also describes
  a table declaring which entity kinds each predicate allows.
  `validate_relation_kinds` (existing, from P1) already does this
  narrowly in code; a real declarative schema table is not built here.
- **No `Statement`/`ProofArtifact` distinction.** Judgments are cataloged
  as one kind; ARCHITECTURE_NEXT.md's fuller model separates `Statement`
  (any formal claim) from `ProofArtifact` (a checked proof). Real Lean
  data in this project doesn't yet carry checker-verified proof
  artifacts (`verified` epistemic state is still unpopulated, per
  `docs/P1_STATUS.md`), so building that distinction now would be
  speculative.
- **Reference coverage is not a release gate.** `stats` reports it as
  information; `verify-release` does not fail if coverage drops below
  100%. Making it a hard gate is a reasonable next step once the catalog
  has been relied on for something client-visible for a while — not yet.
- **No entity-level merges/splits, no curator workflow.** The catalog
  reflects Entity Resolution's existing, automatic alias grouping
  exactly as `mathesis-taxonomy` already computed it. A human correcting
  a wrong merge, or splitting two genuinely different concepts that
  collided under the same canonical key, is the review-workbench work
  ARCHITECTURE_NEXT.md §10 describes — still blocked on the same
  infrastructure gap named in `docs/P2_STATUS.md` (no backend, no auth,
  a static site).
- **No semantic relation sub-typing** (`semantic_level`/`witness_type`/
  `formal_system`). Still correctly out of reach without fabricating
  data — `docs/DATA_DICTIONARY.md`'s "Known limitations" reasoning is
  unchanged and this increment doesn't revisit it.

## Stabilization patch after Increment 1

The additive catalog is now used as a validation boundary without making a
breaking foreign-key migration:

- `relation_policy.rs` defines the allowed entity-kind pairs for every
  relation and separates traversal policy from epistemic state. A proposed
  semantic relation is visible but not eligible for default traversal; a
  formally verified relation may be traversable by default.
- `verify-release` validates every assertion in the release when a catalog is
  present. Unresolved endpoints and relation-schema violations fail the
  release gate; an empty catalog remains a documented compatibility state for
  pre-catalog databases.
- Assertion detail exports now include `traversalPolicy` and explicit evidence
  locator precision (`approximate_location`, `source_only`, `model_output`,
  `formal_artifact`, or `reviewer_note`). A broad legacy locator is not
  presented as an exact source span.
- `build-catalog` records catalog metadata in the provenance database:
  catalog schema/build version, Entity Resolution version, both input hashes,
  and entity/alias counts. Reconciliation includes that metadata in
  `provenance-manifest.json`, and `verify` rejects a manifest whose catalog
  metadata differs from the live catalog.
- Relation imports now carry the versioned source-mapping policy
  `mathesis-source-mapping-v1`; release verification rejects a manifest made
  with a different mapping policy.
- Entity labels record whether they are source-provided, canonicalized,
  derived, or fallback identifiers. Assertion details expose that origin, and
  evidence details expose locator precision rather than implying that every
  legacy locator is an exact span.
- `source_adapter.rs` defines the contract future TheoremGraph/OpenAlex/MSC
  adapters must satisfy: stable source revisions and hashes, typed assertion
  endpoints, mapping-policy version, and complete licensing metadata. Synthetic
  fixtures exercise the contract without importing external data.
- `licensing.rs` validates that a source record is not treated as
  redistributable without both a license and attribution. Existing legacy
  records may remain incomplete and are reported as such rather than silently
  gaining a license.
- Adversarial release-gate tests cover catalog endpoint drift and unknown
  source-mapping policy versions in addition to the P1/P2 corruption cases.
- MSC2020 is now connected through `msc_adapter.rs` and the
  `mathesis-provenance import-msc` command. It imports the official bundled
  snapshot as 6,603 source-provided concept entities and 6,540 extracted
  parent/child `specializes` assertions. The snapshot hash, official URL,
  CC-BY-NC-SA-4.0 license, attribution, adapter version, and parser version
  are stored in `SourceRecord`; reruns are idempotent.
- TheoremGraph is deliberately not imported yet. Its public API and graph
  schema are known, but the public documentation inspected on 2026-09-05
  does not state a redistribution license for graph payloads. Until a license
  or written permission is supplied, importing or publishing its snapshot
  would violate this project's licensing contract. The adapter gate must fail
  closed rather than treating API availability as permission.

This still intentionally does not claim that a catalog resolution proves
mathematical identity or truth. It proves only that the current release's
references resolve deterministically under the recorded catalog.

## Where to look

- `crates/mathesis-provenance/src/entity.rs` — the `entities`/
  `entity_refs` CRUD, with idempotency and alias-resolution tests.
- `crates/mathesis-provenance/src/catalog_adapter.rs` — the builder
  (judgment/paper from `mathesis-graph`, concept from `mathesis-taxonomy`'s
  own `resolve::resolve`), the coverage report, and tests proving labels
  are never invented (an anonymous judgment gets a placeholder naming its
  kind, not a guessed title; an untitled paper falls back to its arXiv
  id) and that spelling variants fold exactly the way `taxonomy export`
  itself folds them.
- `crates/mathesis-provenance/src/main.rs`'s `build-catalog` subcommand
  and the extended `stats` output.
- `crates/mathesis-provenance/src/assertion_export.rs::entity_label_for`
  — how `assertions.json` picks up labels.
- `web/src/provenancePanel.ts::renderRef` — the client side.

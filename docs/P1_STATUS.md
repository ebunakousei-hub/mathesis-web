# Phase 1 status — what it guarantees, and what it doesn't

> Written 2026-09-05 after two rounds of external review, closing with a
> completion gate: `mathesis-provenance verify` (see below). This document
> exists so Phase 1's scope is never overstated — provenance coverage is not
> the same claim as mathematical correctness.

## P1 now guarantees

- **Every currently displayed edge has a resolvable assertion.**
  `mathesis-provenance reconcile` re-derives the exact identity key each
  displayed edge (`judgment_dependencies`, `paper_citations`, `morphisms`,
  the shipped subset of `concept_relations`) was imported under, and only
  writes a sidecar entry when that key resolves to a real
  `RelationAssertion`. Verified on the real `v0-baseline-20260905` data:
  8,969/8,969 edges (5,634 dependencies + 2,284 morphisms + 1,051 relations).
- **Every assertion is linked to a release**, and that link is checked, not
  assumed: `mathesis-provenance verify` confirms each assertion's
  `release_id` matches the release the manifest and sidecars claim.
- **Every assertion has at least one Evidence row**, and every Evidence row's
  `source_record_id` resolves to a real `SourceRecord` — checked per-edge by
  `verify`, not inferred from the schema's foreign keys alone.
- **No duplicate identity key resolves ambiguously** — `verify` checks that
  no two sidecar entries sharing a natural key (e.g. the same morphism id,
  or the same `(subject, object, kind)` triple) point to different
  assertions.
- **A sidecar cannot silently drift from the release it claims to
  represent.** `provenance-manifest.json` records the adapter name/version,
  a schema version, and a SHA-256 of each input database; `verify` recomputes
  those hashes and fails if they've changed since generation.
- **Provenance is available in the running Web application**, not just in a
  standalone database — `web/` fetches the sidecars and shows a "Provenance:
  assertion #N" control on every morphism chip and every typed relation,
  which opens a detail panel (relation kind, subject/object, epistemic
  state, each Evidence's kind/locator/source, review decisions, and whether
  the assertion is eligible for default trusted traversal per
  ARCHITECTURE_NEXT.md §7).
- **Missing provenance does not break legacy rendering.** A missing or
  404'd sidecar changes nothing about what the page shows — this is
  deliberate ("legacy compatibility mode"). A sidecar that loads with the
  wrong shape, a non-404 HTTP error, or a network failure is reported
  loudly in development (`console.error` plus a visible on-page banner,
  `util.ts::reportProvenanceIssue`) so a broken release build can't pass as
  a working one unnoticed — but stays silent in production, so a visiting
  researcher never sees an internal-tooling warning.
- **Reconciliation and verification are re-runnable, not one-time claims.**
  `mathesis-provenance verify` is meant to run every time a release is
  produced (a CI/release gate), not just once during development.

## P1 does not yet guarantee

- **Mathematical truth.** An `extracted` or `proposed` relation is exactly
  as uncertain as its epistemic state says — a traceable Evidence chain is
  not a correctness proof. `eligibleForDefaultTraversal: false` in the
  detail panel is the honest signal for this, not an implementation gap.
- **Exact source spans for every edge.** `judgment_dependencies` locators
  are file:line references, not byte-precise spans; `paper_citations` has
  no locator at all (deliberately `None` rather than a fabricated one, since
  `mathesis-fulltext::citation` never retained one). Only Hearst-derived
  taxonomy relations carry an exact quoted sentence.
- **Complete typed entity validation.** `subject_ref`/`object_ref` are
  `"kind:id"` tagged strings, not foreign keys into a real typed catalog.
  `insert_assertion` rejects the specific nonsense combinations this
  session's own adapter could produce (e.g. `paper:X specializes paper:Y`)
  but is not the `Entity`/`RelationSchema` catalog ARCHITECTURE_NEXT.md
  §5.2 describes.
- **Formal proof verification.** No epistemic state in this system is
  produced by a proof assistant; `verified` is defined but nothing populates
  it yet (§9 priority 4, the Lean elaborator adapter, is not built).
- **Human review**, beyond preserving legacy `Accepted` dispositions as
  unattributed `ReviewDecision` records. There is no reviewer UI, identity,
  or conflict-resolution workflow.
- **Semantic equivalence of relation labels.** `equivalent_to` does not
  distinguish definitional equality from isomorphism from "the same
  informal concept under different names" — see
  `docs/DATA_DICTIONARY.md`'s "Known limitations" for why this was not
  built speculatively.

## Where to look

- `docs/DATA_DICTIONARY.md` — the relation-kind/epistemic-state vocabulary,
  the legacy→target mapping, and the "Known limitations" section listing
  what's deliberately deferred.
- `docs/RELEASES.md` — the `v0-baseline-20260905` release manifest, corpus
  counts, and the provenance sidecar file table.
- `web/README.md`'s "証拠層への追跡" section — how the sidecars reach the
  running application.
- `crates/mathesis-provenance/src/verify.rs` — the completion gate itself;
  `crates/mathesis-provenance/tests/verify_test.rs` — its adversarial test
  coverage (wrong release, missing assertion, missing evidence, ambiguous
  duplicate key, counts mismatch, input-file hash drift, manifest/DB
  mismatch).

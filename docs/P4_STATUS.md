# Phase 4 status — the OpenAlex adapter, and an honest negative result

> Written 2026-09-06, executing `docs/P4_PLAN.md`. The user's instruction:
> implement the OpenAlex adapter now (its CC0 license needs no department
> permission, unlike TheoremGraph/math-graph, which stay blocked), following
> a 5-step order ending in "measure how many previously uncited Mathesis
> papers gain citation links." This document reports what was built and
> what the real, live run against production data actually showed —
> including a result smaller than hoped, recorded honestly rather than
> reframed.

## What changed

- `crates/mathesis-provenance/src/openalex_fetch.rs` (new): the only code
  in this crate that touches the network (`ureq`, matching
  `mathesis-fulltext::source`'s existing convention exactly — same
  `User-Agent`, same "pure functions are tested, the network call isn't"
  split). `arxiv_doi(id)` builds arXiv's own retroactive
  `10.48550/arXiv.<id>` DOI (confirmed live against a real record —
  OpenAlex's `Work.ids` has no direct `arxiv` key, only
  `openalex`/`doi`/`mag`/`pmid`/`pmcid`); `fetch_work_by_arxiv_id` resolves
  a Work by that DOI and returns a minimal `SnapshotEntry` (bare OpenAlex
  work id, DOI, title, bare referenced-work ids). `canonical_content_hash`
  hashes only the fields this crate actually reads, not the raw API
  response — so an unrelated field OpenAlex changes tomorrow doesn't
  spuriously mint a new source revision.
- `crates/mathesis-provenance/src/openalex_adapter.rs` (new): pure,
  network-free import, mirroring `msc_adapter.rs`'s direct-construction
  style (no `SourceAdapter` trait — as before, that trait remains a
  documented contract exercised only by its own fixture tests, not by
  either real adapter). Every `Paper` entity gets its OpenAlex work id and
  DOI added as `entity_refs` aliases alongside the existing `paper:<id>`
  ref. **Citation snowball, enforced in code, not just by convention**: a
  `referenced_works` entry only becomes a `Cites` assertion when it
  matches another paper *already in the snapshot* — i.e., already
  cataloged. A reference to anything outside that set is counted
  (`references_outside_catalog`) and dropped, never fetched further. This
  is the literal implementation of ARCHITECTURE_NEXT.md §3's "no universal
  crawler" boundary and the user's explicit "do not randomly ingest the
  entire OpenAlex corpus yet."
- `Cites` assertions get `epistemic_state: observed` — not a new
  precedent: `legacy_adapter.rs` already uses this exact
  predicate+state pair for `mathesis-fulltext`-parsed `\cite` commands
  (`paper_citation_becomes_cites_observed`, currently exercising 0 rows
  since `paper_citations` is empty in the real graph database). The
  **evidence kind is deliberately `source_span`, not `formal_export`**:
  `assertion_export.rs` maps `FormalExport` to the locator-precision label
  `"formal_artifact"`, a word this project otherwise reserves for
  checker-verified proof artifacts (a future Lean elaborator import). An
  OpenAlex citation is a real, structured, authoritative fact — but
  calling it "formal_artifact" would overclaim in the same way this
  project already refuses to call automatic agreement "reviewed." It gets
  `source_span` with a real, specific locator instead (naming the exact
  OpenAlex work id it was found in), which resolves to
  `"approximate_location"` — an honestly *more* precise locator than the
  sibling `\cite`-derived evidence, which has to leave `locator: None`.
- `stats` now prints citation coverage (`crates/mathesis-provenance/src/
  stats.rs`, `openalex_adapter::citation_coverage`): how many cataloged
  `Paper` entities have at least one `Cites` edge, in or out. Safe to call
  on a DB that has never run this adapter (reads 0/N, not an error).
- New CLI commands, deliberately split by network boundary: `fetch-openalex
  --graph-db <path> --out <snapshot.json>` is the only command that calls
  the live API; `import-openalex --db <path> --release <tag> --snapshot
  <path>` is a pure, offline, idempotent import of whatever snapshot file
  it's given.
- 10 new tests in `openalex_adapter.rs` + 5 in `openalex_fetch.rs`, all
  network-free, covering every item on the user's checklist: idempotent
  reruns produce no duplicates; a changed snapshot content hash creates a
  new source revision while the old one survives; a reference outside the
  snapshot's own paper set is skipped, not fabricated into an edge; a
  malformed snapshot entry (empty id) is rejected, not silently imported;
  every `Cites` assertion has exactly one resolvable `Evidence` row; the
  `Paper`→`Paper` shape is validated by the *existing*
  `relation_policy::valid_entity_kinds`/`insert_assertion` guard, not new
  code; the adapter's own `SourceRecord` shape is checked against the
  *existing* `licensing::validate_source_record` gate. 67/67 tests pass
  workspace-wide (`cargo test -p mathesis-provenance`); full workspace
  build is clean.

## What the real, live run actually showed

Run against `scratch/judgments.db`'s real 138 `Paper` entities (backed up
to `scratch/provenance.db.bak-pre-openalex` before the first live write, as
a precaution for this crate's first outbound-network adapter):

- **`fetch-openalex`: 133/138 papers found on OpenAlex** (5 not found:
  `0710.2320`, `0808.4038`, `1003.2821`, `1104.0685`, `hep-th/0703111` —
  arXiv ids OpenAlex doesn't resolve by their derived DOI; not
  investigated further, since 5/138 not being indexed by an external
  corpus is an expected, unremarkable gap, not a bug in this adapter).
- **`import-openalex`: 133 papers linked, 1,106 references examined,
  0 point to another cataloged paper.** `citation_coverage` reports
  **0/138**. Rerunning the exact same snapshot correctly reports
  `papers linked +0 (already linked 133)`, confirming idempotency on real
  data, not just in the unit tests.
- **This is the same disconnected-sample finding memory already recorded
  once** ([[paper-citation-needs-connected-sample]], from a prior 200-paper
  random sample that also produced zero connected citation edges) — now
  reproduced against a *different*, *real*, *production* seed set, via a
  fully correct implementation of the exact snowball strategy the plan
  called for. The root cause is structural, not a sampling accident: each
  of these 138 papers was individually interned because *one* Lean
  judgment in this project's corpus cites it (via `source_paper`) — they
  were never selected to form a citation-dense neighborhood, so there was
  never a strong reason to expect them to cite each other. **Confirms
  (does not contradict) the plan's own risk framing in
  `docs/P4_PLAN.md`.**
- **Regression check (`web-export` + `verify-release`), run for real, not
  assumed**: `dependencies.json`/`morphisms.json`/`relations.json`
  generated after the OpenAlex import are byte-for-byte identical to the
  versions already in `web/public/` (confirmed by diff) — expected, since
  none of the three export functions in `web_export.rs` read the `Cites`
  predicate at all yet. `verify-release` passes cleanly against the
  OpenAlex-updated `scratch/provenance.db` with the *existing* manifests
  and sidecars, exactly as it did before this increment. P1/P2 remain
  fully untouched, honoring "don't ingest until this path is verified" and
  the broader "stop patching P1/P2" instruction from the prior round.

## What this increment deliberately does not do

- **No `citations.json` web-export file.** With 0 real edges to show, and
  the existing web app having no citation-graph UI surface at all, adding
  a fourth export file now would be speculative infrastructure ahead of
  data — the same discipline this project applied when it declined to
  build a `RelationSchema` table before anything needed to read policy as
  data. `RelationKind::Cites` assertions exist in the provenance database,
  fully evidenced and coverage-tracked via `stats`, ready for a web-export
  extension the moment there's a non-empty result worth shipping.
- **No expansion of the seed set beyond `mathesis-graph`'s 138 `Paper`
  entities.** The much larger `mathesis-taxonomy` corpus (142,948 papers,
  `docs/RELEASES.md`'s baseline) is not yet a source of `EntityKind::Paper`
  catalog entities at all — pulling citation data from it would mean
  designing a new, larger seeding step first (which papers, cataloged how,
  fetched at what volume), a different and larger decision than "run the
  adapter already built," not something to charge into under an
  "implement the adapter" instruction without a separate go-ahead.
- **TheoremGraph and math-graph remain untouched**, per instruction —
  nothing in this pass reads or assumes anything about either dataset.
- **The 5 not-found arXiv ids are not retried under a different DOI
  scheme or investigated further.** A small, expected gap, not chased.

## Where to look

- `crates/mathesis-provenance/src/openalex_fetch.rs` — the network layer
  and the `SnapshotEntry` shape.
- `crates/mathesis-provenance/src/openalex_adapter.rs` — the import logic,
  the snowball boundary enforcement, `citation_coverage`.
- `crates/mathesis-provenance/src/main.rs`'s `fetch-openalex`/
  `import-openalex` subcommands.
- `docs/P4_PLAN.md` — the plan this executed; its risk framing (snowball
  fetch strategy, why citation edges might not materialize on a small
  seed) held up against the real result.
- Memory: [[paper-citation-needs-connected-sample]] — now confirmed twice,
  on two different real samples, with two different collection strategies.

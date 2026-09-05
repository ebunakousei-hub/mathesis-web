# Phase 4 plan — the next external-source adapter (not yet started)

> Written 2026-09-06. Status: **plan only, no code changes**. The user is
> away pending a department reply about licensing/API terms for external
> datasets (TheoremGraph, math-graph); this document scopes the next
> increment so it is ready to execute without re-deriving context, but
> deliberately stops short of writing any adapter code. Nothing here
> commits to a specific external dataset beyond OpenAlex, whose license
> (CC0) is not in question.

## Why this is next

`docs/P3_STATUS.md`'s stabilization pass built the machinery a second
external-source adapter needs but never exercised on a second real source:

- `crates/mathesis-provenance/src/source_adapter.rs`'s `SourceAdapter`
  trait + `validate_adapter` — a contract that fails closed on missing
  license/attribution, an unsupported mapping-policy version, or a
  subject/object kind pair `relation_policy::valid_entity_kinds` doesn't
  allow.
- `crates/mathesis-provenance/src/msc_adapter.rs` — the one adapter that
  has actually implemented this contract against real data (6,603 concept
  entities, 6,540 `specializes` assertions from the bundled MSC2020 CSV).
  It is the template: `get_or_insert_source_record` with full license
  metadata, `get_or_insert_entity`, `insert_assertion` gated by the same
  policy check, everything idempotent by `stable_record_key`.

MSC proved the contract against a small, static, single-file source.
ARCHITECTURE_NEXT.md §9's priority order names OpenAlex next — "paper
identity, metadata, citation context ... a canonical metadata supplement
rather than a new crawler" — and it is the one source in that list whose
license is already unambiguous (OpenAlex data is CC0). It does not depend
on the pending department reply, so it is the correct next target to
*plan* now and implement once the user is back and greenlights it.

## What it would map

| OpenAlex object | Mathesis entity/assertion |
| --- | --- |
| a `Work` matching an arXiv id already in the catalog | `EntityKind::Paper`, cross-linked via `provider_id` = OpenAlex Work ID, with DOI/arXiv id recorded as additional identifiers on the `SourceRecord` |
| a `Work`'s `referenced_works` | `RelationKind::Cites`, `EpistemicState::Observed` (OpenAlex citation edges are the source's own asserted fact, same status Lean-exported dependencies get — not a heuristic) |

`RelationKind::Cites` already requires `Paper -> Paper` under
`relation_policy::valid_entity_kinds` — no policy change needed, only an
adapter that produces well-formed fixtures for it.

This directly closes a gap `docs/RELEASES.md` already names in its own
words: *"0 citations, since `paper_citations` is empty in this release."*
[[paper-citation-needs-connected-sample]] in memory records why a random
100k-paper sample produced zero connected citation edges before — the fix
proposed there (snowball collection, not a bigger random sample) is the
fetch strategy below.

## Fetch strategy (bounded, not a crawler)

Per ARCHITECTURE_NEXT.md §3.1 ("no universal crawler as the first
milestone"):

1. Seed set = arXiv ids already present as `Paper` entities in the
   catalog (currently sourced from `mathesis-taxonomy`'s corpus).
2. For each seed, fetch its OpenAlex `Work` record by
   `https://api.openalex.org/works/https://doi.org/<arxiv-derived-doi>`
   or the `arxiv:<id>` external-id filter (OpenAlex supports both; needs
   a short live spike to confirm which resolves more seeds before
   committing to one).
3. Keep a `Cites` edge only when **both** endpoints already resolve to a
   cataloged `Paper` entity. This is the snowball-connectivity fix: it
   guarantees every imported edge sits inside the corpus we can actually
   show context for, instead of citing into a void.
4. No API key required (OpenAlex's polite pool); record a contact email
   in the User-Agent per their usage policy — does not require the
   pending department permission, since it is OpenAlex's own published
   terms, not a bespoke agreement.

## Shape of the adapter (mirrors `msc_adapter.rs`)

- New `crates/mathesis-provenance/src/openalex_adapter.rs`, implementing
  `SourceAdapter` (for contract tests) plus an `import()` function
  matching `msc_adapter::import`'s signature and idempotency pattern.
- `SourceLicense { license: "CC0", attribution: "OpenAlex", source_url:
  "https://openalex.org/", redistribution_allowed: true }`.
- `mapping_policy_version()` = the existing
  `relation_policy::SOURCE_MAPPING_POLICY_VERSION` — no new version needed
  unless the mapping rules change, which they don't here.
- Fixture-based contract tests only (a handful of hand-built OpenAlex
  `Work` JSON snippets under `tests/fixtures/`) — no live network calls in
  the test suite, matching every existing adapter/import test in this
  workspace.
- A new `mathesis-provenance import-openalex --db <path> --release <tag>
  --cache-dir <path>` CLI subcommand, following `import-msc`'s shape once
  that exists (not yet added — `msc_adapter::import` is currently called
  inline; check `main.rs` for the actual wiring point before adding a
  sibling command).

## What this plan deliberately does not decide yet

- **Exact pagination/rate-limit numbers.** Needs a short live spike
  against the real API before committing to a page size or backoff
  schedule — not something to guess in a planning document.
- **Primary provider_id choice** (OpenAlex Work ID vs. DOI) — both are
  stable; pick whichever resolves more of the existing arXiv seed set in
  the spike above.
- **Author/venue/MSC-class metadata from OpenAlex.** ARCHITECTURE_NEXT
  §9 calls OpenAlex "a canonical metadata supplement," which could extend
  to author identity and venue in a later increment — out of scope here,
  where the concrete gap being closed is citation edges.

## TheoremGraph and math-graph — explicitly blocked, not planned here

The user has asked the department about licensing/API terms for both.
`docs/P3_STATUS.md`'s stabilization pass already built the gate these
would need to pass:

- `licensing::validate_source_record` rejects any source record with an
  empty license or attribution.
- `source_adapter::validate_adapter` rejects any adapter whose
  `redistribution_allowed` is `false`, independent of whether the data is
  technically fetchable.

This means **no code change is required to safely wait** — the gate
already fails closed. Once a license is confirmed, the next step is a
short research pass (not started, not assumed here) into each dataset's
actual schema and API shape before writing an adapter plan as concrete as
this document is for OpenAlex. Nothing about either dataset's schema is
assumed or fabricated in this document.

## Where to look when this starts

- `crates/mathesis-provenance/src/msc_adapter.rs` — the template.
- `crates/mathesis-provenance/src/source_adapter.rs` — the contract this
  must satisfy, and its existing fixture-based test pattern.
- `crates/mathesis-provenance/src/relation_policy.rs` — confirms `Cites`
  needs no policy change.
- `docs/RELEASES.md` — the "0 citations" gap this closes.
- Memory: [[paper-citation-needs-connected-sample]] for why the fetch
  strategy above snowballs from known papers instead of sampling randomly.

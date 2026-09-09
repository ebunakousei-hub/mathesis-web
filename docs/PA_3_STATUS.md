# Phase A.3 status — the MSC classification-status model (改善点.txt item 9)

> Written 2026-09-09, executing item 9 from 改善点.txt's "Do next" list:
> explicit classification statuses (classified/unclassified/unavailable/
> outside-scope/pending/rejected), kept distinct from the MSC code itself,
> each retaining MSC revision/source/evidence/classifier version/release/
> confidence/review status. Full research into how MSC classification
> actually works today (not assumed) preceded any code — see the design
> rationale in `docs/DATA_DICTIONARY.md`'s new "MSC classification status"
> section, which this document doesn't repeat.

## What was built

- **`crates/mathesis-provenance/src/msc_classification.rs`** (new): the
  `ClassificationStatus` enum (6 states), `MscClassification`/
  `MscClassificationInsert` structs, a new `msc_classifications` SQLite
  table (`store.rs`, additive — `CREATE TABLE IF NOT EXISTS`, no migration
  of existing tables needed), and `classify_from_taxonomy()` — opens
  `mathesis-taxonomy`'s own SQLite DB directly (the same cross-crate
  pattern `reconcile` already uses for `GraphStore`/`TaxonomyStore`),
  reads `load_cluster_alignments()`, and classifies each cluster using
  the *exact same* `is_novel()`/`is_confident()` methods `export.rs`
  already uses — not reimplemented, reused, so the two can't silently
  drift apart.
- **New CLI command** `classify-msc --db <provenance> --taxonomy-db
  <taxonomy> --release <tag>` (`main.rs`), idempotent via `(cluster_id,
  release_id)` upsert — reruns for the same release replace, never
  duplicate.
- **`mathesis-taxonomy`'s `export.rs`**: added `pending_clusters` to
  `TaxonomyExport` — the "ambiguous" bucket (`!is_confident() &&
  !is_novel()`) has existed since before this session but was only ever
  exposed as a bare count (`ambiguousClusterCount`); this pass lists the
  actual clusters (`size > 1`, matching `novel_clusters`'s own filter),
  sorted by confidence descending.
- **Frontend** (`dynamicTaxonomy.ts`, `types.ts`, `i18n.ts`,
  `searchWorker.ts`): a third tab, "Pending (unconfirmed candidates)",
  alongside the existing "Browse by MSC field" / "Unclassified" tabs from
  Phase A/PA.2. Reused the existing cluster-card rendering, generalized
  its `novel: boolean` parameter to `mode: "field" | "pending" |
  "novel"` since pending clusters *do* carry a candidate `dominantCode`
  (unlike novel ones) but need their own explanatory hint. Updated the
  stats line and `mscScopeHint` to mention all three states.
- **`mathesis-provenance stats`**: prints DB-wide `msc_classifications`
  totals by status (item 9's own "coverage metrics" bar — item 10's
  fuller per-source/per-record-type breakdown is separate, unstarted).
- **`docs/DATA_DICTIONARY.md`**: new section closing a real, confirmed
  gap — MSC classification had zero mentions in this document before
  this pass, despite being computed since well before this session.

## Deliberately not populated: `unavailable` / `outside_scope` / `rejected`

Verified real data, not guessed: the current `classify_from_taxonomy`
adapter produces **only** `classified`/`pending`/`unclassified` —
structurally, since those are the only three outcomes
`ClassificationStatus::from_alignment` can return from a
`ClusterAlignment`. The other three states are defined in the enum (the
schema/type-level ask item 9 makes) but have zero rows, for reasons
specific to each, not laziness:

- `unavailable` needs a second adapter pass over entity kinds the
  taxonomy pipeline never touches (Lean judgments, Math-Graph nodes) —
  real, scoped future work, not built this pass.
- `outside_scope` has no current pipeline signal that would distinguish
  it from ordinary low-confidence `pending` — inventing one now would be
  exactly the kind of fabrication this project's conventions rule out.
- `rejected` needs an actual authenticated-review decision against a
  classification, and no MSC-classification review workflow exists yet
  (consistent with `review_decisions`: 0 rows in production for every
  other axis in this codebase too — this isn't a special case).

## Two bugs found while verifying, one fixed and tested, one flagged

**Fixed**: regenerating `taxonomy.json` to test `pending_clusters`
revealed that `export.rs`'s `fields` array ordering was **not
deterministic across runs of the same input data** — `by_field` is a
`HashMap<String, Vec<&ClusterAlignment>>`, and Rust's default hasher is
randomized per-process, so two fields tied on `concept_count` could land
in either order depending on the run. Confirmed by diffing two
back-to-back real production exports before the fix (different field
order) and after (byte-identical, excluding the timestamp field).
Fixed with a deterministic `code`-ascending tiebreaker
(`export.rs::build_export`) and two new regression tests
(`fields_with_tied_concept_count_sort_deterministically_by_code`,
`ambiguous_clusters_are_listed_in_pending_clusters_not_silently_dropped`).
This directly undermines `docs/RELEASES.md`'s own stated design goal
("a release is a tagged, **reproducible** snapshot") — worth fixing
immediately once found, not deferring, even though it wasn't what this
pass set out to do.

**Found, not fixed, explicitly flagged**: the same regeneration also
touched `taxonomy.related.json` (the "related concepts" neighbor
export) — diffing it against the previously-committed version found
**17,292 of 77,623 rows (22%) differ across two runs of unchanged data**,
11,762 of them with a genuinely different neighbor *set*, not just
reordering (the underlying `scores` array is unchanged, so this is a
top-k neighbor selection instability, not a scoring bug — most likely
floating-point non-associativity in whatever parallel/ANN computation
`search.rs` uses, though the exact mechanism wasn't tracked down).

This is a **larger, more serious reproducibility gap** than the `fields`
bug — it affects a real user-facing feature (related-concept
suggestions) with a much bigger blast radius, and fixing it properly
means understanding `search.rs`'s neighbor-selection algorithm in
enough depth to make it deterministic, which is real, separate,
unbounded-feeling work well outside "implement the MSC
classification-status model." Rather than either quietly shipping an
unrelated, unverified 22%-different related-concepts dataset as a side
effect of this pass, or spending unbounded time chasing it down
mid-task, **reverted `taxonomy.related.json` back to its
previously-committed, already-verified state** (`git checkout --`) and
recorded this finding here for a dedicated future pass. Only
`taxonomy.json` (the file that actually needed to change for
`pending_clusters`) ships from this round.

## Verified

- `cargo test --all`: clean, including 6 new tests in
  `msc_classification.rs` (status round-trip, the 4 alignment→status
  mapping rules including the "grounded_count=1 with confidence 1.0 is
  still `pending`" edge case, and a real end-to-end test against a
  file-backed `TaxonomyStore`) and 2 new tests in `export.rs`.
- `classify-msc` run for real against production `scratch/
  provenance.db` + `scratch/papers_100k_fc.db`: `887 classified, 1334
  pending, 31862 unclassified (34083 clusters total)` — cross-checked
  against the real `taxonomy.json`'s own `fields`/`novelClusters`/
  `ambiguousClusterCount` (`887` = sum of every field's `clusterCount`,
  `1334` = `ambiguousClusterCount` exactly). Note `unclassified`
  (31,862) is larger than `novelClusters.length` (11,531) in the web
  export — the classification-status table intentionally includes
  singleton (`size == 1`) novel clusters that the web UI's own
  `novel_clusters`/`pending_clusters` filters exclude as "too thin to
  display"; the audit-grade table is deliberately more complete than
  what any one UI surface chooses to show.
- `scripts/regenerate-web-export.sh`: still passes clean after
  `classify-msc`'s additive write to production — confirms the new
  table doesn't interact with the existing release gate at all.
- `npm run build` + `npm run eval`: clean, MRR@10 0.9667 unchanged, 0
  contradictory cycles.
- Live in-browser (`javascript_tool`, both languages): the Pending tab
  renders 950 real clusters, first card matches the real data sampled
  directly from the regenerated `taxonomy.json`, the stats line shows
  "950/1334 pending", the hint text renders correctly, no console
  errors in either language.
- `git diff --stat web/public/`: only `taxonomy.json` changed (2
  insertions/deletions — the file is minified to one line), confirming
  no unintended side effects shipped beyond the intended new field and
  the (byte-verified-deterministic) field reordering.

## What's still open

- `unavailable`/`outside_scope`/`rejected` population (see above) —
  each has a distinct, real reason it's not built yet, not a single
  "finish this later" bucket.
- Item 10 (coverage metrics broken down by source and record type, not
  just DB-wide totals) — explicitly the next item in 改善点.txt's own
  ordering, not started.
- The `taxonomy.related.json` neighbor non-determinism found this pass
  — a real, separate reproducibility bug, larger in impact than the
  `fields` ordering bug that was fixed, deliberately left unfixed and
  unshipped rather than either ignored or rushed.
- No review workflow for MSC classification status specifically (the
  `review_status` column is retained per-row as item 9 asks, but
  nothing writes anything other than `"unreviewed"` yet).
- This pass's changes have not been pushed to `origin` or deployed —
  per the established pattern, that's a separate decision, not
  automatic just because local work is done and verified.

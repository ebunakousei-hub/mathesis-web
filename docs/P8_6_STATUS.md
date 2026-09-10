# Phase 8.6 status — a local raw-data index for the Math-Graph pilot

> Written 2026-09-10, at the user's explicit direction: fix the local
> pilot-indexing inefficiency (repeated full scans of the already-
> downloaded 1GB/89MB CSVs, repeated per-round JSON snapshots) before
> doing anything with the full 13.6GB upstream Math-Graph dataset. The
> full-dataset question stays deferred — unrelated to and unaffected by
> this work — per P8.5's Option B decision (provenance and classification
> semantics are still unresolved, and more indexing doesn't change that).

## The problem being fixed

Every pilot round since P7.4 (`scope_pilot_p7_4.py`, `_p8_1.py`, `_p8_4.py`)
opens `statement_formal.csv` (388,105 real rows — `wc -l` overcounts to
475,582 because several docstrings contain embedded newlines inside a
quoted CSV field) and `formal_dependency.csv` (11,335,708 rows, 1.05GB)
and streams both, start to finish, in Python, to scope out one project.
Each run also writes a fresh `pilot_statements_p8_N.json` /
`pilot_edges_p8_N.json` pair. Cost was growing linearly with both corpus
size and number of rounds, and nothing was indexed — the 4th project
onboarded (`pfr`, P8.4) paid the exact same full-scan cost as the 1st.

This is a different problem from "the full 13.6GB Hugging Face dataset is
large" — that dataset is untouched, stays untouched, and nothing here
changes the P8.5 publish/offline decision. Confirmed unchanged: the
Rust import path, `web/public/`, and the live deployed site (see
`docs/P8_5_STATUS.md` and the 2026-09-10 GitHub-sync update) are not
touched by this round at all — this is a scratch-directory ingestion tool
only, never committed to git and never read by the Rust crate or the web
app.

## What was built

`scratch/math_graph_pilot/`:

- **`pilot_index_lib.py`** — the schema (`SCHEMA_SQL`) and the two
  reusable query functions, `scope_project()` and `classify_project()`,
  that future rounds should call instead of writing a new
  `scope_pilot_p8_N.py`. One place, used by both the importer and every
  consumer.
- **`build_index_p8_6.py`** — the one-time/idempotent importer. Builds
  into a temp file, only replaces `index.sqlite` after a full validated
  build succeeds; a failed run never touches an existing good index.
  Skips rebuilding (unless `--force`) when the recorded input SHA-256
  hashes and importer/schema version already match.
- **`verify_index_p8_6.py`** — compares the new query path against the
  existing CSV-streaming ground truth for one fixed project (see below).
- **`bench_index_p8_6.py`** — timing and disk-usage measurement.
- **`index.sqlite`** (2.83GB, gitignored via the blanket `/scratch/` rule)
  — the canonical index. Three tables (`projects`, `statements`,
  `dependencies`), five indexes, one convenience view, plus a `meta`
  table recording schema/importer version, input file hashes+sizes, row
  counts, and validation findings.

### Schema, and two deliberate deviations from the directive's sketch

The directive's own conceptual schema was followed closely, with two
changes made for reasons found while implementing, not by default:

1. **`dependencies.role` was added.** The directive's sketch omitted it,
   but `crates/mathesis-provenance/src/math_graph_adapter.rs`'s
   `PilotEdge` struct has `pub role: Option<String>` and it is actually
   imported into the provenance store — dropping it from the index would
   have silently lost a field every downstream consumer needs. Checked
   the real Rust struct before deciding, not assumed.
2. **`target_name` is a view (`dependencies_resolved`), not a stored
   column.** `formal_dependency.csv` has no name field for the target at
   all — only `dep_id`. Materializing a resolved `decl_name` onto all
   11.3M rows would nearly double that table for a value already obtainable
   through an indexed join. A `LEFT JOIN` view gets the same convenience
   with no duplication — the kind of bloat this phase exists to remove.

`source_row_number` and `source_row_hash` are stored on every statement
and dependency row (a 16-hex-char fingerprint of the row's own fields),
so any later report can be traced back to the exact input CSV line —
directly serving the directive's stated goal of "a reusable foundation
for future provenance validation."

### Real findings from the import (not assumed, found by running it)

- **0 malformed rows, 0 duplicate `statement_id`s, 0 duplicate
  (repo_slug, module, decl_name) groups** across all 388,105 statements —
  the strict parser (which raises rather than silently coercing, unlike
  the old scripts) found nothing to reject.
- **7,315,754 of 11,335,708 dependency rows (64.5%) have a blank
  `via_proj`** — not `"True"`, not `"False"`, genuinely blank. The old
  scripts' `row["via_proj"] == "True"` coerced every one of these to
  `False` with no visibility that "blank" was ever a distinct case. The
  index stores it as SQL `NULL` and the query layer documents the
  `NULL → False` fallback explicitly at the one place it happens
  (`pilot_index_lib.scope_project`), rather than burying it in a
  string comparison. `via_proj` is not used by any classification rule
  today, so this doesn't change P8.5's conclusions — but it was invisible
  before and now isn't.
- **0 dependency edges reference a `statement_id` outside the full
  30-project index.** This is a different, broader check than each
  round's own "dependency targets outside this project's scope" count
  (P8.1–P8.5): those measure edges leaving one project's ~1–5k
  declarations for another of the 30; this measures edges leaving the
  entire 388,105-row corpus. The dependency graph, at the scope this CSV
  pair was already downloaded at, is fully closed — nothing points
  further outside than that.

## Verification: old path vs. new path, on `pfr`

Per the directive's item 4, the old CSV-streaming parser and the new
SQLite-backed query were run side by side on one fixed project — `pfr`
(P8.4's addition, the freshest ground truth) — and compared before the
new path is used for anything going forward.

"Byte-for-byte" is interpreted here as **exact content equality**, not
identical physical row ordering: the old scripts' order is incidental
CSV row order (an accident of the upstream file, not a meaningful
property), while the index returns canonically sorted output
(`ORDER BY statement_id`, etc.). Both sides were normalized to sorted,
hashable tuples before comparing, so the check is robust to ordering
either way and actually verifies the property that matters — no
declaration or edge gained, lost, or altered.

```
=== Scope check: pfr ===
  statements: old=1073 new=1073 missing_from_new=0 extra_in_new=0 -> OK
  edges: old=66996 new=66996 missing_from_new=0 extra_in_new=0 -> OK

=== Classify check: pfr (expected dir 'PFR') ===
  safe statements: old=43 new=43 missing_from_new=0 extra_in_new=0 -> OK
  safe edges: old=25 new=25 missing_from_new=0 extra_in_new=0 -> OK

=== ALL CHECKS PASSED ===
```

`Mathlib_v429` is deliberately **not** covered by this query path.
Its literal/candidate split comes from a cross-reference against
Mathesis's own independently-extracted Mathlib corpus (P6.2/P7) — a
different data source entirely, not derivable from `statement_formal.csv`
/ `formal_dependency.csv`. Any future round must keep carrying that
split forward from `pilot_statements_p7_4.json` unchanged, exactly as
`classify_pilot_p8_5.py` already does; `pilot_index_lib.classify_project()`
is documented as covering only the 4 non-Mathlib projects.

## Benchmark

| | old (CSV streaming) | new (indexed) |
|---|---|---|
| `scope('pfr')` | 41.79s | 0.29–0.33s (3 runs) |
| speedup | | **~128x** |

| | bytes |
|---|---|
| `index.sqlite` | 2,827,468,800 (2.83GB) |
| existing per-round JSON snapshots (16 files, kept as verification ground truth) | 112,082,784 (112MB) |
| raw source CSVs (unchanged) | 1,143,507,159 (1.14GB) |

The honest tradeoff: the index is **larger on disk** than the raw CSVs it
was built from (2.83GB vs. 1.14GB) — mainly the per-row hash and four
indexes over 11.3M dependency rows. 284GB was free on this machine before
the build; this was judged an acceptable trade for a ~128x query speedup
and the elimination of repeated full-corpus scans, not free. If disk ever
becomes a real constraint, the per-row `source_row_hash` column is the
first thing to drop (traceability could fall back to `source_row_number`
+ a whole-file hash instead of a per-row one).

## What changes for future rounds

A future project addition (a hypothetical "P8.7") should call
`pilot_index_lib.classify_project(conn, repo_slug, expected_top_dir)`
directly against `index.sqlite` and write only a small scope/classify
report — **not** a new `scope_pilot_p8_N.py` that re-streams the raw
CSVs, and not a new multi-MB `pilot_statements_p8_N.json` snapshot. The
existing P8.1/P8.4/P8.5 snapshot files are kept as-is (they're this
round's own verification ground truth, and regenerable, not the kind of
stale "backup" file the user separately had removed from the git repo
this session) — nothing here retroactively rewrites prior rounds' output.

If the raw CSVs are ever re-downloaded or changed, `build_index_p8_6.py`
detects the SHA-256 mismatch against the recorded `meta` values and
rebuilds automatically; it never silently serves a stale index next to
changed inputs.

## What this does not do

- Does not touch the full 13.6GB upstream Math-Graph dataset in any way.
- Does not change the P8.5 classification semantics, the
  `project_attribution_unresolved` filtering, or any Rust code.
- Does not touch production `scratch/provenance.db`, any P8 pilot
  `provenance.db`, or `web/public/`.
- Is not committed to git (`/scratch/` is gitignored wholesale) and is
  not part of the browser build — an ingestion-analysis artifact only.
- Does not resolve or attempt to resolve the provenance/semantic
  uncertainty P8.5 found. That decision (pilot stays offline) is
  unaffected and unrevisited here.

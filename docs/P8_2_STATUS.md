# Phase 8.2 status — connecting the offline connector's derived index to the Chio Panel

> Written 2026-09-10, executing Stage 3 (P8.2) of the user's staged
> TheoremGraph-connector directive, immediately after P8.1's offline
> pilot artifact. Stages 4–6 (P8.3 graph UI, P8.4 project-by-project
> expansion, live-API integration) are **not** started — the directive
> itself stages them as later gates.

## What "the Chio Panel" is, in this codebase

The directive's architecture sketch (`MSC2020 → ... → Server-side
search/index service → Chio Panel`) names a UI component that doesn't
exist under that name in Mathesis — this codebase's MSC-organized
concept browser is `DynamicTaxonomyExplorer`
(`web/src/dynamicTaxonomy.ts`), so that is what "the Chio Panel" is
taken to mean here.

## The real constraint this stage ran into: MSC alignment never touches Lean declarations

The directive's Stage 3 item list (MSC ancestor lookup, All items /
Unclassified / unavailable, paginated results, source badges, "not
independently verified" labels, coverage metrics) assumes a working
`TheoremGraph record → MSC2020 category` mapping exists to browse by.
It doesn't: PA.3 (`docs/PA_3_STATUS.md`) established that MSC alignment
(`mathesis-taxonomy::alignment`) only ever runs on arXiv concept
clusters, never on Lean declarations, and P8.1's own manifest
(`docs/P8_1_STATUS.md`) already reported all 723 pilot declarations as
`unavailable` for exactly this reason. Building a live "browse pilot
declarations by MSC ancestor" filter would therefore be dead code: a
control that can never return a non-empty result, for data that
structurally cannot carry an MSC code yet. That was **not** built.

Instead, the "All items / Unclassified / unavailable" requirement is
answered honestly: every external declaration in the derived index is
reported as `unavailable`, with an explanation, via a single
`mscClassificationNote` string, not a set of filter buttons over states
that can't occur.

## What was built: extending the existing, already-isolated discovery panel

P7.4 already built exactly the "small server-side/read-model prototype
→ browser" pipeline the directive describes, deliberately isolated from
the default trust graph, search index, and lineage view
(`mathGraphDiscovery.ts`'s own doc comment). It already had source
badges and "not independently verified by Mathesis" labels. P8.2
extends it — in both the Rust export and the two already-committed
production JSON files it reads — rather than building a second,
parallel mechanism inside the MSC panel; merging the two would have
undone P7.4's explicit isolation rationale for no functional gain. A
short, honest cross-link was added to the MSC panel instead (see
below).

### Rust (`crates/mathesis-provenance/src/discovery_export.rs`)

`DiscoveryExport`/`DiscoveryEdge` (already reused unchanged by P8.1 as
its "read-model prototype") gained three fields, all computed from data
that was already in the DB — nothing invented:

- `DiscoveryEdge.sourceProject: string | null` — the external edge's
  subject-declaration `repoSlug`, resolved by walking
  subject entity → its `SourceRecord` → `reproducibility_json`
  (the field `math_graph_adapter::import_statement` already writes).
  `null` for Mathesis's own edges, which have no project concept.
- `DiscoveryExport.byProject: {repoSlug, literalCount, hierarchyCount}[]`
  — the "coverage metrics" ask, per source Lean project. Sorted by
  total count descending, then `repoSlug` ascending — the same explicit
  tiebreak PA.3 added to `export.rs::fields` after finding a real
  non-determinism bug from relying on `HashMap` iteration order; this
  export walks a `BTreeMap` into a `Vec` the same way, so it's
  deterministic by construction rather than by accident.
- `DiscoveryExport.mscClassificationNote: string` — one sentence stating
  the classification-status honestly (see above); empty string when an
  export has no external edges at all.

Two new unit tests in `discovery_export.rs` build a synthetic 2-project,
5-declaration, 3-edge DB via the real `import_pilot` adapter and assert:
`sourceProject` resolves correctly per edge, `byProject` sorts by count
with the right tiebreak, and the note text is present/absent correctly.

The `export-discovery` CLI command's flags are unchanged — only its
output shape grew.

### Web (`web/src/mathGraphDiscovery.ts`, `types.ts`, `main.ts`, `style.css`)

- `DiscoveryEdge`/`DiscoveryExport` TypeScript interfaces extended to
  match. `byProject`/`mscClassificationNote` are read with `?.` so an
  export generated before P8.2 (missing these keys entirely) degrades
  to "no breakdown shown," not a crash — verified live (below).
- `renderProject` now shows the per-project coverage chips and the MSC
  note, then groups the external edge list by `sourceProject` (a group
  with an empty/unresolved slug renders unlabeled, for old exports)
  instead of one flat list.
- **Pagination** ("paginated TheoremGraph results", the directive's own
  acceptance criterion "long-running graph expansions are bounded"):
  each project group now shows 50 edges at a time with a "Show more"
  button, instead of rendering all of them unconditionally. Necessary
  in practice — P8.1's combined pilot export has 775 edges for FLT
  alone.
- `main.ts` now passes a third source path,
  `math-graph-discovery-p8-1-pilot.json`, alongside the two existing
  P7.4 project files.
- A short, honest cross-link (`i18n.ts` key `theoremGraphCrossLink`,
  bilingual) was added to `dynamicTaxonomy.ts`'s scope-hint area,
  pointing users from the MSC panel to the discovery panel for
  TheoremGraph/Math-Graph content — the connection the directive asked
  for, made as a real link between two panels that have good reasons to
  stay architecturally separate, not a UI merge.

## A real bug found and fixed while wiring in the third source

`MathGraphDiscoveryPanel.load()` fetched all source paths with
`Promise.all`, but treated *any* single non-ok response as reason to
set `loadError` and abandon the whole load — discarding projects that
*had* loaded successfully. This had never been observed because both
of P7.4's source files have always existed together; adding a third,
optional (pilot, not committed) path was the first real trigger. Fixed
so each path is fetched and parsed independently, with a per-path
failure (bad status *or* a JSON parse error — Vite's dev server returns
this: a missing `public/` file 200s to `index.html`, not a real 404,
which crashed the old code and would have crashed a naive per-path fix
too) reported and skipped, not fatal to the others. Confirmed live: with
the pilot file removed, console shows one reported failure and the two
existing projects still render; with it restored, all three render with
zero console errors.

## The pilot data is real, verified, and deliberately still not shipped to production

`scripts` used: rebuilt `mathesis-provenance --release`, ran
`export-discovery` against the real `scratch/p8_1/pilot_provenance.db`
(P8.1's isolated DB — production `scratch/provenance.db` was opened
read-only, once, only to regenerate the two pre-existing project2/3
JSON files, whose original release tag no longer resolves against the
current single `v0-baseline-20260905` release — that regeneration was
abandoned as out of scope for this pass, see below).

Regenerated `scratch/p8_1/pilot_read_model.json`: 870 edges, all 870
resolve a `sourceProject` (0 unresolved), `byProject` shows real,
distinct counts per repo (`FLT: 775`, `Mathlib_v429: 48`, `carleson:
39`, `PrimeNumberTheoremAnd: 8`), `mscClassificationNote` reads
correctly. Verified rendering live in the browser (dev server,
`web/public/math-graph-discovery-p8-1-pilot.json` copied in
temporarily): coverage chips, the MSC note, per-project grouping,
pagination ("Show more (50 of 775)" → "(100 of 775)" on click), source
badges, and "not independently verified by Mathesis" text all present
and correct, with zero console errors.

**That copy was removed after verification and was never committed.**
`git status` is clean of it. This matches P8.1's own explicit framing
("isolated pilot artifact... promote it to a public search source only
after P8.2 validation") — P8.2 validates that the connection *works*;
whether to actually publish this specific pilot's data to the live
`research-preview-2026-09-09` site is a publish decision reserved for
the user, not something this pass took unilaterally. To regenerate and
preview it locally:

```
cargo build --release -p mathesis-provenance
target/release/mathesis-provenance export-discovery \
  --db scratch/p8_1/pilot_provenance.db --release p8-1-pilot-20260909 \
  --project-label "Math-Graph pilot (5 projects)" \
  --out web/public/math-graph-discovery-p8-1-pilot.json
```

## What was deliberately not touched

- `web/public/math-graph-discovery-project2.json` /
  `-project3.json` — left byte-identical to before this pass. Their
  original release tag (`p7.2-project2`/`-project3`) doesn't exist in
  the current production `scratch/provenance.db` (which now has one
  consolidated release, `v0-baseline-20260905`), so regenerating them
  to carry the new `byProject`/`mscClassificationNote` fields would
  have required figuring out how to re-derive that historical scoping —
  a real but separate task, out of scope for "connect P8.1's artifact."
  The frontend's `?.` guards mean this is a graceful gap, not a bug:
  confirmed live that these two files render exactly as they did
  before, with no coverage chips (since they don't have the data),
  no error.
- No MSC-driven filtering UI for pilot declarations — see "the real
  constraint" above.
- No graph-UI (lineage view) integration — that's P8.3, explicitly
  staged after this one.
- Production `scratch/provenance.db` was opened read-only exactly once
  (the abandoned project2/3 regeneration attempt); nothing was written
  to it.

## Verified

- `cargo build -p mathesis-provenance` and `cargo test -p
  mathesis-provenance discovery_export`: clean, including the 2 new
  unit tests.
- `cargo test --all`: clean (all workspace crates).
- `cd web && npx tsc -b`: clean, both before and after the `load()` fix.
- Live browser verification (Vite dev server) as described above:
  rendering with and without the pilot file present, pagination,
  grouping, coverage chips, MSC note, zero console errors in either
  state.

# Phase 7 status — a scoped, offline Math-Graph pilot

> Written 2026-09-08, executing the user's P7 instruction: import Math-Graph
> (`uw-math-ai/math-graph`, CC BY 4.0) as a bounded offline dataset
> integration — never the live `theoremsearch.com` API — with a scoped
> pilot subset, graph structure before text, conservative predicate
> mapping, a dedicated release gate, and a real comparison against P6's
> own DeGiorgi/Mathlib results. This is a pilot in an **isolated** database
> (`scratch/math_graph_pilot/pilot_provenance.db`), not production
> `scratch/provenance.db` — matching P6.2's precedent for exploratory
> work the user has not yet asked to ship.

## Licensing: what was actually verified, and where

The user's own message already settled the licensing question from public
sources; this pass re-verified it against primary sources before writing
any code, and found one thing worth flagging:

- **`uw-math-ai/math-graph` (the dataset)**: confirmed via HuggingFace's
  own dataset API (`/api/datasets/uw-math-ai/math-graph`, not a scraped
  page) — `cardData.license: "cc-by-4.0"`. Permits commercial and
  non-commercial use, redistribution, and adaptation; attribution only,
  no share-alike. This is the dataset actually behind the TheoremGraph
  paper (arXiv:2606.25363) and its two "graphs" (informal, from arXiv
  theorem environments; **LeanGraph**, elaborator-level dependency
  extraction across 25 Lean 4 projects).
- **`github.com/uw-math-ai/TheoremSearch`** (the code behind the live
  `theoremsearch.com`/`api.theoremsearch.com` service): GitHub's repo API
  reports `"license": null`. No terms of service, rate limits, or
  third-party-integration policy published anywhere on the site. **Not
  touched** — confirms the user's own read, and this pilot never makes a
  network call to that service.
- **New finding, not in the user's summary**: `paper_lean_repo.csv` (the
  per-Lean-project metadata table) has **no license column at all** —
  `paper_id,kind,source,title,authors,url,categories,updated_at,repo_slug,
  repo_url,lean_toolchain,mathlib_rev,git_commit`, verified against the
  real downloaded file, not the dataset card's prose. The dataset's own
  CC BY 4.0 covers the *graph-structure compilation* Math-Graph publishes
  (the fact "declaration X depends on declaration Y"); it says nothing
  about the license of the 25 underlying Lean repositories' actual source
  text. This pilot never imports `body`/`proof`/`docstring` text for
  exactly this reason — sidestepping a licensing question the raw data
  doesn't answer, rather than assuming Mathlib's well-known Apache-2.0
  extends to every one of the 25 projects without checking each one.

## What was downloaded, and what deliberately was not

Math-Graph is 8 CSV tables, ~13.6 GB, 16.1M rows. This pilot downloaded
only the two LeanGraph tables plus tiny per-project metadata — **1.14 GB
total**, `scratch/math_graph_pilot/` (git-ignored, never committed):

| File | Size | Used for |
| --- | --- | --- |
| `formal_dependency.csv` | 1.05 GB (11,335,708 rows) | LeanGraph's typed dependency edges |
| `statement_formal.csv` | 89.5 MB (388,105 formal statements) | Lean declaration identity (name/module/file_path) |
| `paper_lean_repo.csv` | 3.8 KB | Per-project toolchain/mathlib_rev/git_commit |
| `paper_lean_community.csv` | 8.3 KB | Blueprint metadata (unused this pass) |

Both large files' local SHA-256 were checked against HuggingFace's own
reported LFS content hash and matched exactly (`69fc8539…`,
`bf326614…`) — the download is byte-verified, not assumed intact.

**Not downloaded, at all**: `paper_arxiv.csv` (4.0 GB), `statement_informal.
csv` (5.0 GB), `informal_dependency.csv` (2.4 GB), `slogan.csv` (1.05 GB)
— the entire informal/arXiv side. The user's own instruction ("import
graph structure before theorem text") plus this pilot's comparison target
(P6's own Lean work) made LeanGraph the only relevant slice; the informal
side is out of scope for this pass, not merely deferred silently.

## Scoping: the same two namespaces as P6.2, on purpose

`scratch/math_graph_pilot/scope_pilot.py` filters the two LeanGraph
tables down to declarations under `Mathlib.Algebra.Order.Group` and
`Mathlib.CategoryTheory.Category` — **the identical two namespaces
P6.2's pilots 2 and 3 already extracted**, in the closest available
Mathlib toolchain bucket (`Mathlib_v429`, `lean_toolchain: v4.2.9` — the
project's own metadata has no `mathlib_rev`/`git_commit` for this
bucket, left `null` rather than guessed). Deciding what to compare
against *before* importing, not after, is the same discipline P6.2 used
to pick its two Mathlib subtrees.

Real, measured result of the scoping pass:

| | Value |
| --- | --- |
| Formal statements scanned | 388,105 |
| Statements in pilot scope | 63 |
| Dependency edges scanned | 11,335,708 |
| Edges with `src_id` in scope | 290 |
| Self-loops dropped (`src_id == dep_id`) | 9 |
| Distinct dependency targets | 110 |
| ...outside pilot scope | 84 |

The self-loops are a real data artifact, found by inspecting actual rows
(not assumed): 9 of 299 raw edges for in-scope declarations are literal
`src_id,src_id,def,,,,,` rows — dropped as noise, the same instinct as
P6.1's generated-declaration filtering, though this is a different
mechanism (Math-Graph's own edge table, not Mathesis's own extractor).

## Conservative mapping: three deliberate, documented choices

**1. `epistemic_state: extracted`, not `observed`.** By
`docs/DATA_DICTIONARY.md` §4.2's literal text, "observed" is reserved for
"a real Lean-exported dependency" — an elaborator-verified fact — and
LeanGraph genuinely is elaborator-level extraction, so the textbook
reading would say `observed`. This pilot deliberately does not use that
reading: `observed` means *elaborator-verified*, and Mathesis has
independently reproduced and audited its own Lean extraction (pinned
toolchain/mathlib revision, tested filtering policy, byte-stability
proven twice over in P6.1/P6.2) — none of which is true yet for
Math-Graph's own pipeline from Mathesis's side. Per the user's explicit
instruction ("prevent external edges from becoming `default_traversal`
merely because the dataset calls them dependencies"), `extracted` was
chosen because it is the **minimal-footprint way to enforce that without
touching `relation_policy::traversal_policy`** (a small, heavily-tested,
shared function) — `depends_on` + `extracted` resolves to
`visible_only`, automatically, with no new gate code. Verified directly:
[`math_graph_adapter.rs`](../crates/mathesis-provenance/src/math_graph_adapter.rs)'s
own test asserts `traversal_policy(...) == VisibleOnly` for the
imported real data.

**2. `evidence_kind: formal_export` is kept**, despite the epistemic-state
downgrade — `docs/DATA_DICTIONARY.md`'s own "evidence multiplicity, not
epistemic-state inflation" principle treats evidence kind (what check
happened) and epistemic state (how much to trust it) as independent
axes. Calling this `source_span` or `model_output` would misdescribe a
genuinely elaborator-derived edge; `formal_export` is accurate, `extracted`
is the trust policy.

**3. `dependency_origin` carries the raw `edge_type`** (`sig`/`proof`/
`def`/`extends`/`field`/`docref`), not a forced fit into Mathesis's own
`type`/`body`/`both` vocabulary. `dependency_origin` is free text on the
`Evidence` row specifically so a second source's own, more granular
typing doesn't have to be rounded off to fit — P6.2's own lesson ("don't
round without measuring") applied to a schema decision this time, not an
extraction bug.

## `subject_ref`/`object_ref`: `judgment:mathgraph:<uuid>`, and its one real consequence

Math-Graph declarations are registered as `EntityKind::Judgment` (the
structurally honest fit — they are declarations, like Mathesis's own
judgments) under a distinct ref namespace, `judgment:mathgraph:<statement_
id>`, which cannot collide with Mathesis's own `judgment:<numeric-id>`
refs. This satisfies `relation_policy::valid_entity_kinds` and the
entity-catalog FK machinery normally (102/102 subject/object refs
resolved in the real pilot run).

**One consequence, found by reading the code, not guessed**:
`entity.rs::judgment_id_for_entity` — used by `web_export::
build_dependency_edges`/`build_morphism_edges` to turn a `subject_entity_id`
back into the *numeric* judgment id `dependencies.json` needs — parses
everything after `"judgment:"` as an `i64`. `"mathgraph:<uuid>"` fails
that parse, so `judgment_id_for_entity` returns `None`, and
`build_dependency_edges`'s existing `let Some(from) = ... else { continue
}` guard silently excludes these assertions from `dependencies.json`.
**This is an intentional pilot-scope boundary, not an undiscovered bug**:
making Math-Graph declarations show up in the lineage view would require
either giving them a real row in `mathesis-graph`'s own `judgments` table
(cataloging an external dataset as if it were a first-class Mathesis
judgment) or a fourth `EntityKind` — both real design decisions, neither
needed to answer "can this dataset be imported and compared," which is
what this pilot was scoped to answer. Recorded here so it isn't
rediscovered as a mystery later.

## Real import, measured

Against the isolated pilot database (schema created fresh, one release
row `math-graph-pilot` inserted directly — no `import-legacy` run, so no
Mathesis production data is anywhere in this database):

```
$ mathesis-provenance import-math-graph --db pilot_provenance.db \
    --release math-graph-pilot \
    --statements pilot_statements.json --edges pilot_edges.json \
    --dataset-revision ced4ca9de1bd9e5b67aa09d1d515e270e438fa1e

Math-Graph pilot: declarations +63 (skip 0), dependencies +51 (skip 0),
239 dependency targets outside the pilot scope (ignored)
real 0m0.194s
```

| Measurement | Value |
| --- | --- |
| Declarations imported | 63 |
| Dependency edges imported (both endpoints in scope) | 51 |
| Edges pointing outside pilot scope (correctly excluded, not fabricated) | 239 |
| Import wall-clock time | 0.19s |
| Rerun (idempotency check): new rows | 0 declarations, 0 edges — 63/51 correctly detected as existing |
| Duplicate rate on rerun | 0% new, 100% correctly recognized |
| Final `pilot_provenance.db` size | 200,704 bytes (196 KB) |
| Entity reference coverage | 102/102 (100%) |
| `epistemic_state` breakdown | 51 `extracted`, 0 `observed` — confirms design intent |

51 + 239 = 290 (matches the scoping pass's edge count exactly — no rows
lost silently between the Python filter and the Rust import).

## Comparison against P6.2 (item 5): overlap, not superiority

Compared Math-Graph's LeanGraph slice against Mathesis's own P6.2 pilot
manifests (`scratch/p6_2_project2_manifest.json`,
`scratch/p6_2_project3_manifest.json`) — same two Mathlib namespaces,
same underlying source tree, two independent extraction pipelines.

| | Math-Graph (this pilot) | Mathesis P6.2 |
| --- | --- | --- |
| Target declarations, both namespaces | 63 | 671 (601 project 2 + 70 project 3, P6.2's own "target declarations" count) |
| Declarations restricted to the 5 `.lean` files Math-Graph actually touched | 63 | 361 |
| Declaration-name overlap (exact match, either graph's naming) | **1** (`LibraryNote.universe_output_parameters_and_typeclass_caching` — a documentation library-note artifact, not a theorem) | — |
| Math-Graph-only (of the 63) | 62 | — |
| Mathesis-only (of the 361, same files) | 360 | — |
| Declaration `kind` mix (Math-Graph) | 53 `inst`, 6 `theorem`, 2 `def`, 2 `instance` (87% instances) | predominantly theorems/lemmas + a smaller instance share |
| Shared `.lean` files (module-level) | `Mathlib.Algebra.Order.Group.{Synonym,Action.Synonym}`, `Mathlib.CategoryTheory.Category.{Basic,Factorisation,RelCat}` | same 2 of these 5 files entered directly (`.Synonym` via transitive import, `.Basic` as an entry point) |
| Dependency edges (in-scope) | 51 | 59 (project 2) + 9 (project 3) checker-derived |
| Unresolved/external dependency targets | 84/110 (76%) | 446/601 (74%, project 2), 52/70 (74%, project 3) — comparable order of magnitude |

**The near-zero name overlap is real, not a bug**: both pipelines really
did extract from the identical Mathlib source files (confirmed:
5 shared modules), and still picked almost entirely different
declarations. The most likely mechanism, inferred from the data but not
independently confirmed by re-running either pipeline's internals: Math-
Graph's LeanGraph appears to select declarations by literal **file-path
membership** under the namespace (whatever `.lean` files their crawl found
under `Mathlib/Algebra/Order/Group/` and `Mathlib/CategoryTheory/Category/`
at their snapshot's revision), heavily weighted toward auto-derived
`instXxx` typeclass instances in those files. Mathesis's P6.2 pipeline
selects by **transitive-import reachability from one entry file**
(`import Mathlib.Algebra.Order.Group.Basic` / `...Category.Basic`) with a
name-prefix filter and an explicit generated/private-declaration
denylist — reaching `Unbundled.Basic` and other files Math-Graph's slice
never touches, while filtering out many of the `inst`-kind declarations
Math-Graph keeps. Neither is "more correct" — they answer different
questions ("what's in these files" vs. "what does this entry point
transitively pull in, filtered"), and this pilot's job was to measure
that difference, not adjudicate it. A definitive mechanism explanation
would require reading Math-Graph's own extraction code, which is out of
scope here.

**External-target profile** (the 84 unresolved Math-Graph dependency
targets, resolved by name/module for this report only, not imported):
concentrated in `Init.Prelude` (20, Lean core), `Mathlib.Algebra.Group.
Action.Defs` (6), `Mathlib.Algebra.Order.Group.Synonym` itself (6, i.e.
some in-scope declarations depend on other in-scope declarations that
happened to land in the "external" bucket due to the src/dep asymmetry
of a single-pass scope filter — a real, minor scoping artifact, not
double-counted in the DB import since only src-in-scope edges are ever
written), plus `Mathlib.Order.OrderDual`, `Mathlib.Combinatorics.Quiver.
Basic` — all structurally sensible (`OrderDual` instances depending on
`Mathlib.Order.OrderDual`; `Category` depending on `Quiver.Basic`).

## What this pilot deliberately does not do

- Does not touch production `scratch/provenance.db` — everything lives
  in `scratch/math_graph_pilot/pilot_provenance.db`, an isolated,
  disposable database.
- Does not import any theorem/proof/docstring text — declaration
  identity and typed dependency edges only.
- Does not import the informal (arXiv) side of Math-Graph at all.
- Does not make the imported edges appear in `dependencies.json` or any
  release's default traversal — both by `epistemic_state: extracted`
  (policy) and by the `judgment:mathgraph:` ref namespace not resolving
  through `judgment_id_for_entity` (structural), independently.
- Does not run `verify-release` against this database — there is no
  manifest/sidecar pipeline for an isolated pilot DB with no
  `import-legacy` history; `mathesis-provenance stats` and the adapter's
  own tests are this pass's verification, not the production release
  gate.
- Does not decide whether Math-Graph should become a real, ongoing
  `SourceAdapter` alongside OpenAlex/MSC. That is a further scope
  decision (full 25-project import, informal side, live-API question if
  the department reply changes anything) left for explicit instruction,
  matching how every prior TheoremGraph/math-graph mention in this
  project's history has been handled.

## Verified

- `cargo test -p mathesis-provenance` and `cargo test --all`: clean,
  including 4 new `math_graph_adapter` tests (extracted-not-observed,
  outside-scope edges counted not fabricated, self-loop handling
  documented, idempotent rerun).
- Real pilot import against real, byte-verified downloaded data: 63
  declarations, 51 dependency edges, 0% spurious duplicates on rerun,
  100% entity reference coverage, 196 KB database.
- License chain verified against primary sources (HuggingFace dataset
  API, GitHub repo API), not paraphrased summaries — one gap found
  (`paper_lean_repo.csv` has no license column) and designed around
  rather than papered over.

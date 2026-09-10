# Phase 8.4 status — expanding the pilot project by project

> Written 2026-09-10, executing Stage 5 (P8.4) of the user's staged
> TheoremGraph-connector directive, immediately after P8.3's graph UI
> connection. Stage 6 (live-API integration) is **not** started — it
> remains explicitly gated on published API terms, written maintainer
> permission, or an institutional agreement, none of which exist.

## What Stage 5 asked for

> After the pilot demonstrates [useful results, acceptable storage,
> acceptable query latency, valid licensing, stable source mappings,
> understandable graph presentation], expand to additional Math-Graph
> projects or shards. **Each project should be independently importable
> and removable. Do not make one enormous irreversible import.**

Two distinct deliverables follow from this: (1) actually add one more
real project, and (2) make removal a real, working capability — not
just "importing incrementally already happens to work," which was true
before this pass, but "you can retract exactly one project's data
without touching anything else," which was not.

## Part 1: `remove-math-graph-project` — the capability this repo didn't have yet

Checked before writing anything: `ProvenanceStore` had **zero DELETE
methods** anywhere in the codebase. Every phase since P0 only ever
added data (idempotently). That's not an oversight — no prior phase
needed to retract an import. P8.4 is the first one that does.

Added, following the codebase's existing "one file per table" CRUD
convention rather than one big raw-SQL blob:

- `evidence.rs::delete_evidence_for_assertion`
- `review.rs::delete_review_decisions_for_assertion` (review is
  documented elsewhere as an "append-only log," but that's true only
  while the assertion it reviews still exists — retracting the
  assertion itself requires clearing its review rows first, or the
  `DELETE` on `relation_assertions` fails against the FK. Math-Graph
  pilot assertions have never been reviewed in practice, but the method
  is implemented to work correctly even if that ever changes, not
  written to only work by accident.)
- `assertion.rs::assertion_ids_touching_entity` / `delete_assertion`
- `entity.rs::entity_ids_with_ref_prefix` / `delete_entity_refs_for_entity` / `delete_entity`
- `source_record.rs::delete_source_record`

These compose into **`retract.rs::retract_entity`** — a new, generic
primitive (not Math-Graph-specific) that removes one entity and every
row that depends on it, in FK-safe order: evidence → review_decisions →
relation_assertions → entity_refs → entities → source_records.
Deliberately does *not* re-verify that a source_record is safe to
delete before trying — `PRAGMA foreign_keys = ON` (already set at
`ProvenanceStore::open`) will refuse the deletion and return an error
if some other surviving row still needs it, which is a correct, safe
failure mode for a primitive meant to be reused by future adapters,
not just this one.

**`math_graph_adapter.rs::remove_project(prov, repo_slug)`** is the
Math-Graph-specific layer on top: finds every entity whose ref starts
with `judgment:mathgraph:` and whose source record's
`reproducibility_json.repoSlug` matches, then calls `retract_entity`
for each inside one transaction (the CLI wrapper does the
`prov.transaction(...)`, matching how `import_pilot` is invoked
elsewhere in this codebase). Safe to scope this narrowly because
Math-Graph edges are always intra-project by construction
(`scope_pilot_*.py` only keeps edges whose `src_id` is in that
project's own statement set) — removing one project's entities can
never partially orphan another project's edges.

New CLI command: `mathesis-provenance remove-math-graph-project --db <path> --repo-slug <slug>`.

### Tests (9 new, all passing)

`retract.rs` (2): retracting an entity removes exactly its own
assertions/evidence/refs/source_record and leaves an unrelated entity
completely untouched; retracting an entity with no assertions still
cleans up correctly (no accidental early-return on the empty case).

`math_graph_adapter.rs` (3, the ones that matter most for this
directive's actual requirement): **removing project A leaves project
B's entity/assertion counts and refs completely unchanged**; removing
an unknown repo_slug is a safe no-op (0 removed, nothing touched); and
**a removed project can be re-imported to reach byte-identical
counts** — proving "removable" actually composes with "importable"
instead of leaving the DB in some different state after a round trip.

A test-helper bug was caught while writing these: this file's existing
`stmt()` test helper hardcodes `repo_slug: "Mathlib_v429"` (correct for
every prior test, which never varied it) — my first version of the new
tests passed a project name as an argument that silently landed in
`module` instead, and `remove_project` correctly found 0 matching
declarations, failing the test as it should have. Fixed by adding a
dedicated `stmt_in_project` helper rather than changing `stmt`'s
behavior for its existing 6 callers.

## Part 2: the actual expansion — `pfr`

Selected from the same already-downloaded, already-hash-verified
`statement_formal.csv`/`formal_dependency.csv` (re-verified fresh this
pass — `sha256sum` matched P8.1's recorded values exactly, byte for
byte). Checked declaration counts for ~20 unimported candidates in
`paper_lean_repo.csv` before choosing: **pfr** (the Polynomial
Freiman-Ruzsa conjecture, Terence Tao et al.) — 1,073 declarations,
moderate size (smaller than FLT's 2,368), and topically distinct from
the 4 existing projects (additive combinatorics vs. number
theory/harmonic analysis/analytic number theory) — a deliberately
*small* single addition, matching "Do not make one enormous
irreversible import" literally, not just in spirit.

Same scoping/classification pipeline as P8.1 (`scope_pilot_p8_4.py`,
`classify_pilot_p8_4.py` — scratch-only, not committed, same as every
prior Math-Graph Python script). Same structural limitation, stated
again rather than silently reused: `literal` is not computable for pfr
either (Mathesis has never extracted it), so its 43 safe declarations
are all `typeclass_hierarchy`. Real numbers: 1,073 declarations
scanned → 43 typeclass-hierarchy (1,030 excluded) → 25 safe dependency
edges (out of 66,996 scanned for the project, 49,066 excluded as
proof-type, 17,905 excluded for having an endpoint outside the safe
set — consistent in shape with every prior non-Mathlib project's
numbers).

> **Update (`docs/P8_5_STATUS.md`)**: a spot-check against pfr's real
> GitHub source found a counterexample here too —
> `PFR/Mathlib/Probability/Kernel/Composition/Comp.lean`'s
> `IsMarkovKernel (deleteRight κ)` instance has a real tactic proof
> (`by rw [...]; apply ... (by fun_prop)`), not a trivial delegation,
> despite being classified `typeclass_hierarchy`. See that document —
> the rule is real but not a content-free guarantee.

## Verified end-to-end, on the real pilot DB, not just synthetic tests

1. Imported pfr into `scratch/p8_1/pilot_provenance.db` (same release,
   `p8-1-pilot-20260909`): `declarations +43 (skip 0), dependencies +25
   (skip 0)`. Entity count 723 → 766, assertion count 870 → 895 —
   exactly as expected.
2. Ran `remove-math-graph-project --repo-slug pfr` against the **real**
   DB (not a fixture): reported `43 declarations, 25 dependencies`
   removed. Entity/assertion/source_record counts dropped back to
   exactly 723/870/724.
3. **Compared the post-removal DB's entire `entities` table row-for-row
   against a full pre-expansion backup** — byte-identical. Not just
   "counts match" — the actual rows are unchanged.
4. Re-imported pfr: counts returned to exactly 766/895/767, matching
   step 1 precisely — a full, verified import → remove → verify-clean →
   reimport → verify-identical cycle on real data.
5. Regenerated the manifest (`export-pilot-manifest`) and read model
   (`export-discovery`) against the expanded 5-project DB; confirmed
   `declarationEntityCountInDb` (766) still matches
   `mscClassificationCounts.unavailable` (766) — the P8.1 bug-fix
   invariant still holds after expansion.
6. Loaded the regenerated read model into a live dev-server browser
   session: the P8.2 discovery panel's `byProject` breakdown correctly
   shows all 5 projects including `pfr: 25`; toggling external results
   shows a `pfr (25)` edge group; clicking a pfr declaration
   (`FiniteRange.mul' (PFR.ForMathlib.FiniteRange.Defs)`) correctly
   opened the P8.3 local dependency graph scoped to pfr
   (`Local dependency graph — pfr (external, Math-Graph — not
   independently verified by Mathesis)`) — confirming P8.2/P8.3's code
   is genuinely generic across projects, not implicitly tied to the
   original 4. Zero console errors.
7. `cargo test --all`: clean across all workspace crates (9 new tests
   plus every pre-existing one). `cd web && npx tsc -b`: clean (no
   frontend changes this pass).

As with every prior Math-Graph pass, the regenerated
`math-graph-discovery-p8-1-pilot.json` was copied into `web/public/`
only for this session's local verification, then deleted — never
committed.

## What was deliberately not done

- Did not expand to more than one project this pass — "project by
  project" was taken literally.
- Did not commit the Python scoping/classification scripts
  (`scope_pilot_p8_4.py`, `classify_pilot_p8_4.py`,
  `build_scope_report_p8_4.py`) or any generated data — all live under
  git-ignored `scratch/`, matching every prior pass.
- Did not build a "remove-by-anything-other-than-repo_slug" interface,
  or a web UI for removal — the directive's own audience for this
  capability is whoever operates the pilot import pipeline, not site
  visitors; no UI requirement was stated or implied.
- Did not touch production `scratch/provenance.db` — this entire pass
  operated only on the isolated `scratch/p8_1/pilot_provenance.db`.

## What's next

All four sub-stages the directive laid out under the offline-connector
umbrella (P8.1 artifact, P8.2 Chio Panel, P8.3 graph UI, P8.4
expansion-and-removal) are now in place and verified. Stage 6 (live API
connection) remains explicitly gated on terms/permission/agreement that
don't exist yet — not started, and not this session's call to start
without one of those three preconditions.

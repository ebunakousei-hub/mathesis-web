# Phase 8.1 status — the offline TheoremGraph/Math-Graph connector artifact

> Written 2026-09-09, executing Stage 2 (P8.1) of the user's staged
> TheoremGraph-connector directive, immediately after PA.3's MSC
> classification-status foundation — per the directive's own explicit
> ordering ("create the first real TheoremGraph connector during P8.1,
> immediately after the minimum MSC classification-status foundation is
> in place"). Stages 3–6 (P8.2 Chio Panel connection, P8.3 graph UI,
> P8.4 project-by-project expansion, live-API integration) are **not**
> started — the directive itself stages them as later gates, not this
> pass's job.

## What "the artifact" is

A self-describing release manifest (`PilotArtifactManifest`,
`crates/mathesis-provenance/src/pilot_artifact.rs`) plus the isolated
pilot database it describes (`scratch/p8_1/pilot_provenance.db`, never
production `scratch/provenance.db`) plus a small read-model export
(reusing P7.4's `export-discovery` unchanged). New CLI command:

```
mathesis-provenance export-pilot-manifest --db <path> --release <tag>
  --dataset-revision <hash> --retrieved-at <unix> --scope-report <path>
  [--raw-source-file <path>]... [--index-file <path>]... --out <path>
```

Every field the directive asked for is populated from something real,
not invented: dataset URL/revision (the same HuggingFace commit already
verified in P7), retrieval timestamp (the raw CSVs' own filesystem
mtime — a real proxy, documented as such, not a fabricated precise
value), file hashes (re-verified fresh this pass, matching P7's
originally-recorded values exactly), schema/adapter version, license
and attribution (`math_graph_adapter`'s existing constants, unchanged),
per-project counts, DB-derived counts (queried live from the isolated
DB, not copied from the Python-side report — see the bug below), MSC
classification-status counts, and the read-model export's own SHA-256.

## Project selection: "a few carefully selected additional projects"

No new download — this pass reused the exact `formal_dependency.csv`/
`statement_formal.csv` (1.14 GB) already retrieved and hash-verified in
P7 (`sha256sum` re-run this pass: `69fc8539…`/`bf326614…`, byte-identical
to P7's own recorded values). `scratch/math_graph_pilot/
paper_lean_repo.csv` (a few KB) lists all 30 Lean projects in the
dataset — read directly rather than guessed.

Selected, from that real list, by size and topical variety: **FLT**
(Fermat's Last Theorem, Kevin Buzzard et al. — 2,368 declarations),
**carleson** (Carleson's theorem — 2,852), **PrimeNumberTheoremAnd**
(prime number theorem and friends — 5,108), alongside the 2 existing
Mathlib sub-namespace pilots from P7–P7.4 (63 declarations across
`Mathlib.Algebra.Order.Group`/`Mathlib.CategoryTheory.Category`).

## The literal/hierarchy/excluded split — and its real limitation for non-Mathlib projects

P7.2's `literal` classification means "this declaration exactly matches
one Mathesis independently extracted from its own Lean corpus." Mathesis
has never extracted FLT, carleson, or PrimeNumberTheoremAnd itself (its
own corpus is DeGiorgi + 2 Mathlib sub-namespaces) — there is nothing to
cross-reference, so `literal` is **structurally unavailable** for these
3 projects, not merely unobserved. Every declaration in them is either
`typeclass_hierarchy` (P7.3's schema signal: `kind ∈ {inst, instance}`
and zero proof-type outgoing edges) or `excluded`. The manifest's
`literalMatchMethod` field says this explicitly per project rather than
leaving a zero uncommented.

**This also means the hierarchy rule is being *applied*, not
*re-validated*, on non-Mathlib data for the first time.** P7.3 derived
and validated it against Mathlib/DeGiorgi content specifically (checking
that def-edges reconstruct Mathlib's real `extends` hierarchy). Whether
the same schema signal identifies genuine typeclass-hierarchy positions
in FLT/carleson/PrimeNumberTheoremAnd as reliably is an **open
methodological question**, recorded here rather than silently assumed —
recorded in the manifest's own `scopeNotes` too, so it travels with the
artifact.

> **Update (`docs/P8_5_STATUS.md`)**: this question is no longer purely
> open — a spot-check against the real, live Lean source of 20 sampled
> `typeclass_hierarchy` declarations found real counterexamples (e.g.
> `FLT/Deformations/Algebra/InverseLimit/Basic.lean`'s `Group
> (InverseLimit G f)` instance has substantive multi-field tactic
> proofs, not a trivial structural composition). The rule is a real
> signal but not a "content-free" guarantee — see that document before
> describing these records as verified-structural anywhere.

Real numbers (`scratch/math_graph_pilot/classify_summary_p8_1.json`,
combined with the existing P7.4 numbers in
`scratch/math_graph_pilot/scope_report_p8_1.json`):

| Project | Total declarations | Literal | Typeclass-hierarchy | Excluded |
| --- | ---: | ---: | ---: | ---: |
| Mathlib_v429 (2 sub-namespaces) | 63 | 20 | 42 | 1 |
| PrimeNumberTheoremAnd | 5,108 | 0 | 109 | 4,999 |
| FLT | 2,368 | 0 | 498 | 1,870 |
| carleson | 2,852 | 0 | 54 | 2,798 |
| **Total scanned/scoped** | **10,391** | **20** | **703** | **9,668** |

**What actually landed in the isolated pilot DB** (only the safe subset
— `excluded` declarations are never fed to the Rust importer at all,
matching the P7.4 contract): **723 declarations, 870 dependency edges**
(48 from the original 2 Mathlib pilots + 822 from the 3 new projects).
Of 637,248 scanned dependency edges for the 3 new projects alone,
425,021 were excluded as proof-type and 211,405 for having an endpoint
outside the safe set — only 822 (0.13%) were safe enough to import.
Zero duplicate `declName`+`module` pairs found within any project (a
real check, not assumed).

## A real bug found and fixed while verifying the manifest against actual data

The first version of `build_manifest` set `mscClassificationCounts.
unavailable` from `scope_report.totals.declarations` (10,391 — the full
scanned/scoped universe, including the 9,668 `excluded` declarations
that were **never imported**). Comparing the generated manifest's own
`declarationEntityCountInDb` (723) against that field caught the
mismatch immediately. Fixed to use the DB's own live entity count
instead of the Python-side scope report's total — the two numbers now
necessarily agree, verified: both read `723` from the same manifest
run. Added a regression test
(`msc_classification_count_reflects_what_is_actually_in_the_db_not_the_
larger_scoped_total`) that populates a tiny in-memory DB with 3 entities
against a scope report claiming a 62-declaration total, and asserts the
manifest reports 3, not 62 — so this specific class of bug can't
silently return.

## MSC classification-status counts — honestly all `unavailable`

Per `docs/DATA_DICTIONARY.md`'s PA.3-era classification-status model:
MSC alignment (`mathesis-taxonomy::alignment`) only ever runs on arXiv
concept clusters, never on Lean declarations. This artifact's 723
declarations are therefore reported as `unavailable` — the reserved
status PA.3 defined but left unpopulated, specifically anticipating a
case exactly like this one. Not a new adapter pass over
`msc_classifications` (that table is `cluster_id`-keyed, a
taxonomy-specific concept — see `msc_classification.rs`'s own doc
comment on why it isn't a `RelationAssertion`); the pilot manifest
reports this count directly, honestly, without forcing Lean declarations
into a cluster-shaped table where they don't belong.

## What was NOT done, on purpose, per the directive's own list

- **No full production database migration** — the artifact lives at
  `scratch/p8_1/pilot_provenance.db`, bootstrapped fresh via
  `import-legacy` against two empty `GraphStore`/`TaxonomyStore` files
  (so it carries zero dependency on the old P6.2/P7.2 fixture databases,
  which no longer exist locally after this session's own disk cleanup).
  Production `scratch/provenance.db` was never opened this pass.
- **No browser download, no live API integration** — every byte came
  from the already-downloaded, already-hash-verified P7 CSVs.
- **No canonical trusted graph** — nothing here touches
  `epistemic_state`/`traversal_policy` differently than P7.4's own
  established policy (`extracted`, `visible_only`, by construction of
  `import_pilot` itself, unchanged this pass).
- **No theorem/proof/context text** — only `statement_id`/`decl_name`/
  `module`/`kind`/`file_path`/`is_instance` per declaration, matching
  every prior Math-Graph pass since P7.

## What's committed vs. not, per the directive's own list

**Committed**: `crates/mathesis-provenance/src/pilot_artifact.rs`
(adapter/schema code), the new `export-pilot-manifest` CLI command and
its usage text, `crates/mathesis-provenance/tests/pilot_artifact_test.rs`
+ `tests/fixtures/pilot_sample_{statements,edges}.json` (small, entirely
synthetic — no real Math-Graph content, so no licensing question),
this document.

**Not committed** (git-ignored, matching every prior Math-Graph pilot
pass since P7): the raw 1.14 GB CSVs, the isolated pilot database, the
generated manifest/read-model JSON themselves, and the Python scoping/
classification scripts (`scratch/math_graph_pilot/scope_pilot_p8_1.py`,
`classify_pilot_p8_1.py`, `build_scope_report_p8_1.py`) — kept alongside
`scope_pilot.py`/`scope_pilot_p7_4.py` as scratch tooling, same as
established precedent.

## Verified

- `cargo test --all`: clean, including the new fixture-based end-to-end
  integration test (`pilot_artifact_test.rs`) and 3 new unit tests in
  `pilot_artifact.rs`.
- `export-pilot-manifest` run for real against the isolated P8.1 DB:
  723 declarations, 870 edges, `declarationEntityCountInDb` ==
  `mscClassificationCounts.unavailable` == 723 (confirmed after the
  fix above).
- `export-discovery` (reused unchanged as the "small read-model
  prototype") run against the same DB: 870 edges, 2 literal + 868
  hierarchy, 0 mathesis-checker/mathesis-text (correctly — this DB has
  no Mathesis-side extraction data mixed in, only Math-Graph).
- Production `scratch/provenance.db` untouched — confirmed via
  `git status`/no writes to that path anywhere in this pass's commands.
- Raw source file hashes re-verified fresh (not trusted from memory)
  and matched P7's originally-recorded values exactly.

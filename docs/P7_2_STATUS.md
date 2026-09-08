# Phase 7.2 status — fixing the proven reachability gap, and what's left after that

> Written 2026-09-09, executing the user's P7.2 instruction: fix the
> transitive-import-reachability gap P7.1 proved, re-run the comparison
> with the expanded scope, classify the remaining mismatch with evidence
> (not name-similarity guessing), keep Math-Graph a separate visible-only
> source, and document before deciding whether to broaden further. Still
> no full dataset download, no live API, no merge into Mathesis's
> canonical graph — same boundaries as P7/P7.1.

## 1. Expanded extraction scope

New files, **not edits to P6.2's own committed scripts** (preserves
`docs/P6_2_STATUS.md`'s byte-stable regression baseline):
`crates/mathesis-lean-extract/pilots/ExtractManifest_MathlibAlgebraOrderGroup_P7_2.lean`
and `..._MathlibCategoryTheoryCategory_P7_2.lean`. Each is a byte-for-byte
copy of its P6.2 counterpart's filtering logic (`isProjectModule` through
`declDeps`, `filteringPolicyVersion`, `extractorVersion` — diffed to
confirm, only imports/labels/diagnostics differ) plus the missing
imports:

- Algebra pilot: `+ import Mathlib.Algebra.Order.Group.Action.Synonym`
- CategoryTheory pilot: `+ import Mathlib.CategoryTheory.Category.Factorisation`, `+ import Mathlib.CategoryTheory.Category.RelCat`

Each script also reports whether the target module actually loaded, via
`env.header.moduleNames.contains` — a direct check, not an assumption.

**Real result**, same project revision as P6.2
(`5c8398df528176d9c87ccd9226ba8f7c8852d59c`, `leanprover/lean4:v4.29.0-rc6`):

```
actionSynonymModuleLoaded: true
factorisationModuleLoaded: true
relCatModuleLoaded: true
```

All three previously-unreachable modules confirmed loaded. Both scripts
run twice; both pairs **byte-identical**
(`run1 exit:1 size:311251` / `run2 exit:1 size:311251` for algebra,
`263389`/`263389` for CategoryTheory — `exit:1` is the same benign
`unknown declaration 'main'` warning every `#eval`-only script in this
project emits, unrelated to correctness).

## 2. Re-run comparison — P6.2 baseline vs. P7.2 expanded

| | Project 2 (Algebra.Order.Group) — P6.2 | — P7.2 | Project 3 (CategoryTheory.Category) — P6.2 | — P7.2 |
| --- | --- | --- | --- | --- |
| Target declarations | 601 | **633** | 70 | **346** |
| Raw constant references (Σ) | 10,521 | **10,649** | 831 | **4,901** |
| Published dependencies (Σ) | 378 | **410** | 265 | **1,986** |
| — origin: type | 84 | 116 | 42 | 236 |
| — origin: body | 228 | 228 | 59 | 336 |
| — origin: both | 66 | 66 | 164 | 1,414 |
| Text-extracted judgments (unchanged — same 36/24 `.lean` files, they already contained the 3 newly-reachable files) | 899 (36 files) | 899 (36 files) | 410 (24 files) | 410 (24 files) |
| Text-extracted dependency edges | 473 | 473 | 597 | 597 |
| Checker-derived assertions imported | 59 | **59** (skip 0 dup) | 9 (skip 1 dup) | **111** (skip 2 dup) |
| Declarations unmatched to a judgment | 446 (74%) | **478 (75%)** | 52 (74%) | **247 (71%)** |
| Dependency targets unmatched | 61 | 61 | 90 | **561** |
| Comparison scope (text-extracted ∩ checker-derived scope) | 65 | **65** | 8 | **85** |
| Agree | 59 | **59** | 6 | **54** |
| Text-only | 6 | **6** | 2 | **31** |
| Checker-only | 0 | **0** | 3 | **57** |
| Byte-stable across 2 runs | Yes | **Yes** | Yes | **Yes** |

**Project 2 barely moved.** The 32 new declarations from
`Action/Synonym.lean` added raw candidates (478 unmatched, up from 446)
but produced **zero** new checker-derived dependency edges that matched
into the text-extraction comparison scope — the comparison numbers
(agree/text-only/checker-only) are identical to P6.2's original run.
Not a surprise in hindsight: `Action/Synonym.lean`'s declarations are
almost entirely instances relating `SMul`/`VAdd` typeclasses across
`OrderDual`/`Lex`, and none of that traffic happened to land inside the
name-matched comparison window.

**Project 3 changed substantially.** `Factorisation.lean`/`RelCat.lean`
are real, non-trivial files (structures, functors, an equivalence proof)
— target declarations grew 70→346, checker-derived imports grew 9→111,
comparison scope grew 8→85. This is the honest, expected shape of "the
extraction scope changed" the user asked not to hide: project 3's P6.2
numbers and P7.2 numbers describe measurably different extractions, not
a refinement of the same one.

## 3. Classifying the remaining mismatch — evidence required, no name-similarity guessing

P7.1 left 62 of 63 Math-Graph declarations unexplained by anything other
than "the module wasn't loaded" (18) or "genuinely different declaration
model, inferred not confirmed" (44). With the 3 modules now loaded, this
pass re-checked all 63 against the **expanded** ground truth (P7.2's
raw + published manifests) and, where a name still didn't match
literally, tested one further hypothesis before giving up: Lean's
prime-mark disambiguation (`'`, `''`) reappearing in Math-Graph's data as
underscore-number suffixes (`_1`, `_2`) — verified against the real
source file for each hit, not assumed from the pattern alone.

| Category | Count | Evidence |
| --- | --- | --- |
| **Literal Lean declaration — reachability now fixed** | 7 | `CategoryTheory.Factorisation.{id_h, instQuiver, comp_h, comp_h_assoc, Hom.ι_h_assoc, Hom.h_π_assoc, ι_π_assoc}` — all present, non-generated, in the expanded P7.2 manifest. |
| **Literal Lean declaration — prime→underscore normalized** | 11 | `OrderDual.instPow_1` = `OrderDual.instPow'` (confirmed present in P7.1's own raw dump); `OrderDual.{instMulAction,instAddAction}_1` = `...'`, `OrderDual.instIsScalarTower_{1,2}` = `...'`/`...''`, `OrderDual.{instSMulCommClass,instVAddCommClass,instVAddAssocClass}_{1,2}` = `...'`/`...''` — every one checked against `Mathlib/Algebra/Order/Group/Action/Synonym.lean`'s actual source, which declares exactly the base + `'` + `''` triple for each (e.g. `instance instIsScalarTower`, `instIsScalarTower'`, `instIsScalarTower''`, lines 47–56). |
| **Literal Lean declaration — canonicalized to Lean's auto-naming convention** | 1 | `CategoryTheory.instInhabitedRelCat` — the real source names this instance explicitly (`instance inhabited : Inhabited RelCat := ...`, `RelCat.lean` line 39, i.e. `CategoryTheory.RelCat.inhabited`); Math-Graph reports the name Lean's *automatic* instance-naming convention would have produced had it been anonymous, not the name actually in the source. |
| **Genuine match, unchanged from P7.1** | 1 | `LibraryNote.universe_output_parameters_and_typeclass_caching`. |
| **Unresolved external semantics** | 43 | No literal, reachability, or prime-normalized correspondence found anywhere in the expanded ground truth. Per instruction, not classified further by inference — see below for what *was* checked and ruled out. |

**Present in both systems (after the fix): 20 of 63 (32%)** — up from
1/63 before P7.2. **Math-Graph-only: 43 of 63 (68%)**, all recorded as
*unresolved external semantics*, not "typeclass instance" or
"synthesized/generated declaration" — those labels would require
confirming *how* Math-Graph produced them, which P7.1 already noted
could not be done from the arXiv PDF's available text, and this pass
did not find a new way to confirm it either. What **was** checked and
ruled out for these 43: reachability (all in already-loaded modules),
prime-suffix normalization (none matched), and generated/private status
in Mathesis's own filter (none were `isGeneratedOrPrivate: true` in the
raw dump — the filter never had the chance to touch any of them,
confirming P7.1's finding that filtering isn't the cause here either).
41 of the 43 are `OrderDual.*` names spanning the full `Add`/`Mul` →
`Group`/`CommGroup` algebraic hierarchy in `Synonym.lean` (`instMonoid`,
`instMul`, `instCommGroup`, `instMulOneClass`, ...) where Mathesis's own
raw ground truth has only the bundled top-level instances
(`instGroup`, `instAddGroup`, `instCommMonoid`, ...); the remaining 2 are
`CategoryTheory.Category.mk'` (no `'`-suffixed constructor exists in the
real source, only bare `.mk`) and, from project 3,
`CategoryTheory.instInhabitedRelCat`'s sibling cases were exhausted by
the one match already found. The "resolved typeclass hierarchy" reading
from P7.1 remains the best *unconfirmed* explanation for the 41
`OrderDual.*` cases specifically.

## 4. Source separation — verified unchanged, nothing new required

P7/P7.1 already built this; P7.2 re-confirms it rather than assuming it
still holds:

- **Mathesis checker-derived**: `epistemic_state: observed` (own Lean
  build) or the pre-existing text-extraction `extracted` state — both
  imported into the isolated `scratch/p7_2/project{2,3}_provenance.db`
  study databases (not production `scratch/provenance.db`), exactly like
  P6.2's own study DBs.
- **Math-Graph external**: unchanged from P7 — `epistemic_state:
  extracted`, `evidence_kind: formal_export`, provider `math-graph`,
  `source_kind_label` = "External dataset: Math-Graph (uw-math-ai, CC BY
  4.0) — not independently verified by Mathesis", ref namespace
  `judgment:mathgraph:` which does not resolve through
  `judgment_id_for_entity`'s numeric-id contract (P7.1 finding, still
  true — nothing in P7.2 touches that code path).
- `relation_policy::traversal_policy(DependsOn, Extracted) ==
  VisibleOnly` — unmodified, still the only thing standing between any
  Math-Graph edge and `default_traversal`, and it was not changed this
  pass.

No Math-Graph edge exists in production `scratch/provenance.db` — the
P7 pilot database remains isolated, and P7.2 added no new Math-Graph
import at all (this pass was about *Mathesis's own* extraction
coverage, not re-importing Math-Graph).

## 5. Comparison report

The tables in §2 and §3 above are this phase's comparison report — a
markdown table, not a new interactive UI panel. P7.1 already added the
per-edge source-attribution UI (`source_kind_label` in the provenance
panel); building a *second*, separate aggregate comparison UI panel
would be new scope this phase didn't need to answer "is Math-Graph
useful enough to broaden," so it wasn't built. Noted as a real, available
next step if the user wants a live comparison view rather than a static
report.

## An unrelated production bug found and fixed by this phase's own regression check

`verify-release` run against production `scratch/provenance.db` (part of
this phase's own Definition-of-Done checklist) failed:
`web_export_stale: relations.json: content on disk does not match what
the current ProvenanceStore would generate`. Diagnosed before touching
anything: P6.3 (`669a354`) added a `traversalPolicy` field to
`RelationEdge`, but `web/public/relations.json` was never regenerated
against production afterward — P6.3's and P7/P7.1's own web-export runs
all went to isolated scratch directories, by design, and nothing had
re-run `web-export` against the real site's output since. Confirmed via
diff before fixing: **identical 1,051 entries, identical assertion ids,
the only difference is the additive `traversalPolicy` field** — zero
edges added, removed, or changed. Regenerated
`web/public/relations.json` and `web/public/web-export-manifest.json`
directly (`web-export --out-dir web/public`, not copied from a scratch
run, so the manifest's recorded paths stay correct).
`verify-release`/`npm run build`/`npm run eval` all pass cleanly
afterward, with identical eval numbers to before
(MRR@10 0.9667, 1051 relations / 36 confirmed / 1015 grounded). This is
unrelated to Math-Graph — a genuine gap from P6.3's own change, only
caught because P7.2's Definition of Done required running
`verify-release` against production again.

## Definition of done

- [x] All three previously unloaded modules confirmed loaded (`env.header.moduleNames.contains`, direct check).
- [x] Repeated extraction byte-stable (both pilots, 2 runs each).
- [x] Comparison regenerated with the expanded scope, reported separately from P6.2's baseline, not hidden.
- [x] Remaining mismatches classified where evidence supports it (19 reclassified from "unexplained" to literal-declaration causes); the rest explicitly marked "unresolved external semantics," not guessed.
- [x] Math-Graph remains separate, attributed, `visible_only` — reverified, not just assumed.
- [x] No production trusted edges changed — confirmed via diff before any production file was touched; the one production fix made was independently discovered, unrelated to Math-Graph, and purely additive.
- [x] `cargo test --all`, `npm run build`, `npm run eval`, and `verify-release` all pass.
- [x] Documented here.

## Open question for the next decision

Project 2 (typeclass-heavy) barely changed after fixing reachability —
suggesting its mismatch was never mainly about missing files. Project 3
(namespace/generated-heavy) changed a great deal. If a broader pilot is
worth doing, these two pilots now argue for *different* next steps:
project 2's real blocker looks like the declaration-identity-model
question (§3, still unresolved for 43/63 cases); project 3's was mostly
reachability, now fixed, and its expanded comparison (agree 54,
text-only 31, checker-only 57) is itself worth a closer look before
broadening further — that comparison hasn't been example-level audited
the way P6.1/P6.2's checker-only/text-only pairs were.

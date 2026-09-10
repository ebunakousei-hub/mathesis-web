# Phase 8.7 status — source-provenance validation and the publication decision

> Written 2026-09-10, at the user's explicit direction: before considering
> any broader Math-Graph import or publication, determine (1) whether the
> dataset carries enough source-revision information to reproduce records,
> (2) which projects/records tie to an exact commit, (3) which files and
> declarations remain unverifiable, (4) whether the pilot should stay
> offline permanently or publish as a clearly-labeled experimental layer,
> and (5) what provenance fields a future server-side index would need.
> The full 13.6GB dataset was not downloaded and no live API was
> contacted — everything here runs against the already-downloaded pilot
> CSVs (via P8.6's `index.sqlite`) plus read-only public GitHub metadata
> for the 5 pilot repos, the same category of check P8.5's Phase B item 3
> already used.

**Verdict up front**: the pilot stays offline. Nothing found here
improves on P8.5's picture — if anything it's more clear-cut: the one
field (`leanToolchain`) that looked like a partial anchor turns out to
correspond to nothing checkable at all (item 1), zero projects tie to an
exact commit (item 2), and a direct declaration-level spot check finds
real content drift even at the most forgiving possible baseline —
current HEAD, not the unknown extraction-time revision (item 3). See
item 4 for why "clearly labeled experimental" doesn't change this
conclusion.

## Item 1: does the dataset carry enough revision information?

Queried `index.sqlite`'s `projects` table — **all 30 projects** in the
downloaded corpus (not just the 5 pilot ones), so this is a complete
census, not a sample:

| field | non-empty / 30 |
|---|---|
| `repo_url` | 0 |
| `git_commit` | 0 |
| `mathlib_rev` | 0 |
| `lean_toolchain` | 6 (`Batteries_v427/428/429`, `Mathlib_v427/428/429`) |
| `updated_at` | 30 (all `2026-05-23`, a collection/scrape date, not a source date) |

`lean_toolchain` was the one field P8.5 called "a weaker but genuine
anchor" for `Mathlib_v429`. Checked directly against the three most
plausible upstream repos:

```
gh api repos/leanprover/lean4/git/refs/tags/v4.2.9              -> 404 Not Found
gh api repos/leanprover-community/mathlib4/git/refs/tags/v4.2.9 -> 404 Not Found
gh api repos/leanprover-community/batteries/git/refs/tags/v4.2.9 -> 404 Not Found
```

Same result for `v4.2.7` and `v4.2.8`. Lean4's real tag scheme (checked
via `gh api repos/leanprover/lean4/tags`) is `v4.29.0`, `v4.29.1`,
`v4.30.0`, `v4.33.1`, etc. — two-to-three-digit minor versions, not
`v4.2.x`. `v4.2.7`/`v4.2.8`/`v4.2.9` is not a Lean release, not a Mathlib
tag, not a Batteries tag. Given the repo_slugs are literally
`Mathlib_v427`/`_v428`/`_v429` (three sequential snapshots) and
`Batteries_v427`/`_v428`/`_v429` in lockstep, the far more likely reading
is that `427`/`428`/`429` is Math-Graph's own internal batch/snapshot
counter, reformatted to *look* like a version string — not a value that
resolves to anything outside the dataset. `docs/P8_5_STATUS.md` has been
corrected in place (blockquote) to retract the "weaker but genuine
anchor" characterization.

**Answer**: no. Not one field in this dataset, for any of its 30
projects, resolves to an externally-checkable revision. `updated_at` is
a scrape-time timestamp, useful only as a loose upper bound for a
best-effort estimate (as P8.5's Phase B item 3 already used it) — never
a substitute for a real revision.

## Item 2: which projects/records tie to an exact commit?

**None.** `git_commit` is `NULL` for all 30 projects (query above), which
means all 388,105 statement rows and all 11,335,708 dependency rows in
the downloaded corpus — not just the 5 pilot projects' subset — carry no
commit-level provenance. This isn't a gap specific to the pilot's
selection; it's a property of the dataset as downloaded.

## Item 3: which files and declarations remain unverifiable?

Two distinct questions, checked separately because they have different
answers:

**(a) Does the claimed file still exist, at all, anywhere in the repo's
current state?** For every one of the 113 distinct file paths behind the
5 projects' 674 safe (`external_structural_candidate`) declarations,
fetched each repo's full current-HEAD tree (`git/trees/<branch>?recursive=1`,
one API call per repo) and checked membership:

| project | file paths found at current HEAD | declarations affected by missing files |
|---|---|---|
| FLT | 78/81 | 19 (of 498 safe) |
| Mathlib_v429 | 5/5 | 0 |
| PrimeNumberTheoremAnd | 3/3 | 0 |
| carleson | 15/15 | 0 |
| pfr | 9/9 | 0 |
| **total** | **110/113** | **19** |

The 3 missing FLT files (`FLT/AutomorphicForm/QuaternionAlgebra/Defs.lean`,
`FLT/Basic/FreyPackage.lean`,
`FLT/Deformations/RepresentationTheory/ContinuousSMulDiscrete.lean`) no
longer exist in FLT's repo at all — moved, merged, or deleted since
Math-Graph's snapshot. This is the most lenient possible check (current
HEAD, an actively-developed repo months after collection) and it already
finds real drift.

**(b) Of the files that DO still exist, are the claimed declarations
still there?** A file existing doesn't mean its contents are unchanged.
Fetched the actual current content of one sample file per project (5
files, 16 declarations) and searched for each declaration's short name:

| file | declarations checked | found in current content |
|---|---|---|
| `FLT/EllipticCurve/Torsion.lean` | 4 | 1 |
| `Mathlib/CategoryTheory/Category/Basic.lean` | 1 | 0* |
| `PrimeNumberTheoremAnd/Wiener.lean` | 2 | 2 |
| `Carleson/Defs.lean` | 3 | 0 |
| `PFR/ForMathlib/Entropy/Measure.lean` | 6 | 6 |
| **total** | **16** | **9** |

The `Carleson/Defs.lean` misses were double-checked for a Unicode
false-negative (two of the three names contain `Θ`, GREEK CAPITAL THETA
— a legitimate Lean identifier character, not corrupted data; confirmed
by inspecting the raw codepoints) — genuinely absent even searching for
any `instFunLike*`/`instCoe*ContinuousMap` variant, not a search-encoding
artifact. The one Mathlib miss (`*`) is a methodology limit, not
necessarily evidence of drift: `LibraryNote.universe_output_parameters_and_typeclass_caching`
is Math-Graph's synthesized name for a `library_note "..." in ...` block,
which isn't written as a literal identifier in Lean source at all, so a
substring search can't find it regardless of whether the note still
exists. `ProbabilityTheory.*` in PFR matched only by short name, not full
name — expected, since Lean's `namespace ProbabilityTheory` makes the
qualifier implicit in-file, not a real/short-name distinction bug.

**Answer**: at the file level, ~97% (110/113) of the safe subset's
distinct source files still exist post-hoc, but that already understates
drift — a 16-declaration spot check inside existing files found roughly
half genuinely relocated or renamed (FLT, carleson), against current
HEAD, which is the *most* forgiving comparison available. There is no
way to check against the actual revision Math-Graph extracted from,
because that revision is unknown (item 1/2) — so the true unverifiable
fraction is understated by every number in this section, not overstated.

## Item 4: offline permanently, or a clearly-labeled experimental layer?

The UI/labeling machinery for "clearly labeled experimental layer"
already exists and was built across P8.1–P8.5: the discovery panel is
hidden by default, every external edge carries the
`STRUCTURAL_CANDIDATE_CAVEAT` text inline (not hover-only), external
data never enters default search or traversal (`visible_only` policy),
and the classification vocabulary itself was renamed in P8.5 specifically
so it no longer implies a confirmed fact. So the real question isn't
"can this be labeled honestly" — it already can be, and already is, in
code that's sitting unused. The question is narrower: **does adding a
disclosure label resolve the actual gap this phase found?**

It doesn't, for a reason distinct from reliability: labeling addresses
*"might this classification be wrong,"* which P8.5 already disclosed.
It does not address *"can a reader check this claim against the source
themselves,"* which items 1–3 show is currently impossible in principle
for 4 of 5 projects, and only loosely possible (via a best-effort
date-based estimate, per P8.5 Phase B item 3) for the file-attribution
question, never for the exact declaration content. A caveat that reads
"not independently verified, and cannot currently be verified even by
you" is a materially different, weaker claim than what the existing
caveat text says. It would need rewriting before publication regardless
of the offline/online decision — and once rewritten to be that blunt,
publishing it as a "layer" of the graph, rather than as a documented
methodology limitation, stops being the more honest framing anyway.

There's a second, independent problem labeling can't fix:
**reproducibility.** Without a pinned revision, there is no way to detect
if Math-Graph's own upstream dataset is later corrected, expanded, or
revised — Mathesis's local snapshot would silently drift from whatever
"Math-Graph" means at the time a visitor reads the label, with no
mechanism to notice.

**Decision: stay offline (Option B, reconfirmed).** Not because the
pilot's engineering is unsound — P8.1–P8.6 hold up, and the classifier
itself has held every adversarial check thrown at it. Because the two
things a publication decision actually needs — a claim a reader can
verify, and a snapshot that won't silently go stale — aren't available
from this dataset as downloaded, and no amount of index-building,
re-verification, or relabeling changes that. This is the same conclusion
P8.5 reached, now checked exhaustively (all 30 projects' revision fields,
not an inference from 5; real file-tree and content diffs against live
GitHub, not a plausibility argument) rather than assumed to still hold.

## Item 5: minimum provenance fields for a future server-side index

Not a build spec — a record of what this phase's negative findings imply
a future design would need, so it isn't designed the same way twice.
Per project:

- **A resolved, pinned commit SHA** for the exact revision each
  declaration was extracted from — not a toolchain tag, not a scrape
  timestamp. Item 1 found none of the fields this dataset provides
  substitute for this.
- **A source repository URL that actually resolves** — `repo_url` was
  empty for all 30 projects here; a future index needs this populated
  and checked reachable, not merely present.
- **A content hash of the declaration's source span** (the specific
  lines, not the whole file) at the pinned revision — the file-vs-content
  drift in item 3 shows file-level pinning alone is insufficient; a
  file surviving doesn't mean the declaration inside it did.
- **A declared top-level-directory-to-project mapping supplied by the
  dataset itself**, not reconstructed after the fact — P8.5's
  `project_attribution_unresolved` rule and this phase's `expected_dir`
  table (`pilot_index_lib.PILOT_PROJECT_EXPECTED_DIR`) exist only because
  Math-Graph doesn't supply this; every project onboarded has needed a
  manual audit to establish it.
- **A dataset/extraction revision identifier for Math-Graph itself**
  (a release tag, commit, or DOI-style version) distinct from the
  per-project source revision above — so a future index can detect when
  upstream Math-Graph has changed and knows to re-validate, rather than
  silently serving a stale snapshot indefinitely.
- **Machine-checkable non-triviality evidence for classification**, not
  just a `kind` field — the whole reason `external_structural_candidate`
  exists instead of a confirmed label is that `kind == instance` plus
  "no recorded proof edge" is a structural proxy, not a semantic
  guarantee (`docs/DATA_DICTIONARY.md`). A future source would need to
  supply (or make checkable) the actual elaborated term, not just a
  kind tag, before "structural candidate" could become "confirmed."

None of this is being built now — per the directive, this stays a
specification derived from evidence, not a P8.8 implementation.

## What was (and wasn't) done

- Used: `index.sqlite` (P8.6) for the full 30-project field census;
  read-only `gh api` calls against the 5 pilot repos' public metadata,
  tags, and trees (same category of check as P8.5 Phase B item 3, no
  new class of access).
- Not used: the 13.6GB Hugging Face dataset (not downloaded), the
  Math-Graph/TheoremGraph live API (never contacted), production
  `scratch/provenance.db` (untouched), `web/public/` (untouched — the
  pilot data still isn't there).
- Corrected in place: `docs/P8_5_STATUS.md`'s "weaker but genuine
  anchor" claim for `Mathlib_v429`, via blockquote, pointing here.

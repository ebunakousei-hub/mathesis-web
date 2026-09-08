# Phase 6.2 status — a three-project Lean extraction and comparison study

> Written 2026-09-08, executing the user's P6.2 instruction: determine
> whether P6.1's filtering policy is general, or overfitted to
> DeGiorgi's naming, namespace, and build conventions. Not "collect many
> projects" — test materially different dependency shapes, run the fixed
> procedure unmodified, and report each project separately. This is a
> study, not a release: nothing here touches the production
> `scratch/provenance.db` / `scratch/judgments.db` / `web/public/*.json`
> that P6.1 verified and shipped.

## The three projects

1. **DeGiorgi** (regression baseline) — the P6.1 project, unchanged.
2. **`Mathlib.Algebra.Order.Group`** (typeclass-heavy/abstraction-heavy)
   — the ordered-group typeclass hierarchy (`Monoid` → `Group` →
   `OrderedCommGroup`, `Colex`/`Lex`/`OrderDual` synonym instances…),
   entered via `Mathlib.Algebra.Order.Group.Basic`.
3. **`Mathlib.CategoryTheory.Category`** (namespace/generated/
   library-heavy) — `Category`/`CategoryStruct`/`Quiver`, `Epi`/`Mono`,
   deeply `extends`-chained structures, entered via
   `Mathlib.CategoryTheory.Category.Basic`.

**Why Mathlib subtrees, not two more arXiv papers**: they are real,
independent, actively-maintained code with wildly different naming and
structuring conventions from DeGiorgi (a strong test of "overfitted or
general") — the exact opposite of code I would write myself, which
would risk unconscious bias toward my own filter. They also reuse the
Mathlib checkout P6.1 already built and cached, so no new multi-GB
download was needed. `mathesis-import` was confirmed to have no arXiv
coupling (`GraphStore::intern_paper` does no format validation — see
`crates/mathesis-graph/src/paper.rs`), so the *identical* text-extraction
pipeline DeGiorgi used runs against these subtrees with a synthetic paper
id, producing a genuine text-extraction baseline for the comparison —
not a synthetic one.

## A real finding before any numbers: the P6.1 name-prefix fix was itself DeGiorgi-specific

Running the **unmodified** P6.1 extractor against `Mathlib.Algebra.Order.
Group.Basic` and `Mathlib.CategoryTheory.Category.Basic` first returned
**zero declarations** for both. Not a crash — a silent, wrong empty
result, which is worse.

Cause: P6.1's Bug 1 fix required a candidate declaration's own
*qualified name* to start with the project namespace, in addition to its
*module*, to stop a Mathlib name (`ContDiffBump.mk.congr_simp`) leaking
in as a fake "DeGiorgi declaration." That fix silently assumed DeGiorgi's
own convention — every file wraps its content in `namespace DeGiorgi ...
end`, so name and module share a prefix. **Mathlib does not follow this
convention.** A declaration in the file (module) `Mathlib.CategoryTheory.
Category.Basic` typically lives in the Lean namespace `CategoryTheory.
Category` — with no `Mathlib.` prefix at all. Requiring the name match
made every single Mathlib declaration invisible to the extractor.

Fix: added `requireNamePrefixMatch : Bool`, `true` for DeGiorgi
(regression-tested to produce byte-identical output — see below),
`false` for both Mathlib pilots (module-attribution scoping only, same
as the pre-P6.1 design, now legitimate again because the P6.1 denylist
expansion — `rec`/`inj`/`congr_simp` — closes the *original* leak by a
different, more general route: `congr_simp` is now caught by
`knownGeneratedSuffixes` regardless of which module a name is
attributed to). This is exactly the kind of overfitting P6.2 was
commissioned to find, found on the first real attempt, not a synthetic
worry.

## Per-project results (not combined into one percentage)

| | DeGiorgi | Mathlib.Algebra.Order.Group | Mathlib.CategoryTheory.Category |
|---|---|---|---|
| Lean toolchain | 4.29.0-rc6 | 4.29.0-rc6 | 4.29.0-rc6 |
| Mathlib revision | `5c8398df…` | `5c8398df…` (same checkout) | `5c8398df…` (same checkout) |
| Project commit | not recorded (P6.1 ran against uncommitted source; recording a future hash would be fabrication) | `5c8398df…` (project *is* this Mathlib revision) | `5c8398df…` |
| `requireNamePrefixMatch` | `true` | `false` | `false` |
| Target declarations | 248 | 601 | 70 |
| Raw constant references (Σ) | 32,152 | 10,521 | 831 |
| Published dependencies (Σ) | 735 | 378 | 265 |
| — origin: type | 31 | 84 | 42 |
| — origin: body | 439 | 228 | 59 |
| — origin: both | 265 | 66 | 164 |
| Filtered-out raw constants (Σ) | 31,417 | 10,143 | 566 |
| Text-extracted judgments (whole subtree) | 1,431 | 899 (36 files) | 410 (24 files) |
| Text-extracted dependency edges (whole subtree) | — (production release) | 473 | 597 |
| Checker-derived assertions imported | 551 | 59 | 9 (skip 1 duplicate pair) |
| Declarations unmatched to a judgment | 43 (17%) | 446 (74%) | 52 (74%) |
| Dependency targets unmatched | 73 | 61 | 90 |
| Comparison scope (text-extracted, in scope) | 535 | 65 | 8 |
| Agree | 483 | 59 | 6 |
| Text-only | 52 | 6 | 2 |
| Checker-only | 68 | 0 | 3 |
| Byte-stable across 2 runs | Yes (matches committed P6.1 manifest exactly) | Yes | Yes |

DeGiorgi's row is unchanged from P6.1: the regenerated manifest is
byte-for-byte identical to `scratch/lean-manifest-degiorgi-p6.1.json`
(the committed one), confirming `requireNamePrefixMatch` is a true no-op
for DeGiorgi and the 551 imported edges do not change.

## Why the unmatched rate jumped from 17% to 74%

This is not the filter breaking — it's `judgment_name_index`'s existing,
deliberate "skip ambiguous bare names, never guess" rule
(`lean_manifest_adapter.rs`) doing more work in a denser namespace.
DeGiorgi is one paper's worth of long, paper-specific identifiers
(`unitBallApproxEps_pos`); a 36-or-24-file Mathlib slice packs many
short, generic lemma names (`mk`, `comp`, `id`, `assoc`) into the same
scope, so far more bare names collide across multiple judgments and get
correctly excluded rather than guessed. Confirmed by spot-check: exactly
one judgment in the CategoryTheory scope is named bare `mk`
(`ReflQuiv.lean:157`, judgment id 305) — not itself ambiguous — but the
name is short enough that a differently-scoped project easily could
produce a real collision; DeGiorgi's own comparison already documented
this class (`restrict`, `docs/P6_STATUS.md`).

## Per-project disagreement explanations (not one flattened story)

**DeGiorgi** — unchanged from P6.1: text-only false positives trace to
generic-identifier collisions (`restrict`); checker-only misses trace to
indirection (`unfold`, dot notation) the text matcher can't see.

**Mathlib.Algebra.Order.Group** — **zero checker-only** misses (the
checker-derived set was a strict subset of what text-extraction found in
this project), a real, different shape from DeGiorgi's 68. The 6
text-only cases have a *different* root cause than DeGiorgi's generic-name
collision: inspecting the actual source
(`Algebra/Order/Group/Unbundled/Basic.lean:380`),
`mul_inv_lt_mul_inv_iff'`'s real proof is
`rw [mul_comm c, mul_inv_lt_inv_mul_iff, mul_comm]` — but the
text-extracted "dependencies" reported for it
(`inv_mul_lt_iff_lt_mul`, `lt_inv_mul_iff_mul_lt`, `inv_lt_inv_iff`, …)
match *none* of those identifiers. They do, however, match names of
`alias`/`theorem` declarations packed immediately *after* it in the same
dense file. This is consistent with `mathesis-lean-parse`'s
identifier-proximity heuristic not perfectly isolating one declaration's
boundary in Mathlib's terse, alias-heavy file style — a real, different
failure mode from DeGiorgi's more spread-out formalization, not
diagnosed further here (out of scope: fixing text extraction is not part
of P6.1/P6.2).

**Mathlib.CategoryTheory.Category** — the 3 checker-only edges
(`uliftCategory`, `epi_of_epi`, `epi_iff_forall_injective`, all →
`mk`) share one real, verified cause: the *target* declaration, a
structure's `mk` constructor, is invoked through anonymous-constructor
notation (`⟨…⟩`) at the use sites — which never writes the identifier
"mk" as literal text, so `mathesis-lean-parse`'s identifier matching
structurally cannot see it, while the Lean elaborator's term-level view
sees the real constructor application regardless of surface syntax. This
is a *third* distinct disagreement mechanism (surface-syntax invisibility
of the target, not the source) — genuinely different from both DeGiorgi's
`unfold`-indirection and the Order.Group project's proximity
over-attribution, exactly the kind of "different dependency shape" P6.2
was meant to surface.

## The `restrict`-style collision, checked elsewhere

Per the completion criteria: yes, the *underlying pattern* (a short,
generic identifier the checker resolves unambiguously via full
qualification but that a mathematician's eye — or a text matcher — could
misattribute) recurs, but manifests differently per project: DeGiorgi's
`restrict` is a text-only false positive; CategoryTheory's `mk` is a
checker-only miss (the opposite direction) caused by notation, not
name-genericity per se, though the short name is *why* it was worth
checking specifically. No case was found where the checker itself
resolved a generic name to the *wrong* fully-qualified target — full
qualification at the `Expr` level continues to make that class of error
structurally impossible, matching the mechanism `docs/P6_STATUS.md`
already established.

## Manual inspection of five representative categories

**Type-only** (`origin: "type"`): `Mathlib.Algebra.Order.Group`'s
`instIsCancelAddColex → instAddColex` — an instance's own *type*
(what it's an instance *of*) mentions another instance; its body (the
instance proof/data) doesn't re-mention it.

**Body-only**: `AddGroup.toOrderedSub → sub_le_iff_le_add` — the
instance's data/proof cites a lemma its *type* (the `OrderedSub`
interface signature) never mentions.

**Typeclass-resolution-introduced**: the same
`instIsCancelAddColex → instAddColex` pair — `instAddColex` is itself an
auto-named typeclass instance (Lean's `inst`-prefixed naming for
instances found by resolution), so this edge is a direct, real example
of one instance depending on another purely through the elaborator's
instance-search machinery, not through anything a source-text scan would
recognize as an explicit citation.

**Implementation-only, and separately, dependencies a reader could
mistake for mathematical**: `Mathlib.CategoryTheory.Category`'s
`toCategoryStruct` and `toQuiver` — auto-generated field-projection
functions from `class Category extends CategoryStruct`/`CategoryStruct
extends Quiver`. They appear as a published dependency on **almost every
declaration in the file** (`Category.assoc`, `.comp_id`, `.id_comp`,
`Epi.left_cancellation`, `cancel_epi`, …). A reader skimming the
dependency graph could easily read "`cancel_epi` depends on `toQuiver`"
as a mathematical fact about categories and quivers; it is not — it is
how Lean implements structure inheritance, present because P6.1's own
policy decision explicitly keeps field projections (`docs/
LEAN_DEPENDENCY_POLICY.md`: "structure field projections... are things
the author's structure declaration actually introduces, not derived side
effects of it"). This is real, first-hand confirmation of *why* this
project insists on "elaborated declaration dependencies," never "minimal
mathematical dependencies" — not a hypothetical concern, an observed
pattern in real, published output.

## Verified

- The identical, unmodified filtering logic (`isProjectModule`,
  `knownGeneratedSuffixes`, `isGeneratedOrPrivate`, `declDeps`, and the
  manifest-assembly `#eval` block) runs byte-for-byte the same across
  all three extractor scripts — confirmed by diffing the shared sections
  pairwise before any project ran, not assumed.
- Each project's manifest is byte-stable across two independent runs
  (`diff` on the raw JSON, not just a summary count) — DeGiorgi, Order.Group,
  and CategoryTheory all confirmed.
- DeGiorgi's regenerated manifest is byte-identical to the committed
  P6.1 manifest — the 551 edges do not change.
- No production file changed: `git status` shows only the two new
  `requireNamePrefixMatch`-related lines in `ExtractManifest.lean` and
  the new `pilots/` directory. `scratch/provenance.db`,
  `scratch/judgments.db`, and `web/public/*.json` are untouched; this
  study's databases live under separate `scratch/p6_2_project{2,3}_*.db`
  files.
- No Rust or TypeScript source changed this round — `cargo build
  --workspace` clean, and the full Rust test suite / `verify-release` /
  `npm run build`/`eval` results carried forward unchanged from P6.1's
  own verification (nothing in this round could have invalidated them).
- Unresolved declarations reported, not hidden, per project (43/446/52
  above) — none silently dropped or guessed.

## What this pass deliberately does not do

Per the user's list: does not drop `subject_ref`/`object_ref`, does not
implement `RelationSchema`, does not import TheoremGraph/Math-Graph,
does not describe checker-derived edges as mathematically true or as
minimal dependencies (reinforced, not just repeated, by the
`toCategoryStruct`/`toQuiver` finding above). P6.3 (the authenticated
review workflow) is not started.

## Where to look

- `crates/mathesis-lean-extract/ExtractManifest.lean` — DeGiorgi,
  now with `requireNamePrefixMatch` documented and regression-tested.
- `crates/mathesis-lean-extract/pilots/` — the two Mathlib-slice
  extractors.
- `scratch/p6_2_project2_manifest.json`, `scratch/p6_2_project3_manifest.json`
  — the raw manifests (not committed — regenerable, matching the
  existing `scratch/` convention).
- `scratch/p6_2_project{2,3}_{graph,taxonomy,provenance}.db` — the
  study-only databases (not committed, not production).

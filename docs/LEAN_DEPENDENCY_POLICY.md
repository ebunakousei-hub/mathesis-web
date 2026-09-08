# What counts as a checker-derived dependency (P6.1)

> Written 2026-09-08, executing the user's P6.1 instruction: "Fix that
> ambiguity before adding more trusted edges" — `Lean.Expr.getUsedConstants`
> returns every constant an elaborated term mentions, which includes things
> no mathematician would call a "dependency": auto-generated recursors,
> match compilers, equation lemmas, `noConfusion`/`sizeOf` structural
> members, and private implementation helpers. Phase 6 (`docs/P6_STATUS.md`)
> shipped without filtering any of this out. This document defines the rule
> precisely, states the decisions the user's spec asked for explicitly, and
> records what is *not* claimed.

## Definition

> A checker-derived dependency (a "published dependency") of declaration D
> is a distinct, non-generated, project-namespace declaration C such that
> C's fully-qualified name occurs in D's elaborated type and/or value
> (proof term / body), after removing D itself, constants outside the
> target project's own namespace, and constants recognized as
> compiler-generated or private-implementation detail.

This is called **"elaborated declaration dependencies,"** not "minimal
mathematical dependencies" — nothing here proves minimality (a proof can
mention a lemma it doesn't strictly need), only that the elaborated,
type-checked term actually contains the reference. Every place this data
reaches a human (docs, UI, code comments) must use this phrase, not
"minimal."

## The six decisions the user's spec asked for explicitly

1. **Type and body are combined into one set**, but each published
   dependency is tagged with where it came from: `origin: "type" | "body" |
   "both"`. (`ExtractManifest.lean::declDeps`, `PublishedDep.origin`.)
2. **Transitive dependencies are excluded**, by construction rather than by
   choice: `Expr.getUsedConstants` walks only the target declaration's own
   type/value expression. It does not recurse into the bodies of the things
   that expression references. If C appears in D's elaborated term, that is
   a *direct* reference; whatever C itself depends on is a separate fact,
   recoverable by walking the published-dependency graph one more hop, not
   something this extractor inlines.
3. **All declaration kinds are treated uniformly** in this pass — no
   kind-specific handling for theorem vs. def vs. structure vs. instance vs.
   namespace. Every `ConstantInfo` has a `.type`; `.value?` is `none` for
   axioms and inductive/structure carriers and `some` otherwise, and the
   extractor already branches on that `Option` rather than assuming a value
   exists.
4. **Mathlib (and Lean core) dependencies are excluded** from
   `publishedDependencies` — kept visible in `rawConstants` for audit, but
   not published as an edge. This matches the granularity the existing
   text-extraction (`judgment_dependencies`) already uses: it only tracks
   judgment-to-judgment edges within one paper, never a `judgment ->
   Mathlib lemma` edge. Precedent: `docs/P6_STATUS.md` already made this
   call for the original (unfiltered) extractor; P6.1 keeps it.
5. **Private, inaccessible, generated, and compiler-created names are
   excluded** — see the predicate below.
6. **Self-references and duplicates are removed.** Self-reference removal
   pre-dates P6.1 (Phase 6 already excluded `n != info.name`, catching
   recursive definitions). Deduplication is now paired with a deterministic
   sort (by fully-qualified name) so the manifest's declaration and
   dependency arrays are byte-stable across reruns — `env.constants.toList`
   iterates a `HashMap` whose order Lean does not guarantee, so this is not
   optional if the manifest is going to be reproducible.

## The generated/private/internal exclusion predicate

No single Lean API flags "everything the compiler generated." The predicate
combines five dedicated Lean/Mathlib APIs with one maintained denylist for
the rest:

| Check | Catches |
| --- | --- |
| `Lean.isPrivateName` | `private def`/`private theorem` (Lean mangles these to `_private.<module>.<hash>.<name>`) |
| `Lean.Name.isInternal` | hygiene-macro-scoped names |
| `Lean.Name.isInternalDetail` | a broader "internal detail" marker Lean itself uses (empirically covers e.g. `_sizeOf_1`/`_sizeOf_inst` and matcher auxiliaries) |
| `Lean.isAuxRecursor env` | `.rec`/`.recOn`/`.casesOn`/`.brecOn`/`.binductionOn`/ctor-elim families |
| `Lean.Meta.isMatcherCore env` | `.match_N` match-compiler auxiliaries |
| `Lean.Meta.isEqnThm` (`CoreM`) | auto-generated equation lemmas (`.eq_N` etc.) |
| `knownGeneratedSuffixes` denylist | `noConfusion`, `noConfusionType`, `ctorIdx`, `toCtorIdx`, `ctorElim`, `ctorElimType`, `sizeOf_spec`, `below`, `ibelow`, `binductionOn`, `injEq`, `rec`, `inj`, `congr_simp` |

The denylist exists because none of the five dedicated predicates catch
these — verified empirically by compiling a small `inductive`/`def`-with-
pattern-match probe and printing every predicate's value against every
name Lean generated for it (`ProbeNs.Color.noConfusion`,
`ProbeNs.Color.ctorIdx`, `ProbeNs.Color.sizeOf_spec`, etc. all came back
`false` on every one of the five API predicates).

**The last three entries (`rec`, `inj`, `congr_simp`) were not caught by
the synthetic probe — they were found by running the extractor against
the real DeGiorgi project**, exactly the "inspect what got filtered out
and check it still looks like compiler machinery" operational rule this
document asks for on every new project (see P6.2 below). Two real
structures, `DeGiorgi.Cutoff` and `DeGiorgi.MemW1pWitness`, surfaced them:
`DeGiorgi.Cutoff.rec` (the *primitive* recursor — `isAuxRecursor` catches
`recOn`/`casesOn`/`brecOn` etc., the "auxiliary" recursors, but not the
kernel-generated primitive `.rec` itself, confirmed by directly checking
`isAuxRecursor` against a probe inductive's own `.rec` name and getting
`false`), and `DeGiorgi.Cutoff.mk.inj` / `DeGiorgi.MemW1pWitness.mk.congr_simp`
(constructor injectivity and congruence lemmas, auto-derived, caught by no
predicate at all). Structure field projections and the default constructor
itself (`Cutoff.mk`, `Cutoff.toFun`) are deliberately **not** added to the
denylist — those are things the author's `structure ... where` declaration
actually introduces, not derived side-effects of it, matching the
"constructors are not excluded" rule below.

**This is a maintained, empirically-derived list, not a formally exhaustive
characterization of "compiler-generated."** A future Lean or Mathlib
version may introduce new generated-name shapes this list does not yet
cover. **Operational rule for P6.2** (running the pilot on a new project):
before trusting a new project's `publishedDependencies`, inspect
`rawConstants` entries that are in-namespace but were filtered out anyway,
and spot-check that they still look like compiler machinery rather than
something a person wrote. Extend `knownGeneratedSuffixes` if not.

**Constructors are not excluded.** `Color.red`/`Color.green`/`Color.blue`
are things the author wrote as part of defining the type — a real
dependency, not compiler noise.

## What gets published to the JSON manifest

Two arrays per declaration, both kept (see `docs/P6_STATUS.md`'s
"never-fabricate" precedent — nothing here silently discards data):

- `rawConstants`: every constant `getUsedConstants` found across type and
  value, fully qualified, deduplicated, sorted. Includes self-references,
  Mathlib lemmas, and generated/private names — the full unfiltered signal,
  for auditability.
- `publishedDependencies`: the filtered, project-namespace-only,
  human-meaningful subset, each tagged with `origin`.
- `filteredOutCount`: `rawConstants.len() - publishedDependencies.len()` —
  the minimum bar the spec asked for ("at least the filtering counts").
  The full reason breakdown is recoverable by anyone with the raw manifest
  file by recomputing the predicate above against the difference — not
  duplicated field-by-field into the JSON to avoid bloating a
  643-declaration manifest with per-constant reason strings that are pure
  functions of already-published data.

## A nuance the fixtures surfaced: `origin: "both"` is common, `"type"` is rare

Adversarial fixture 10 (`crates/mathesis-lean-extract/fixtures/Fixtures/
DependencyFixtures.lean`) needed three attempts to actually produce a
`"type"`-only tag. `Expr.getUsedConstants` walks the *fully elaborated*
core term, and Lean's elaborator routinely re-embeds a referenced
constant into the value side even when nothing in the source text visibly
uses it there:

- A `def`/`theorem` whose value is a lambda carries its parameter's type
  annotation as part of the value's own term structure (`fun (_ : Fin
  sized) => ...` contains `sized` in the value, not just the signature).
- `rfl`-style proofs do not delta-reduce away the sub-terms they unify —
  `rfl : restrict 3 = 3` elaborates to a term that still literally contains
  `restrict 3`, not a reduced `3 = 3`.
- Elaborating implicit arguments (e.g. `Or.inr`'s left-disjunct type
  parameter) fills them in explicitly, so proving `P ∨ Q` by `Or.inr proof`
  embeds `P`'s constants into the value even though the proof never
  "uses" `P`.

The one case fixture 10 settled on that reliably produces `"type"` alone is
an `axiom` — `ConstantInfo.value?` is `none` by construction, so nothing
can leak into the value side. This is not a bug in the extractor; it is an
accurate reflection of what "the elaborated value" actually contains in
Lean 4. It reinforces the "elaborated declaration dependencies," not
"minimal," naming above — `origin` says where a reference occurs in the
elaborated term, not where it is logically essential.

## Reproducibility metadata

Every formal-export `SourceRecord` created by `import_lean_manifest` now
carries a `reproducibility_json` blob
(`crates/mathesis-provenance/src/lean_manifest_adapter.rs`):
`leanToolchain`, `mathlibRev`, `projectCommit` (nullable — see below),
`extractorVersion`, `filteringPolicyVersion`, `rawManifestHash` (sha256 of
the exact bytes received), `normalizedManifestHash` (sha256 of a canonical
reconstruction from the *parsed* manifest — invariant to incidental JSON
formatting, sensitive to any real change in declarations/dependencies/
origins). `verify_release`'s new
`verify_formal_evidence_has_reproducibility_metadata` check
(`crates/mathesis-provenance/src/verify.rs`) rejects any `default_traversal`
assertion backed by `formal_export` evidence whose `SourceRecord` is
missing this blob, has an empty required field, or whose
`filteringPolicyVersion` does not match the current build's
`lean_manifest_adapter::FILTERING_POLICY_VERSION` — the same "policy drift"
treatment `SOURCE_MAPPING_POLICY_VERSION` already gets elsewhere in this
crate.

`projectCommit` is the **only** nullable required-but-not-always-present
field: the vendored `fixtures/arxiv/DeGiorgi/` source is committed directly
into the Mathesis repo (no independent git identity of its own — see
`.gitignore`'s treatment of its `.lake/` build output), so "the project's
own commit" is really "the Mathesis repo's own commit at import time." The
CLI accepts it as an optional `--project-commit` flag (matching how
`import-legacy --git-commit` already works, `main.rs`) rather than shelling
out to `git` itself — if the caller does not pass it, it stays `null`
rather than being guessed.

Per-declaration reproducibility (source file, declaration name, filtered
dependency list) is **not** duplicated onto every `Evidence` row it would
apply to. It is recoverable without redundancy: `Evidence.locator` already
encodes `"{qualifiedName} -> {dep}"` (source + target), and the full
published-dependency list for a given source declaration is exactly the
set of `depends_on` assertions whose evidence locator starts with that
declaration's qualified name — no need to also inline the whole list on
every one of a declaration's N edges.

## Why the filter logic exists in two places

`crates/mathesis-lean-extract/ExtractManifest.lean` (the real pilot, run
against the compiled DeGiorgi project) and
`crates/mathesis-lean-extract/fixtures/Fixtures.lean` +
`crates/mathesis-lean-extract/fixtures/RunFixtureTests.lean` (the
adversarial tests, run against a tiny standalone Lake package) duplicate
the filtering predicate rather than sharing one module. This was not the
first choice — verified empirically that `lake env lean --run` cannot
`import` a sibling `.lean` **source** file the way it can a project's own
compiled modules (`import Foo` fails with "no directory 'Foo' or file
'Foo.olean' in the search path" even when `Foo.lean`'s directory is on
`LEAN_PATH`; pre-compiling `Foo.olean` separately hits an "incompatible
header" version mismatch against `lake env lean`'s own toolchain binary).
Cross-project sharing would require turning this into a real Lake
dependency of both projects, which is disproportionate for ~80 lines of
pure logic run as standalone scripts. Both copies are stamped with the
identical `filteringPolicyVersion`/`extractorVersion` strings so a drift
between them is at least nominally visible; if this logic grows
materially, revisit packaging it properly.

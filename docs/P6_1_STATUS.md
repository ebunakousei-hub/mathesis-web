# Phase 6.1 status — making checker-derived dependencies precise and explainable

> Written 2026-09-08, executing the user's P6.1 instruction: harden the
> Phase 6 (`docs/P6_STATUS.md`) pipeline before expanding the trusted
> graph. Phase 6 proved Lean *can* produce trusted evidence; it shipped
> without filtering `Lean.Expr.getUsedConstants`'s raw output, which
> includes compiler-generated recursors, matchers, equation lemmas, and
> private implementation details no mathematician would call a
> "dependency." This pass defines the rule precisely
> (`docs/LEAN_DEPENDENCY_POLICY.md`), hardens the extractor and the
> release gate, adds adversarial fixtures, and — found only by actually
> running the hardened pipeline against real data — fixes two latent gaps
> Phase 6 never surfaced because nothing had tried to click through a
> checker-derived edge's evidence before.

## 1. The filtering policy (`docs/LEAN_DEPENDENCY_POLICY.md`)

Full definition, the six explicit decisions the user's spec asked for,
and the generated/private exclusion predicate live in that document, not
duplicated here. Two things worth surfacing:

- **"Elaborated declaration dependencies," never "minimal mathematical
  dependencies."** Nothing in this pipeline proves minimality — a proof
  can cite more than it strictly needs. Every surface (docs, UI, code
  comments) uses the accurate phrase.
- **The denylist was extended twice by running against real data, not
  just a synthetic probe.** The synthetic probe caught `noConfusion`,
  `sizeOf_spec`, `ctorIdx`, etc. Compiling the *actual* DeGiorgi project
  surfaced two more real gaps the probe missed entirely — see §3.

## 2. `ExtractManifest.lean`: raw vs. published, with origin

Each declaration now emits two arrays instead of one flat `dependsOn`:

- `rawConstants` — everything `getUsedConstants` found, unfiltered,
  sorted, deduped. Nothing is silently discarded.
- `publishedDependencies` — the filtered, same-project-namespace,
  non-generated subset, each tagged `origin: "type" | "body" | "both"`.

Output is now byte-stable: declarations and dependencies are explicitly
sorted by fully-qualified name, because `env.constants.toList` iterates a
`HashMap` whose order Lean does not guarantee.

## 3. Two real bugs found by actually running it, not by reasoning about it

**Bug 1 — module-attribution leak.** The outer loop originally selected
"declarations to report" by checking only which *module* a name was
attributed to. Real DeGiorgi output showed
`ContDiffBump.mk.congr_simp` (a Mathlib name) reported as a DeGiorgi
declaration — Lean sometimes attributes an auto-generated congruence
lemma to whichever file happens to trigger its generation, not to the
name's own defining module. Fixed by requiring the declaration's own
*name* to also start with the project namespace, in addition to its
module.

**Bug 2 — `.rec`/`.mk.inj`/`.mk.congr_simp` slipped through every
predicate.** Running the (already namespace-fixed) extractor against real
DeGiorgi structures (`DeGiorgi.Cutoff`, `DeGiorgi.MemW1pWitness`) turned
up three more generated names none of the five dedicated Lean predicates
catch: the *primitive* recursor `.rec` (Lean's `isAuxRecursor` only
recognizes the "auxiliary" recursors — `recOn`/`casesOn`/etc. — not the
primitive one, confirmed by direct probing), and constructor
injectivity/congruence lemmas (`.mk.inj`, `.mk.congr_simp`). Added to the
denylist. Structure field projections and the default constructor itself
(`Cutoff.mk`, `Cutoff.toFun`) were deliberately **not** added — those are
things the author's `structure ... where` actually introduces, not
derived side effects of it.

Both fixes are documented in `docs/LEAN_DEPENDENCY_POLICY.md` with the
exact real declarations that surfaced them.

## 4. Adversarial fixtures (`crates/mathesis-lean-extract/fixtures/`)

A standalone Lake package (no Mathlib dependency — deliberately, to avoid
a second multi-GB checkout for what only needed Lean core) with 11 small
declarations, each pinning an exact expected `publishedDependencies` set:
direct dependency, non-transitivity (indirect dependency via a
definition), a generic-identifier collision (`restrict`) resolved
correctly by full qualification, a namespace-qualified dependency, a
self-generated matcher excluded from its own owner's dependencies, a
private helper excluded, a self-reference (recursion) excluded, duplicate
references collapsing to one, an external (Lean-core) declaration
excluded, and both `origin: "type"`-only and `origin: "both"` cases.

Three of the eleven needed correction after the *first* run against real
compiled output — not because the extractor was wrong, but because my
predictions about Lean's elaboration were: `rfl`-style proofs don't
delta-reduce the term they display, lambda-valued `def`s carry their
parameter types into the value side of the term, and even a disjunction
proof like `Or.inr h` embeds the *other* disjunct's constants as an
explicit (elaborated) implicit argument. The only declaration shape that
reliably isolates `origin: "type"` turned out to be an `axiom` (no value
at all, by construction). This nuance is now documented in the policy
doc so it isn't rediscovered the hard way again.

All 11 cases pass: `lake env lean --run RunFixtureTests.lean` prints
`ALL 11 FIXTURE CASES PASSED`.

The filtering logic is duplicated (not shared via `import`) between
`ExtractManifest.lean` and `fixtures/RunFixtureTests.lean` — verified
empirically that `lake env lean --run` cannot import a sibling **source**
file the way it imports a project's own compiled modules, and
pre-compiling a shared `.olean` hit a toolchain-binary version mismatch.
Documented as a known, accepted trade-off in the policy doc, with both
copies stamped by the same `filteringPolicyVersion` string.

## 5. Reproducibility metadata + a new release-gate check

Every `formal_export` `SourceRecord` now carries a `reproducibility_json`
blob (Lean toolchain, mathlib revision, project commit — nullable, see
below —, extractor version, filtering-policy version, raw manifest hash,
*normalized* manifest hash computed from the parsed structure so it's
invariant to incidental JSON formatting). New schema: `evidence` gained
`dependency_origin`; `source_records` gained `reproducibility_json`
(idempotent `ALTER TABLE`, following the same pattern as the P5 EntityId
migration).

`verify_formal_evidence_has_reproducibility_metadata` (`verify.rs`)
rejects any `default_traversal` assertion backed by `formal_export`
evidence whose `SourceRecord` is missing this blob, has an empty required
field, or whose `filteringPolicyVersion` doesn't match the current
build's `lean_manifest_adapter::FILTERING_POLICY_VERSION` — the same
"policy drift" treatment `SOURCE_MAPPING_POLICY_VERSION` already gets.
Three new adversarial tests in `verify_test.rs` prove it: missing
metadata is rejected, a stale `filteringPolicyVersion` is rejected, and
complete/current metadata is not.

`projectCommit` stays nullable and is supplied by the CLI
(`--project-commit`, mirroring `import-legacy --git-commit`) rather than
shelled out to `git` — the vendored `fixtures/arxiv/DeGiorgi/` source has
no independent git identity of its own, so "the project's commit" really
means "the Mathesis repo's commit," which this round's import left
unrecorded (the extraction ran against this round's own uncommitted
source — recording a future commit hash in advance would be exactly the
kind of fabrication this project's memory explicitly warns against).

## 6. UI: what a checker-derived edge actually means

This needed more than backend fields — investigating turned up that
**dependency edges had no click-to-detail wiring at all.** Morphism chips
already opened `showAssertionDetail` (external review 2026-09-05,
`provenancePanel.ts`); `depends_on` chips (`renderRelationRow` in
`lineageView.ts`) never did. Added the same `ⓘ` badge pattern:
`LineageGraph` gained `dependencyAssertionId`/`dependencyOrigin` maps
(`lineage.ts`, populated in `proofGraph.ts` alongside the existing
`dependencyPolicy` map), `chip()` now accepts optional provenance info,
and a small green "Lean検査由来" badge marks checker-derived chips inline
(text-extracted stays visually silent — it's the default, matching the
existing "only flag the exception" convention morphism status badges
already use).

Clicking through now shows, for a real checker-derived edge:

> Checker-derived dependency — found in both the declaration's type and
> value.
> leanprover/lean4:v4.29.0-rc6, mathlib 5c8398df, filter policy
> mathesis-lean-dependency-filter-v1
> This means the elaborated declaration's type-checked term contains this
> constant — not that it is a minimal mathematical dependency (a proof
> may cite more than it strictly needs).

(Verified live against `unitBallApproxEps_pos`'s real edges, browser
session, both a checker-derived and a text-extracted chip — see §8.)

## 7. Two more real bugs, found only by actually clicking through the new UI

**Bug 3 — `reconcile` never knew checker-derived edges existed.**
`reconcile_graph` built `assertions.json` (the detail panel's data
source) by iterating `mathesis-graph`'s own `judgment_dependencies` table
— which `import-lean-manifest` never touches, by design (it writes only
to the provenance layer). Clicking a checker-derived edge's new badge
produced "Assertion #105082 not found in assertions.json." This was a
latent Phase 6 gap: nothing had tried to open a checker-derived edge's
detail panel until this round built the wiring to attempt it. Fixed by
having `reconcile_graph` also scan the provenance store directly for
`depends_on` assertions with a `lean-manifest:` legacy_ref and add them
to the same sidecar, resolving `from`/`to` through the entity catalog
(the same `judgment_id_for_entity` reverse lookup `web_export.rs`
already uses). `reconcile` now traces 6185/6185 dependencies (was
5634/5634 — the 551 checker-derived edges were the exact gap).

**Bug 4 — a release-gate check's assumption Phase 6 had already
outgrown.** Fixing bug 3 immediately tripped `verify-release`'s
`ambiguous_identity_key` check: 483 `(from, to)` pairs now legitimately
resolve to *two* different assertions (one text-extracted, one
checker-derived — exactly the "agree" cases the Phase 6 comparison
counts). The check's premise — one key, one assertion — predates Phase
6's dual-source design and was never exercised against dependency data
until reconcile actually started including both sources for the same
pair. Removed the ambiguous-key check for `dependencies` specifically
(citations/morphisms/relations remain 1:1 and keep it), with a comment
explaining why: each adapter's own idempotency is independently
guaranteed by `relation_assertions`'s `UNIQUE(release_id, legacy_ref)`
constraint, which this sidecar-level check was never the primary
guarantee for.

## What this pass deliberately does not do

Per the user's own list: does not drop `subject_ref`/`object_ref`, does
not implement `RelationSchema`, does not touch TheoremGraph/math-graph,
does not promote text-extracted edges, does not treat all
`getUsedConstants` output as mathematically meaningful (the entire point
of this pass), does not expand OpenAlex citation crawling. Also not
started: P6.2 (repeating the pilot on additional projects), P6.3
(authenticated review workflow for reviewed semantic relations).

## Verified

- **Rust**: 46/46 `mathesis-provenance` lib tests, 7+8+18 = 33/33
  integration tests (`store_roundtrip`, `release_gate_test`,
  `verify_test` — including 3 new P6.1 reproducibility-gate adversarial
  tests), workspace-wide `cargo build --workspace` clean.
- **Lean**: 11/11 adversarial fixture cases pass
  (`fixtures/RunFixtureTests.lean`), extractor runs clean against the
  real, already-compiled DeGiorgi project.
- **Real pilot, regenerated from scratch** (`scratch/provenance.db`,
  backed up first as `provenance.db.bak-pre-p6.1`): 248 real declarations
  (down from Phase 6's 643 module-attributed count, which included
  generated/private noise — see §3), 551 checker-derived `depends_on`
  assertions imported (down from 680, for the same reason), comparison
  against text extraction — 535 text-extracted in scope, 483 agree, 52
  text-only, 68 checker-only. Both disagreement classes match Phase 6's
  already-understood causes (generic-name collisions like `restrict`;
  indirection the text matcher can't see) — spot-checked against real
  judgment names, not assumed.
- `mathesis-provenance reconcile` / `web-export` / `verify-release`
  regenerated end to end against `web/public/`: 9520 sidecar entries,
  6185/6185 dependencies traced, web-export integrity OK, zero failures.
- `npm run build` / `npx tsc --noEmit` / `npm run eval` all clean, no
  regression (MRR@10 0.9667, Top-1 95%, dead 0% — identical to Phase 6,
  as expected since P6.1 doesn't touch search).
- **Browser-verified live**, not assumed: `unitBallApproxEps_pos`'s
  lineage view, "Trusted only" toggle showing 18 real edges (same
  judgment Phase 6 verified, confirming no regression), the new
  "Lean検査由来" badge appearing only on checker-derived chips, and
  clicking through to the assertion detail panel for both a
  checker-derived edge (showing the origin/revision/caveat text quoted
  in §6) and a text-extracted one (showing the unchanged, un-embellished
  panel) in the same session.

## Where to look

- `docs/LEAN_DEPENDENCY_POLICY.md` — the policy itself, the predicate
  table, the two Lean bugs, the type/body/both nuance.
- `crates/mathesis-lean-extract/ExtractManifest.lean` — the hardened
  extractor.
- `crates/mathesis-lean-extract/fixtures/` — the adversarial fixtures and
  test runner.
- `crates/mathesis-provenance/src/lean_manifest_adapter.rs` — new
  schema parsing, `FILTERING_POLICY_VERSION`, reproducibility JSON,
  `normalized_manifest_hash`.
- `crates/mathesis-provenance/src/verify.rs` —
  `verify_formal_evidence_has_reproducibility_metadata`, the
  `ambiguous_identity_key` fix.
- `crates/mathesis-provenance/src/reconcile.rs` — the checker-derived
  completeness fix.
- `web/src/lineageView.ts` / `web/src/lineage.ts` / `web/src/proofGraph.ts`
  — the new dependency-edge evidence-navigation wiring.
- `web/src/provenancePanel.ts` — the enriched checker-derived evidence
  rendering.

# Phase 6.3 status — authenticated review, a release gate that enforces it, and one real end-to-end proof

> Written 2026-09-08, executing the user's P6.3 instruction: add
> reviewer-authenticated decision records, define what "trusted" means,
> enforce it at the release gate, surface it in the UI with full
> provenance traceability, and keep the formal/text/reviewed pipelines
> separate. Explicitly deferred: dropping `subject_ref`/`object_ref`,
> `RelationSchema`, TheoremGraph/Math-Graph import, broad citation
> crawling, relabeling extracted relations as trusted.

## What already existed before this round

Before writing any code, this round read the actual state of the
provenance crate rather than assuming P6.3 started from nothing (per
this project's own recurring lesson: verify "not implemented" claims by
reading code, not by trusting a plan document). A substantial amount of
this had already been built under a different name — "Priority 2" — on
2026-09-08, earlier in this same working session, before P6.1/P6.2:

- `review_decisions` table, `ReviewDecision`/`NewReviewDecision`,
  `ReviewOutcome{Accept,Reject,Split,Merge,NeedsExpert}` — an
  **append-only** log (insert-only API, no update/delete method existed).
- `relation_policy::traversal_policy` — a `TraversalPolicy` enum
  (`Excluded`/`VisibleOnly`/`DefaultTraversal`/`FormalOnly`) computed
  purely from `(predicate, epistemic_state)`, already keeping
  text-extracted (`visible_only`), checker-derived
  (`observed`→`default_traversal`), and reviewed semantic assertions
  (`reviewed`→`formal_only`, `verified`→`default_traversal`) on separate
  tracks.
- `verify::verify_trusted_assertions_have_qualifying_evidence`, wired
  into `verify_release` — already rejected any assertion whose
  `traversal_policy == default_traversal` unless it had `formal_export`
  evidence **or** an accept `ReviewDecision` with a non-null
  `reviewer_id`.
- `assertion_export.rs`/`provenancePanel.ts` already exposed
  `reviewDecisions` (decision, reviewer, rationale, scope, timestamp)
  per assertion in the click-through detail panel.

**What did not exist**, and is what this round actually added:

1. No `authorization_level` field — a decision recorded *who* but never
   *under what authority*.
2. No expiration or release-matching. The gate's check was
   `.any(d.decision == Accept && d.reviewer_id.is_some())` over **every**
   review ever recorded for an assertion — a decision made against an
   old release, or later reversed by a reject, still counted forever.
3. No `Supersede`/`Revoke` outcomes — nothing could retract an accept.
4. **No way to actually create an authenticated review at all.** The
   only code path that ever called `insert_review_decision` was
   `legacy_adapter.rs`, migrating old `mathesis-annotate` acceptances
   with `reviewer_id: None` — which the gate correctly never counted as
   authenticated. Checked production `scratch/provenance.db`: 0 rows in
   `review_decisions`, and 0 assertions anywhere in `Reviewed`/`Verified`
   state. The mechanism to reach "trusted via review" was entirely
   theoretical.
5. `RelationEdge` (the type shipped in `relations.json`, i.e. the actual
   concept-relation display data) had no `traversal_policy` field at
   all, unlike `DependencyEdge`/`MorphismEdge` — a structural gap that
   would have made a reviewed concept relation *pass the release gate*
   while still being invisible to the reader as anything other than an
   ordinary extracted relation.

## What this round built (additive, following the existing
`ensure_p6_1_columns`-style idempotent-`ALTER TABLE` migration pattern)

**1. Richer review records** (`model.rs`, `store.rs::ensure_p6_3_columns`)
   — three new nullable columns on `review_decisions`:
   `authorization_level TEXT`, `expires_at_unix INTEGER`,
   `supersedes_review_id INTEGER REFERENCES review_decisions(id)`. Two
   new `ReviewOutcome` variants, `Supersede` and `Revoke`. Kept the
   existing five outcomes (`Accept`/`Reject`/`Split`/`Merge`/
   `NeedsExpert`) unchanged — `ARCHITECTURE_NEXT.md` §10 names exactly
   that vocabulary; `Supersede`/`Revoke` are this round's extension of it
   for exactly the "review decisions create events, they do not
   overwrite the original proposal" append-only requirement §10 already
   states but the code didn't yet let anyone act on.

**2. A real definition of "trusted"** (`review.rs::is_authenticated_accept`)
   — replaces the old `.any(...)` scan. `effective_review_decision`
   takes the *time-ordered last* decision for an assertion (not a chain
   walk through `supersedes_review_id`, which is kept purely as a
   human-readable audit link — "what does the last word say" is a
   simpler and, for a single append-only log, equally correct rule for
   "what's the current status"). A decision counts as an authenticated
   accept only if **all** of:
   - it's `Accept` or `Supersede` (both trust-affirming; `Reject`/
     `Revoke`/`Split`/`Merge`/`NeedsExpert` are not, so a later revoke
     correctly un-trusts an earlier accept — this is the one behavior
     the old `.any()` gate literally could not express);
   - `reviewer_id` is present and non-empty;
   - `authorization_level` is present and non-empty (identity alone is
     not authority — deliberately not fabricating an institutional role
     hierarchy here; the field is a free string, not a fixed enum,
     because this project has no such hierarchy to be honest about yet);
   - `expires_at_unix` is unset or still in the future;
   - `dataset_version` equals the release actually being verified
     ("no drift" — a review made against release A does not silently
     carry over to release B).
   Five unit tests cover: matching accept passes; anonymous accept
   fails; a later revoke withdraws trust from an earlier accept; an
   expired accept fails; an accept recorded against a different release
   does not carry over.

**3. Release gate enforcement** (`verify.rs`) — the existing
   `verify_trusted_assertions_have_qualifying_evidence` now calls
   `effective_review_decision` + `is_authenticated_accept` instead of
   the old any-match. `VerifyInputs` gained `now_unix: i64` (passed by
   the CLI at call time, matching how every other timestamp in this
   codebase is computed at the call site, not read from a clock buried
   inside a library function — keeps expiry testable).

**4. The missing write path** (`main.rs`) — two new CLI subcommands,
   the actual mechanism, not just the record shape:
   - `review` — records a standalone authenticated decision against an
     *existing* assertion (mirrors what the old any-match gate assumed
     already existed).
   - `promote-review` — the command that makes the whole thing usable in
     practice. Because `traversal_policy` gates on `epistemic_state`
     too, and `legacy_adapter`/`mathesis-taxonomy` only ever produce
     `extracted`/`proposed` semantic assertions, `review` alone could
     never actually promote anything — as the production-DB check above
     found, *nothing* was in `Reviewed`/`Verified` state to review.
     `promote-review` inserts a new assertion (same subject/predicate/
     object, new `epistemic_state`, `supersedes_id` pointing at the old
     row — the old row is never mutated), attaches a `reviewer_note`
     Evidence row carrying the reviewer's rationale (the one evidence
     kind `store.rs`'s own schema comment already names as the
     exception to "every assertion needs Evidence"), and records the
     authenticated accept — all inside one transaction. This is the
     concrete, CLI-based analog of what `mathesis-annotate` already is
     for the legacy graph layer, now built for the provenance layer's
     semantic relations.

**5. A latent web-export bug, found by using the feature, not by
   inspection** — running `promote-review` against production data and
   then regenerating `relations.json` showed the new edge nowhere in the
   output. `build_relation_edges` required `source_span` evidence
   specifically; a reviewed assertion has only `reviewer_note`. Fixed to
   accept either. That fix immediately created a second latent bug: a
   synthetic test fixture caught `build_relation_edges` now also
   picking up a *judgment*-domain `EquivalentTo` assertion (a morphism)
   that happened to carry `reviewer_note` evidence, because
   `Specializes`/`EquivalentTo`/`Generalizes` are shared vocabulary
   between judgment-morphisms and concept-relations
   (`relation_policy::valid_entity_kinds` already documents this) and
   `build_relation_edges` never checked `EntityKind` on either endpoint
   — it happened to be safe before only because `reviewer_note` wasn't
   accepted yet. Fixed by requiring both endpoints resolve to
   `EntityKind::Concept`. Both bugs were caught by the existing
   `every_generated_edge_resolves_to_a_consistent_assertion` adversarial
   fixture test — proof the fixture is worth keeping.

**6. `RelationEdge.traversalPolicy`** added (Rust + `types.ts`), matching
   `DependencyEdge`/`MorphismEdge`, closing the structural gap in
   finding 5 above. `status` gained a third value, `"reviewed"`, ranked
   above `"confirmed"` in display order — a human reviewer's judgment is
   stronger evidence than an LLM/detector agreeing with a text match.

**7. Another real gap, found the same way** — `reconcile`'s
   `assertions.json` (the id→detail dictionary the click-through panel
   fetches) is built from assertion ids discovered by walking the
   *legacy* graph/taxonomy tables' own edges. A `promote-review`d
   assertion has `legacy_ref: None` (there is no legacy-table row it
   corresponds to), so it was never discovered by that walk — it showed
   up correctly in `relations.json` (built independently, by scanning
   every assertion in the release) but the detail panel returned
   "Assertion #N not found in assertions.json" for it. Fixed by having
   `run_reconcile` additionally call `build_web_export` and union in the
   ids it actually emits, so `assertions.json` covers everything the
   deployed UI can actually click on, not just what has a legacy
   ancestor. Confirmed the established `import-legacy` → `build-catalog`
   → `reconcile` pipeline order (documented in `docs/P5_STATUS.md`)
   before relying on `build_web_export`'s `entity_count()` precondition
   inside `reconcile` — calling it there does not risk a new failure
   mode in the real pipeline.

**8. UI: review decisions, not just evidence** (`provenancePanel.ts`,
   `types.ts`) — `ReviewDecisionDetail` gained `authorizationLevel`,
   `datasetVersion`, `expiresAtUnix`, and a server-computed
   `isCurrentAuthenticatedAccept` (so the client never has to
   re-implement `is_authenticated_accept`'s logic and risk drifting from
   it). The panel now shows, per decision: decision + reviewer +
   authorization level, a "counts toward trust" / "does not count
   toward trust" badge, the decided-at date, the release the review was
   made against, an explicit staleness warning when that release
   differs from the assertion's own, and an expiry warning. Also fixed
   `dynamicTaxonomy.ts`'s relation badge, which had no case for the new
   `"reviewed"` status and silently fell through to the "text only"
   label, plus a latent cosmetic bug (an arXiv link built from an empty
   `evidenceArxivId` for evidence that isn't from arXiv at all).

## Real end-to-end proof, on production data, then reverted

Backed up `scratch/provenance.db` first. Found a genuine candidate via
direct SQL: assertion #11244, `concept:approximate identity
equivalent_to concept:linear span`, extracted from arXiv 1206.4022's
sentence *"an operator algebra has a contractive approximate identity
iff the linear span of the elements with positive real part is
dense"* — an explicit "iff," a real equivalence in that technical
context, not a spurious co-occurrence.

Ran, against the real production database:

```
mathesis-provenance promote-review --db scratch/provenance.db \
  --assertion-id 11244 --to-state reviewed --release v0-baseline-20260905 \
  --reviewer-id ebunakousei --authorization-level project-maintainer \
  --rationale "..."
```

Result: new assertion #105260, `traversal_policy = default_traversal`.
Then, still against the real database:

- `web-export` → `relations.json` grew from 1051 to 1052 entries; the
  new one carries `status: "reviewed"`, `traversalPolicy:
  "default_traversal"`.
- `reconcile` → `assertions.json` (after the fix in finding 7) contains
  assertion #105260 with the full review record: `reviewerId:
  "ebunakousei"`, `authorizationLevel: "project-maintainer"`,
  `datasetVersion: "v0-baseline-20260905"`, `isCurrentAuthenticatedAccept:
  true`.
- `verify-release` (both the P1 sidecar-completeness gate and the P2
  web-export-integrity gate, against the real graph/taxonomy/provenance
  databases) — **passed cleanly**, including the rewritten
  `trusted_assertion_missing_qualifying_evidence` check now consulting
  `is_authenticated_accept`.
- The old assertion #11244 (still `extracted`, still `visible_only`) is
  untouched and still present — `promote-review` is additive, not a
  rewrite. Both now show on the "approximate identity" concept page,
  with visibly different badges ("authenticated review" vs. "text
  only").
- Verified in the browser (`web/src` served against these regenerated
  exports): the concept page shows the new edge with the "authenticated
  review" badge; the click-through provenance dialog shows `epistemic
  state: reviewed`, `default-traversal eligible`, and the review
  decision line `accept by ebunakousei (project-maintainer) — counts
  toward trust — decided 2026-09-08 · reviewed against release
  'v0-baseline-20260905'`.

**Then reverted.** `scratch/provenance.db` was restored from its
pre-promotion backup (105259 assertions, 0 review decisions — matching
the state before this round, byte-for-byte re-derived, not
approximated) and `web/public/*.json` restored via `git checkout`. This
was a deliberate choice, not an oversight: accepting a specific
mathematical claim under a named reviewer's authority is a **content**
decision, not a plumbing one, and this round's job was to prove the
*mechanism* works end-to-end — which it now demonstrably does — not to
unilaterally leave a real trust assertion standing under the user's name
without the user separately choosing that particular claim. The
mechanism, the CLI, the schema, and the gate are all real and committed;
the one substantive test edge was reverted after being proven to work.

## Completion criteria, checked against what actually happened

- Append-only decision log, not a boolean field: unchanged from before
  this round (already true) — confirmed still true (insert-only API,
  no update/delete on `review_decisions`).
- "Trusted" now requires formal evidence **or** an authenticated,
  non-expired, current-release-matching accept — not "checker-derived
  implies trust," and not "any historical accept counts forever."
- Release gate rejects any `default_traversal` assertion lacking
  qualifying evidence — proven on real data (production `verify-release`
  passes; the rewritten check is what makes it pass, not a weaker one).
- Review UI shows evidence origin, reviewer identity, authorization
  level, rationale, release/version, formal-vs-reviewed, and
  accepted-when/staleness — implemented and confirmed rendering
  correctly in the browser.
- Formal/text/reviewed pipelines stay separate: `evidence_kind`
  (`formal_export`/`source_span`/`reviewer_note`) and
  `epistemic_state` remain the only two axes `traversal_policy`
  consults; nothing was collapsed into one "trusted graph" — a reviewed
  edge and its original extracted edge coexist as two separate
  assertions, visibly labeled differently.
- Rust tests: all pass workspace-wide (`cargo test --all`), including 5
  new `review.rs` unit tests and the fixed
  `every_generated_edge_resolves_to_a_consistent_assertion` regression
  fixture. Web: `tsc --noEmit`, `npm run build`, and `npm run eval` all
  clean, `npm run eval`'s numbers unchanged from before this round
  (1051 relations, 36 confirmed / 1015 grounded — confirming the
  production DB was genuinely restored, not left with drift).

## What this round deliberately does not do

- No web UI for *authoring* a review — `review`/`promote-review` are
  CLI-only, matching this codebase's existing precedent
  (`mathesis-annotate` is CLI-only for the legacy graph layer too). A
  browser-based review workbench is a real feature, not a small one, and
  was not asked for here.
- No enforced authorization-level hierarchy (no "maintainer > reviewer >
  contributor" policy) — `authorization_level` is a free string the
  gate requires to be non-empty, not a fixed vocabulary this project has
  a basis to assert.
- No true evidence-drift detection (e.g. "the assertion's own evidence
  changed after this review was made") — `Evidence` rows have no
  timestamp in this schema, and adding one is a bigger migration than
  this round's scope. "No drift" here means release-tag matching only,
  documented as such, not silently overstated.
- No `RelationEdge`-level "trusted only" UI toggle for concept relations
  — `traversalPolicy` is now exported on every `RelationEdge` (closing
  the structural gap), but `dynamicTaxonomy.ts` itself has no filter
  control consuming it yet; only the lineage view (dependencies/
  morphisms) has that toggle, from P5. Left as data-ready, not
  UI-built, since building a new filter control was not what was asked.
- Did not drop `subject_ref`/`object_ref`, did not implement
  `RelationSchema`, did not import TheoremGraph/Math-Graph, did not do
  broad citation crawling — all explicitly deferred by the user's
  instruction.
- Did not start P6.4/whatever comes after — broadening the pilot to more
  Lean projects, deciding whether the default graph shows trusted-only
  vs. a curated mix, or the dependency/implication/citation/taxonomy
  graph product question are all product decisions the user's own
  instruction described but did not authorize starting.

# Phase A.2 status — responding to 改善点.txt's "Do now" list

> Written 2026-09-09, working through 改善点.txt's own "Recommended
> priority order → Do now" list (items 1, 2, 3, 4, 5, 6, 7 in that
> document). This document covers what's been done in the first pass;
> items not yet reached are listed at the end, not silently dropped.

## Item 1 — the SHA-pinned workflow, verified by actually running it

改善点.txt's premise ("It has been changed to immutable commit SHAs")
was already true when checked: `.github/workflows/deploy-pages.yml`
already pins every third-party action to a commit SHA (`actions/
checkout@11d5960a...  # v4`, etc.) and was already committed at HEAD
(`adad036`) — this repo's write path pins GitHub Actions to SHAs
automatically, not something done by hand this pass.

What the SHA pin being committed does **not** by itself prove — and
what this item's own closing line demands — is that the workflow
actually runs successfully. Checked with `gh run list`: the push from
the prior round **did** trigger `build-and-verify` automatically (it
listens on `push: branches: [main]`), and it **failed**
(run `34306675766`, exit 101). This was a real, previously-unverified
bug, not a hypothetical:

```
error: #[derive(RustEmbed)] folder '.../web/dist' does not exist
  --> crates/mathesis-server/src/main.rs:43:1
error: could not compile `mathesis-server` (bin "mathesis-server" test)
```

`crates/mathesis-server` embeds `web/dist` at **compile time** via
`#[derive(RustEmbed)] #[folder = "../../web/dist"]` — the workflow ran
`cargo test --all` *before* `npm run build`, so on a fresh checkout
(where `web/dist` doesn't exist yet, it's git-ignored) `mathesis-
server`'s own test binary can't compile. This only worked when tested
locally in the prior round because `web/dist` already existed on disk
here from earlier manual builds — a gap in local verification that
only running the real workflow surfaced, exactly the risk this item's
closing line warns about ("Do not regard the site as securely
deployable until the SHA-pinned workflow has actually run
successfully").

**Fixed and reproduced locally, not just reasoned about**: moved
`web/dist` aside, confirmed `cargo test -p mathesis-server` fails with
the identical error GitHub reported; reordered the workflow so
`npm run build` runs before `cargo test --all`; rebuilt `web/dist`;
confirmed `cargo test --all` now passes (all workspace tests green).
**Not yet re-verified on GitHub itself** — that requires pushing this
fix, which hasn't happened as of this document (see "What's not done"
below with the rest of the push-related items).

## Item 4 — generated-artifact freshness, made structural

This project has shipped a stale `web/public/relations.json` at least
once before (P6.3 added `RelationEdge.traversalPolicy`; the export
wasn't regenerated; P7.2's own Definition-of-Done check caught it days
later). The detection mechanism already existed and already worked
(`release_gate::verify_web_export`'s structural re-derivation, plus a
P7.4 regression test proving it catches a field-shape change even when
the stale file's own hash is self-consistent) — the actual gap was
that regenerating (`web-export`) and verifying (`verify-release`) were
two separate commands a person had to remember to run in sequence.

Added `scripts/regenerate-web-export.sh`: runs `reconcile` →
`web-export` → `verify-release` against the fixed local production
paths (`scratch/provenance.db`, `scratch/judgments.db`, `scratch/
papers_100k_fc.db`, `web/public`) as one atomic sequence, non-zero exit
if verification fails. This is "the release command regenerates all
Web artifacts automatically," per 改善点.txt item 4's own first
suggested approach — regeneration and verification can no longer be
two separately-rememberable steps; they're one script invocation.

**Verified by actually running it against production data**, not just
written and assumed correct: `bash scripts/regenerate-web-export.sh`
regenerated `web/public/{judgments.provenance.json,
taxonomy.relations.provenance.json, assertions.json,
provenance-manifest.json, dependencies.json, morphisms.json,
relations.json, web-export-manifest.json}` from the real production
`scratch/provenance.db`, then verified successfully (`9520` sidecar
entries, P1+P2 both OK). `git diff` afterward showed only 3 files
changed, one line each (`generatedAtUnix` timestamp bumps) — everything
else byte-identical, confirming the committed exports were already
fresh and the regeneration is deterministic given unchanged inputs.
Reverted the no-op timestamp diff (`git checkout --`) since nothing
substantive changed; the script itself is the durable deliverable.

`docs/RELEASES.md` and `web/README.md` updated to point at the script
as the only sanctioned way to regenerate `web/public/`'s exported
files from now on.

## What's not done yet in this pass

- **Item 1's own closing bar** ("has actually run successfully") isn't
  met yet — the fix above needs to be pushed and a real Actions run
  needs to succeed before this is true, not just locally reproduced.
- **Item 2 (first real deployment)** — checked via `gh api repos/
  ebunakousei-hub/mathesis-web/pages`: `build_type` is still `"legacy"`
  (branch-based Pages, the one rendering the README) — the Pages source
  has not yet been switched to "GitHub Actions." Still the user's call
  per the prior round's explicit hand-off; unchanged this pass.
- **Item 3 (verify the deployed URL end to end)** — blocked on item 2;
  nothing to browser-test yet.
- **Item 5 (launch messaging matches reality)** — mostly already true
  from Phase A's `#data-scope` page (corpus-not-exhaustive, no-
  minimality claim, `reviewed`-count-is-zero, Math-Graph-not-verified
  are all already stated there, checked against the actual page text
  this pass). The two genuinely missing claims — "MSC2020 covers only
  classified records" and "absence from an MSC category ≠ absence from
  the corpus" — are honestly not addable as a true statement until item
  6 (an actual Unclassified/All-items path) exists; asserting it in
  prose without the UI to back it up would be exactly the kind of
  claim-not-matching-reality this item warns against.
- **Items 6 and 7** (All items/Unclassified navigation; persistent
  graph/source/trust legend) — real, non-trivial UI features, not yet
  started this pass.

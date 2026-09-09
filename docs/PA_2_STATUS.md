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
**Re-verified on GitHub itself, not just locally**: pushed the fix
(`8065d30`), watched the triggered run (`gh run watch 34309780679`) to
completion — `build-and-verify` passed clean in 4m10s (checkout, Rust
toolchain, rust-cache, wasm-pack, the WASM build, `npm ci`, `npm run
build`, `cargo test --all`, `npm run eval`, the dist sanity check —
every step green). `deploy` correctly did not run (an ordinary push to
`main`, not `workflow_dispatch`/a `web-release-*` tag). Item 1's own
bar — "the SHA-pinned workflow has actually run successfully" — is now
genuinely met, confirmed by watching the real run, not assumed from the
local fix.

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

## Item 6 — a visible "no result ≠ no item" path, scoped honestly

Read `dynamicTaxonomy.ts` before building anything (per this project's
own recurring lesson about verifying "not implemented" claims against
real code): a two-tab structure already existed — "Browse by MSC field"
and a tab for clusters where zero members match any MSC2020 code
(`tabNovel`). That's substantively already "MSC2020 categories" +
"Unclassified," at cluster granularity — not the gap 改善点.txt's item 6
is actually pointing at. The real, verified gap was the **explanatory
message itself**: nothing told a user that an empty or thin MSC-field
result means "not yet classified in this release," not "nothing here."

Relabeled `tabNovel` from "Terminology not yet in MSC2020" to
"Unclassified (not yet in MSC2020)" — same meaning, clearer as a
navigation-path name. Added a persistent `mscScopeHint` line, always
visible under the tab bar regardless of which tab is open, stating
almost verbatim 改善点.txt's own required message, plus pointing at the
existing concept search box as the practical "search regardless of
classification" path (a real, already-built "All items" equivalent —
building a second, redundant flat browse-all-94K-concepts view would
contradict this project's own "research interface, not a search
service" self-identity rather than serve the user). "Classification
unavailable/restricted" and "Outside MSC scope" as distinct, separately
tracked statuses are item 9's job (the classification-status data
model, a "Do next" item) — not invented here without the backing data.

## Item 7 — a persistent graph legend covering source and review status

`lineageView.ts` already had a persistent, always-visible legend
(`renderLegend()`) — but it only covered relationship type (spine /
dependency / specialization / equivalence) and traversal policy
(visible-only). The two dimensions 改善点.txt calls out as actually
causing confusion — **source** (checker-derived vs. text-extracted) and
**review status** (proposed / accepted / rejected) — were only ever
shown by opening an individual edge's chip, exactly the "do not require
users to open every edge" failure mode the item describes.

Extended the same legend with 5 more entries, deliberately reusing the
*exact* CSS classes the real per-edge chips already use
(`.lin-chip-origin.is-checker-derived`, `.lin-chip-status-{accepted,
proposed,rejected}`) rather than inventing new colors/shapes that could
drift from what the graph actually shows. Added a dashed empty-swatch
item for "no badge = text-extracted," since the absence of a badge is
itself meaningful here and was previously unexplained. Math-Graph's own
4-way source distinction was deliberately **not** folded into this same
legend — those edges structurally cannot appear in this lineage view
(P7.1's numeric-judgment-id finding) and already have their own
complete, correct legend inside the separate Math-Graph discovery
panel; merging the two would suggest a combination that can't occur.

Verified live in the browser (not just compiled): opened a real
judgment's lineage view, dumped `.lin-legend`'s rendered HTML via
`javascript_tool` — all 10 items present with the correct badge classes
and colors in both languages, no console errors. `npm run build` +
`npm run eval` clean afterward (MRR@10 0.9667 unchanged, 0 contradictory
cycles).

## What's not done yet in this pass

- **Item 2 (first real deployment)** — checked via `gh api repos/
  ebunakousei-hub/mathesis-web/pages`: `build_type` is still `"legacy"`
  (branch-based Pages, the one rendering the README) — the Pages source
  has not yet been switched to "GitHub Actions." Still the user's call
  per the prior round's explicit hand-off; unchanged this pass.
- **Item 3 (verify the deployed URL end to end)** — blocked on item 2;
  nothing to browser-test yet.
- **Item 5 (launch messaging matches reality)** — the two claims tied
  directly to item 6's data model ("MSC2020 covers only classified
  records," "absence ≠ absence from corpus") are now communicated in
  the UI itself (`mscScopeHint`, above) rather than only in prose; the
  rest was already true from Phase A's `#data-scope` page.
- **Item 9's fuller classification-status model** (classified /
  unclassified / unavailable / outside-scope / pending / rejected, as
  distinct tracked statuses with their own evidence/revision/confidence)
  is still not built — item 6's UI-level fix above is real but narrower
  than that data model, deliberately, per its own "Do next" priority.

# Phase A.1 status — reproducible GitHub Pages deployment through Actions

> Written 2026-09-09, executing the user's PA.1 milestone: a GitHub
> Actions-based, artifact-driven Pages deployment, replacing the old
> manual "build locally, copy dist/, commit to a throwaway clone" process
> ([[mathesis-web-published]], now corrected). All work in this pass is
> local — new/changed files, verified by local builds and test runs.
> **Nothing has been pushed to `origin`, no GitHub repository setting has
> been changed, and no deploy has been triggered** — those three actions
> are exactly what this document flags as still needing the user's
> explicit go-ahead, per this session's own publish-confirmation rule.

## What was built

- **`.github/workflows/deploy-pages.yml`** — two jobs:
  - `build-and-verify` (every push to `main`, every PR into `main`, and
    manual/tag triggers): `cargo test --all` → `wasm-pack build` (run
    from `crates/mathesis-wasm/`, so the existing remap-path-prefix fix
    still applies) → `npm ci` → `npm run build` → `npm run eval` → a
    sanity check that `dist/` actually contains `index.html`, `404.html`,
    `.nojekyll`, a `.wasm` asset, `relations.json`/`assertions.json`, and
    that `index.html` references `/mathesis-web/assets/...` (catches a
    silently-wrong base path before it ships).
  - `deploy` (needs `build-and-verify` to pass; only runs on
    `workflow_dispatch` or a push of a `web-release-*` tag):
    `actions/configure-pages` + `actions/upload-pages-artifact` +
    `actions/deploy-pages`, with `pages: write`/`id-token: write`
    permissions scoped to just that job and a `concurrency` group so two
    deployments can't race.
  - An ordinary push to `main` (not a tag) runs the verify job only, per
    the user's own "do not deploy automatically on every push" point —
    `deploy`'s `if` condition is false for that event.
- **`web/vite.config.ts`**: `base` is now computed from Vite's own
  `command` (`"/mathesis-web/"` for `vite build`, `"/"` for the dev
  server) instead of relying on remembering to pass `--base=...` by hand
  every time. Verified: a fresh `npm run build` produces
  `dist/index.html` with `src="/mathesis-web/assets/index-*.js"` /
  `href="/mathesis-web/assets/index-*.css"`, and the bundled JS embeds
  `/mathesis-web/` (from `import.meta.env.BASE_URL`) at every one of the
  fetch call sites already fixed in the original 2026-09-05 launch
  (`dynamicTaxonomy.ts`/`proofGraph.ts`/`searchWorker.ts`/`main.ts`'s
  Math-Graph discovery panel) — no source changes were needed there,
  only the config-level base.
- **`web/public/.nojekyll`** (ships into `dist/.nojekyll`): defensive —
  the Actions-based Pages deployment method doesn't run Jekyll on the
  uploaded artifact regardless, but this makes that explicit rather than
  relying on it being true by omission.
- **`web/package.json`**: added a `postbuild` script
  (`node -e "...copyFileSync('dist/index.html','dist/404.html')"`) so a
  direct/deep link 404s to the same app shell instead of a bare GitHub
  Pages 404 page. Cross-platform (Node, not shell `cp`) since local dev
  is Windows and CI is `ubuntu-latest`. Worth being honest about scope:
  this app has no client-side router today (single `index.html`,
  anchor-only in-page navigation, confirmed by grepping `web/src` for a
  router) — so there are no "deep-linked routes" for the 404 fallback to
  actually rescue yet. It's cheap, harmless, and future-proofing, not a
  fix for an active gap.
- **`web/README.md`**: replaced the stale "copy `dist/` into a clone of
  `mathesis-web`, commit, push" republish instructions with the new
  Actions-based process (manual `workflow_dispatch` or a `web-release-*`
  tag; ordinary pushes verify only).

## Where this pass deliberately diverges from the literal PA.1 spec, and why

**`mathesis-provenance verify-release` is not run in CI.** The spec asked
for "provenance/release verification" as a build step. That check needs
the live `ProvenanceStore` (`scratch/provenance.db`) — P1's sidecar-
completeness check walks assertion → evidence → source_record → release
against the actual database, not just the exported JSON. `scratch/` is
deliberately `.gitignore`d (see its own comment: "Working/experiment
state... Not release data") and was never meant to be committed — it's
sizeable and treated as local/private throughout this whole session.
Committing it just to satisfy CI would be a bigger, unrequested
architecture change (and would re-publish a large SQLite file publicly)
for a check that already has a home: it's the local pre-tag gate this
session has run before every prior release-shaped commit (most recently
in Phase A, this same day). What CI runs instead is `npm run eval`
against exactly the `web/public/*.json` files this build ships — a real
regression gate (MRR@10, contradiction-cycle check, the web-export
bidirectional-consistency check), just scoped to what's actually
committed rather than what can't be.

**Toolchains are pinned to what's locally verified**, not just "stable":
Rust `1.98.0` (`dtolnay/rust-toolchain@1.98.0`, matching
`docs/RELEASES.md`'s own recorded toolchain), `wasm-pack 0.15.0`
(`jetli/wasm-pack-action`, matching `wasm-pack --version` output here),
Node `24.16.0` (matching `node --version` here) — reproducibility over
always-latest, per the spec's own "pinned Node and Rust toolchains."

**SPA fallback and deep-link smoke checks** are scoped honestly per the
point above — no router exists yet, so "test direct navigation to the
Chio Panel / data-scope page / language-specific routes" doesn't apply
literally (there's one URL, `#data-scope` is an in-page anchor not a
route). The `404.html` copy is kept anyway as harmless future-proofing.

## Verified locally, this pass

- `cargo test --all`: clean (already re-confirmed no Rust changed this
  pass beyond what Phase A already tested).
- `npm run build`: clean, `dist/` contains the correct base-pathed
  `index.html`/assets, plus `404.html` and `.nojekyll` from the new
  `postbuild` step.
- `npm run eval`: MRR@10 0.9667 (unchanged), 0 contradictory cycles,
  1051/1051 `relations.json` entries bidirectionally consistent.
- `npm ci` (clean install, not `npm install`) succeeds against the
  existing committed `package-lock.json` — confirms the CI workflow's
  own `npm ci` step will work. `npm audit` reports 2 dev-only
  vulnerabilities (esbuild's dev-server request-forwarding issue,
  GHSA-67mh-4wv8-2f99) — irrelevant to a static production build (no dev
  server is ever exposed publicly) and fixing it means an unrequested
  major-version Vite bump; noted here, not silently ignored, not acted
  on this pass.
- The `.github/workflows/deploy-pages.yml` YAML itself has **not** been
  executed by GitHub Actions — it can't be, since nothing has been
  pushed. Its correctness rests on each step having been run locally in
  the same order (wasm-pack from `crates/mathesis-wasm/`, then `npm ci`,
  then `npm run build`, then `npm run eval`) and matching action
  behavior I'm confident about (`actions/checkout`,
  `actions/setup-node`, `actions/upload-pages-artifact`,
  `actions/deploy-pages` are all official, extremely widely used
  actions) — but "verified locally" is not the same claim as "verified
  in Actions," and I'm not asserting the latter.

## What's still the user's call — nothing below this line has been done

Per this session's own rule, pushing code to a public remote, changing a
repository's settings, and triggering a public deployment each need
explicit confirmation — not just endorsement of the overall mechanism
plan. Concretely, still needed:

1. **Push to `origin`.** `origin/main` has zero commits not already in
   local `master` (verified: `git rev-list --left-right --count
   master...origin/main` → `N 0`), so this is a plain fast-forward, not
   a force-push — but it is still the first time this session's actual
   Rust source, `docs/`, and history become publicly visible on
   `github.com/ebunakousei-hub/mathesis-web`, which is a real,
   consequential "pushing code" action.
2. **Set the repository's Pages source to "GitHub Actions"** (currently
   whatever default is causing the README-render behavior found in
   Phase A) — a repository settings change, doable via the GitHub web UI
   or `gh api repos/ebunakousei-hub/mathesis-web/pages`, either of which
   this session has not touched.
3. **Trigger the first real deploy** — either a manual
   `workflow_dispatch` run from the Actions tab, or pushing a
   `web-release-2026-09-09`-style tag. Per the user's own point 8, this
   is meant to be a deliberate, separate action from step 1, not
   automatic.

Steps 2 and 3 are blocked on step 1 regardless (the workflow can't run
on GitHub until it's pushed). This document, and PA_STATUS.md before it,
intentionally stop here.

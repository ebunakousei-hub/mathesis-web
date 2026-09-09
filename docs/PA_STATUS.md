# Phase A status — launch preparation (research-preview scoping pass)

> Written 2026-09-09, executing the "Phase A — Launch preparation" track of
> the user's launch/MSC-indexing directive: freeze the corpus, run the
> release gate, do a security/privacy pass, and add a public data-scope and
> licensing page — stopping short of any actual deploy/publish action,
> which needs the user's explicit go-ahead per this session's own rule for
> publishing/modifying public content. Phase B (MSC discovery foundation)
> and Phase C (public expansion) are not started.

## 1. Corpus freeze / release candidate

The production corpus (`scratch/provenance.db`) has not changed in row
counts since the `v0-baseline-20260905` release (verified: `stats`,
`web-export-manifest.json`, and `provenance-manifest.json` all still show
6,185 dependencies / 2,284 morphisms / 1,051 relations, identical to
`docs/RELEASES.md`'s own table). All P1–P7.4 work happened either in
isolated study databases (`scratch/p6_2/`, `scratch/p7_2/`, `scratch/p7_4/`)
or as additive schema/code changes that don't touch production row counts.
`review_decisions: 0` in production — no assertion has an authenticated
human review yet; the P6.3 review pipeline exists but hasn't been run
against real production data. There is nothing to "freeze" in the data
sense; the candidate is: this data, at the current commit.

`mathesis-provenance verify-release` run fresh against production:

```
--- P1: sidecar completeness (verify) ---
verified 9520 sidecar entries
OK — every entry resolves through sidecar -> assertion -> evidence -> source_record -> release.
--- P2: web-export integrity ---
OK — dependencies.json/morphisms.json/relations.json match the live ProvenanceStore and the web-export manifest.
```

No web-export regeneration was needed — the gate already passes, and
`web/dist`'s existing build (from the P7.4 pass) already reflects current
source.

## 2. Security/privacy review

- `web/public/*.json` and `web/dist/assets/*` scanned for the local
  username/absolute-path leak class of bug (`docs/`-referenced memory
  `wasm-builds-leak-local-paths-without-remap.md`): clean. The
  `crates/mathesis-wasm/.cargo/config.toml` remap-path-prefix fix is still
  in place and still effective in a fresh build.
- No API keys, tokens, or secrets found in `web/src/*.ts` (grepped for
  `api[_-]?key|secret|password`; the one `token` hit is unrelated —
  search-term tokenization).
- The app is a static, client-only build: no server-side endpoints, no
  user accounts, no database exposed to the browser beyond the pre-built
  JSON snapshots. Most of the checklist's server-hardening items (rate
  limiting, parameterized queries, admin auth for review commands) do not
  apply to the current deployment shape and are recorded here as N/A
  rather than silently skipped.
- Review/ingestion commands (`review`, `promote-review`, `import-*`) are
  CLI-only, never exposed to the web build — already true before this
  pass, reconfirmed.

## 3. Public data-scope & licensing page

New section (`#data-scope`, `web/src/i18n.ts` + `web/src/main.ts` +
`web/src/style.css`), linked from a line directly under the tagline so
it's reachable without scrolling past six other sections first. Bilingual
(ja/en), verified live via the dev server in both languages (see
`applyLangUi()`'s new `#data-scope-*` wiring). Content, sourced from
verified facts (constants already in the Rust code, not invented):

- **Status/snapshot**: research preview, `v0-baseline-20260905` corpus,
  reconfirmed unchanged.
- **Source table**: Lean/Mathlib proof graph (Apache-2.0), arXiv concept
  taxonomy (arXiv's own metadata-reuse terms; only single-sentence
  attributed excerpts shown, never full paper text), MSC2020 (CC BY-NC-SA
  4.0, Mathematical Reviews/zbMATH — this attribution did not exist
  anywhere in the UI before this pass, despite the license constant
  existing in `msc_adapter.rs` since P4), Math-Graph (CC BY 4.0,
  visible_only, not independently verified), OpenAlex (CC0 1.0, 0 citation
  edges currently resolved in this corpus despite the pipeline existing).
- **Trust-level glossary**: checker-derived / text-extracted / reviewed
  (explicitly noted as 0 in this release) / external-visible_only /
  proposed (~50% measured precision, 38 samples).
- **Explicit "what this is not" list**: not exhaustive search, not proof
  verification, no minimality claim, not every edge is a proof/implication,
  external data not independently verified.
- **Experimental features list** and an **operational note** (static
  read-only snapshot, no accounts, no server API, no telemetry).

Verified in the browser: renders correctly in both languages, no console
errors, `npm run build` + `npm run eval` (MRR@10 0.9667, unchanged) both
clean, no leaked local paths in the fresh `web/dist`.

## 4. Deploy / publish — not done, and a real gap found

Per this session's own rule, publishing or modifying public content needs
the user's explicit go-ahead — this pass stopped before any deploy step.
While checking what "deploy to staging" would even mean here, found:

- `git remote -v` → `origin` is `https://github.com/ebunakousei-hub/mathesis-web.git`.
- `git ls-remote --heads origin` → only `main` exists, no `gh-pages`
  branch, no CI/CD workflow file anywhere in this repo (`.github/workflows`
  doesn't exist).
- `origin/main` is **14 commits behind local `master`**, stopping at
  `d645d76` ("Connect MSC2020 source and harden provenance adapters" — an
  early P3/P4-era commit). None of P4's late items, P5, Priority 2 (the
  trust-badge fix), P6.x, or P7.x have been pushed.
- The live URL (`https://ebunakousei-hub.github.io/mathesis-web/`) was
  checked directly in-browser this pass: it currently serves GitHub
  Pages' default Jekyll rendering of the repository's own `README.md`
  (crate table, dev instructions, "MIT license" footer) — **not** the
  built interactive `web/dist` app. `web/dist/` is git-ignored and has
  never been part of any pushed branch or workflow found in this repo.
- Net effect: the interactive Mathesis web app has apparently never
  actually been deployed to this URL. The `mathesis-web-published.md`
  memory entry claiming otherwise is stale and should be corrected.

This is the actual state of "staging" today: nothing is there yet. A real
first deploy — not a relaunch — is needed before "Phase A, step 7" can
mean anything, and involves decisions (push 14 commits to a public `main`,
choose a Pages source/branch, decide whether to build via Actions or
commit `dist/`) that are exactly the kind of publish-affecting,
public-visibility choices this session defers to the user.

## What's done vs. what's still the user's call

Done, local-only, no public effect:
- [x] Corpus reconfirmed frozen; release gate passes.
- [x] Security/privacy scan of the static build — no findings needing a
  fix.
- [x] Public data-scope/licensing page written, wired, bilingual, verified
  in-browser (no console errors, build + eval clean).

Explicitly not done — needs the user:
- [ ] Deciding what "staging" and "publish" should actually mean now that
  there's no existing deployment to build on (push commits to `main`?
  which Pages source? commit `dist/` or add a build workflow?).
- [ ] The actual push/deploy action itself.
- [ ] Phase B (MSC discovery foundation) and Phase C (public expansion) —
  not started, per the user's own "Phase A first" framing.

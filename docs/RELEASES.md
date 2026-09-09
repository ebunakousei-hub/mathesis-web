# Releases

A release is a tagged, reproducible snapshot: the exact git commit, the
public JSON files it shipped, and the corpus counts at that point in time.
This satisfies ARCHITECTURE_NEXT.md §12 Phase 0's "record actual corpus
counts at release time" and gives Phase 1+ a fixed baseline to diff against.
This is a human-readable manifest, not the versioned append-only release
system §5.4/§6.2 describes for later phases.

## research-preview-2026-09-09

The first public deployment — the live site actually serves the
interactive app for the first time (previously the GitHub Pages URL
served a Jekyll render of this repo's README; see
`docs/PA_STATUS.md`/`docs/PA_1_STATUS.md`/`docs/PA_2_STATUS.md` for the
full trail of what that took). Labeled a **read-only research preview**
— see the live `#data-scope` page for exactly what that means and
doesn't.

- **Tag**: `research-preview-2026-09-09` (annotated, points at the
  commit below).
- **Source commit**: `d02c0347e1b791297b7c46707e8bb9359feddf6f`.
- **Deployment**: GitHub Actions, `.github/workflows/deploy-pages.yml`,
  triggered manually (`workflow_dispatch`) — run
  [`34311497848`](https://github.com/ebunakousei-hub/mathesis-web/actions/runs/34311497848),
  both `build-and-verify` and `deploy` succeeded. Pages source switched
  from legacy branch-based publishing to `"workflow"` (GitHub Actions)
  the same session, confirmed via `gh api repos/.../pages` →
  `build_type: "workflow"`, `status: "built"`.
- **Live URL**: https://ebunakousei-hub.github.io/mathesis-web/ —
  verified end-to-end in-browser post-deploy (not assumed from a green
  CI run): app shell, asset/WASM/worker loading, concept + judgment
  search, MSC field browsing and the "Unclassified" tab, the persistent
  empty-category explanation, the graph legend (source + review-status
  badges), a real evidence/provenance panel (assertion detail), the
  bilingual `#data-scope` page, a direct deep-link to `#data-scope` on
  a fresh page load, mobile viewport (no page-level horizontal
  overflow), the `404.html` app-shell fallback, and `.nojekyll` — all
  confirmed working, zero console errors, zero failed network requests.
- **Corpus / data snapshot**: `v0-baseline-20260905` — row counts
  unchanged since that release (reconfirmed by Phase A/PA.1/PA.2's own
  `verify-release` runs and `scripts/regenerate-web-export.sh`'s test
  run, all this same week).
- **Provenance release**: `release_id=1`, `release_tag=v0-baseline-20260905`.
- **Catalog revision**: `mathesis-provenance::catalog-v1` /
  `mathesis-taxonomy::resolve-v1`, 98,468 entities, 117,795 aliases
  (`web/public/provenance-manifest.json`'s `catalog` block).
- **Dataset revisions**: arXiv/Lean legacy import inputs —
  `scratch/judgments.db` sha256 `a23a538d…`, `scratch/papers_100k_fc.db`
  sha256 `f6e11dd3…` (both recorded in `provenance-manifest.json`);
  Math-Graph pilot — HuggingFace commit `ced4ca9de1bd9e5b67aa09d1d515e270e438fa1e`
  (`uw-math-ai/math-graph`, CC BY 4.0); MSC2020 — Mathematical
  Reviews/zbMATH, CC BY-NC-SA 4.0.
- **Generated artifact hashes** (`web/public/web-export-manifest.json`):
  `dependencies.json` sha256 `b9917ee2…`, `morphisms.json` sha256
  `102e874d…`, `relations.json` sha256 `3336f8a9…`.
- **Build date**: 2026-09-09.

## v0-baseline-20260905

The first commit into version control — this repository had no `.git`
before this release. Captures the application exactly as it stood before
the ARCHITECTURE_NEXT.md migration begins.

- **Commit**: `626500f8e2736719997fa8922b761e4d72d07730` (tag
  `v0-baseline-20260905`; resolve with `git rev-parse v0-baseline-20260905^{commit}`).
  This hash changed once, after the initial commit: the first 3 commits and
  the tag were rewritten to use a pseudonymous author identity before any
  push, since the original local git config used a real personal email.
- **Generated at**: 2026-09-05
- **Toolchain**: `rustc 1.98.0` / `cargo 1.98.0`

### Corpus counts

From `crates/mathesis-graph`'s export (`web/public/judgments.json`):

| Metric | Count |
| --- | --- |
| Judgments | 4,052 |
| Dependencies | 5,634 |
| Morphisms | 2,284 |
| Papers (Lean-linked) | 138 |

From `crates/mathesis-taxonomy`'s export (`web/public/taxonomy.json`):

| Metric | Count |
| --- | --- |
| Papers | 142,948 |
| Candidate phrases | 113,339 |
| Resolved concepts | 94,278 |
| Clusters | 34,083 |
| Ambiguous clusters | 1,334 |

From `web/public/relations.json`:

| Metric | Count |
| --- | --- |
| Relation edges (total) | 1,051 |
| — status: confirmed | 36 |
| — status: grounded | 1,015 |
| — status: proposed (shipped) | 0 |

### Public JSON files (offline snapshot / read model)

Node data (judgment statements, papers, concepts, clusters, search index)
comes from `mathesis-graph`/`mathesis-taxonomy`'s own exporters. Edge data
(dependencies, morphisms, typed concept relations) comes from
`mathesis-provenance web-export` — generated directly from the evidence
core, not from these two crates' SQLite tables. See
`docs/P2_STATUS.md` for why this split exists and what changed.

| File | Size | Generated by |
| --- | --- | --- |
| `web/public/judgments.json` | 2.67 MB | `mathesis-import --export` (node data only as of P2 — no `dependencies`/`morphisms` arrays) |
| `web/public/dependencies.json` | 228 KB | `mathesis-provenance web-export` |
| `web/public/morphisms.json` | 410 KB | `mathesis-provenance web-export` |
| `web/public/taxonomy.json` | 8.4 MB | `mathesis-taxonomy export` |
| `web/public/taxonomy.papers.json` | 19.3 MB | `mathesis-taxonomy export` |
| `web/public/taxonomy.related.json` | 12.9 MB | `mathesis-taxonomy export` |
| `web/public/relations.json` | 419 KB | `mathesis-provenance web-export` (supersedes the old `taxonomy.relations.json`, which is no longer generated) |
| `web/public/taxonomy.aliases.json` | 781 KB | `mathesis-taxonomy export` |
| `web/public/taxonomy.head.json` | 27.6 KB | `mathesis-taxonomy export` |

None of these files carry a schema/adapter/model version field today (only
a per-file `generatedAtUnix` timestamp) — that gap is intentionally left for
Phase 1's release-manifest schema (§5.4), not backfilled here.

### Provenance integrity-check files (Phase 1, added after this release's initial cut)

Generated by `mathesis-provenance reconcile` from this same release's
`ProvenanceStore` — see `docs/P1_STATUS.md`. As of P2 (`docs/P2_STATUS.md`),
the browser no longer fetches `judgments.provenance.json`/
`taxonomy.relations.provenance.json` — `dependencies.json`/`morphisms.json`/
`relations.json` above already carry their own `assertionId`. These two
files now exist solely as inputs to `mathesis-provenance verify`'s
release-integrity gate.

| File | Size | Covers |
| --- | --- | --- |
| `web/public/judgments.provenance.json` | 322 KB | all 5,634 dependencies + all 2,284 morphisms (0 citations, since `paper_citations` is empty in this release) |
| `web/public/taxonomy.relations.provenance.json` | 114 KB | all 1,051 shipped (grounded/confirmed) relations — `Proposed` relations are excluded, matching what `relations.json` itself ships |
| `web/public/assertions.json` | 4.4 MB | full per-assertion detail (predicate, subject/object, epistemic state, every Evidence row's kind/locator/source, review decisions, default-traversal eligibility) for all 8,969 referenced assertions — powers the Web app's provenance detail panel |
| `web/public/provenance-manifest.json` | <1 KB | machine-readable record of how the two sidecar files above were produced: release id/tag, adapter name+version, a SHA-256 of each input database, generation timestamp, and counts |
| `web/public/web-export-manifest.json` | <1 KB | P2 (`docs/P2_STATUS.md`): machine-readable record of how `dependencies.json`/`morphisms.json`/`relations.json` were produced — schema version, release id/tag/commit, `web_export_version`, a SHA-256 of each of the 3 output files (computed after writing), and their counts. Written by `mathesis-provenance web-export` itself, not by `reconcile`. |

`mathesis-provenance reconcile` verifies 100% coverage before writing these
files (non-zero exit otherwise) — as of this release, every one of the
5,634 + 2,284 + 1,051 = 8,969 currently-displayed edges resolves to exactly
one `RelationAssertion`.

**Release gate**: `mathesis-provenance verify-release` is the single
canonical gate — it runs P1's `verify` (sidecars + manifest against the
live `ProvenanceStore` and, if given the original input databases, their
current SHA-256) *and* P2's web-export check (the 3 edge files' hashes
against `web-export-manifest.json`, plus a structural re-derivation
comparison against the live store, which catches an export generated from
a stale commit/DB state even if its recorded hash was recomputed to
match). One nonzero exit code means the release is not publishable. See
`docs/P1_STATUS.md` and `docs/P2_STATUS.md` for exactly what this does and
doesn't guarantee.

**Regenerating `web/public/`'s exported files**: always use
`scripts/regenerate-web-export.sh` (`bash scripts/regenerate-web-export.sh`
from the repo root), never `reconcile`/`web-export` run by hand. This
project has twice shipped a stale `web/public/*.json` after a schema
change (P6.3's `traversalPolicy` field, caught during P7.2's own
Definition-of-Done check) because regeneration and verification were two
separate, separately-rememberable commands. The script runs
`reconcile` → `web-export` → `verify-release` as one atomic sequence
against the fixed local paths (`scratch/provenance.db`,
`scratch/judgments.db`, `scratch/papers_100k_fc.db`) and exits non-zero
if verification fails — regeneration and verification can no longer
happen as two separate steps a person has to remember to chain
(`docs/PA_2_STATUS.md`).

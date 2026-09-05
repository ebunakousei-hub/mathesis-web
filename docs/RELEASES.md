# Releases

A release is a tagged, reproducible snapshot: the exact git commit, the
public JSON files it shipped, and the corpus counts at that point in time.
This satisfies ARCHITECTURE_NEXT.md §12 Phase 0's "record actual corpus
counts at release time" and gives Phase 1+ a fixed baseline to diff against.
This is a human-readable manifest, not the versioned append-only release
system §5.4/§6.2 describes for later phases.

## v0-baseline-20260905

The first commit into version control — this repository had no `.git`
before this release. Captures the application exactly as it stood before
the ARCHITECTURE_NEXT.md migration begins.

- **Commit**: `325dba1cc10b03f16593149d4928766e87a0bd5f` (tag
  `v0-baseline-20260905`; resolve with `git rev-parse v0-baseline-20260905^{commit}`).
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

From `web/public/taxonomy.relations.json`:

| Metric | Count |
| --- | --- |
| Relation edges (total) | 1,051 |
| — status: confirmed | 36 |
| — status: grounded | 1,015 |
| — status: proposed (shipped) | 0 |

### Public JSON files (offline snapshot / read model)

| File | Size |
| --- | --- |
| `web/public/judgments.json` | 3.64 MB |
| `web/public/taxonomy.json` | 8.4 MB |
| `web/public/taxonomy.papers.json` | 19.3 MB |
| `web/public/taxonomy.related.json` | 12.9 MB |
| `web/public/taxonomy.relations.json` | 316 KB |
| `web/public/taxonomy.aliases.json` | 781 KB |
| `web/public/taxonomy.head.json` | 27.6 KB |

None of these files carry a schema/adapter/model version field today (only
a per-file `generatedAtUnix` timestamp) — that gap is intentionally left for
Phase 1's release-manifest schema (§5.4), not backfilled here.

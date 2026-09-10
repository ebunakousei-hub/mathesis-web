# Phase 8.3 status — connecting the offline connector to the graph UI

> Written 2026-09-10, executing Stage 4 (P8.3) of the user's staged
> TheoremGraph-connector directive, immediately after P8.2's Chio Panel
> connection. Stage 5 (P8.4, project-by-project expansion) and Stage 6
> (live-API integration) are **not** started.

## What Stage 4 actually asked for, and the real constraint on doing it literally

The directive: "Only after the search/index path is stable should
external edges appear in the graph. Keep them separate: Mathesis
checker-derived edges; Mathesis text-extracted edges; reviewed
relations; Math-Graph literal dependencies; Math-Graph
typeclass-hierarchy structures. External edges remain `visible_only`.
Typeclass-hierarchy records should be represented as structural
external nodes, not as ordinary Lean declaration nodes."

Mathesis's actual "graph UI" (`LineageView`/`lineage.ts`, driving the
proof-graph/lineage pages) is built entirely around Mathesis's own
`ExportedJudgment` records — numeric judgment ids, real statement text,
Lean/LaTeX parsing, morphisms, concept-search handoff. Math-Graph
declarations have **none of that** (P7's own deliberate scope: graph
structure only, no statement/proof/context text, for licensing and
scope reasons — see `docs/P7_STATUS.md`). There is also **no
established link** between a specific Mathesis judgment and a specific
Math-Graph declaration: the two are separate catalogs (confirmed by
re-reading `math_graph_adapter.rs`'s own doc comment, written in P7 —
building that link would mean either cataloging Math-Graph declarations
as Mathesis's own judgments, or adding a 4th `EntityKind`, both
explicitly deferred as a separate, unsolved problem).

Given that, literally feeding Math-Graph data through `LineageView`
was rejected as the wrong move on two counts: it would require either
fabricating statement/judgment-shaped data Math-Graph doesn't have, or
solving the deferred entity-resolution problem first (a much larger,
separate undertaking) — and even if either were done, the result would
render with the exact same CSS classes and interactions as trusted
Mathesis judgments, which is precisely what the directive's own text
prohibits ("not as ordinary Lean declaration nodes").

## What was built instead: a self-contained local dependency graph for Math-Graph's own structure

Two new files, deliberately parallel to (but not sharing code or CSS
with) `lineage.ts`/`lineageView.ts`:

- **`web/src/mathGraphLineage.ts`** — pure data layer. `buildNeighborhood(edges, focus)`
  takes one Lean project's already-`sourceProject`-scoped
  `DiscoveryEdge[]` (from P8.2's `discovery_export.rs`) and a focus
  declaration's label, and returns its **direct** dependencies and
  dependents only — one hop, not a multi-level traversal. Each side is
  capped at `MAX_NEIGHBORS = 12`, with the excess reported as a count,
  not silently dropped.
- **`web/src/mathGraphLineageView.ts`** — a stateless render function,
  `renderMathGraphLineage(edges, state, handlers)`, producing a 3-row
  SVG+HTML diagram (dependents above, focus in the middle, dependencies
  below). New CSS namespace `mgl-*` (`web/src/style.css`) — dashed
  borders, no shared class with `.lin-node`/`.lin-edge` anywhere, so
  the two are never visually confusable. Clicking a neighbor re-focuses
  the view on it (walking the graph one hop at a time); a separate "ⓘ"
  toggle on each neighbor shows the connecting edge's real evidence
  (epistemic state, traversal policy, edge type, license, locator) —
  "inspect why an edge exists" — without re-focusing.

**Why a render function, not a class with its own DOM root** (unlike
`LineageView`): `MathGraphDiscoveryPanel` already does a full
state-driven `innerHTML = ""` + rebuild on every interaction (the same
pattern `dynamicTaxonomy.ts` uses). A second component with its own
persistent root and lifecycle would either fight that pattern or need
its DOM re-parented on every parent render. Simpler and more robust:
the graph's state (`{repoSlug, focus, detailFor}`) lives as one field
on `MathGraphDiscoveryPanel`, and the render function is called fresh
each time, like every other sub-view in that class.

**Why 1-hop instead of porting `lineage.ts`'s multi-layer algorithm**:
considered reusing `lineage.ts`'s spine-finding/layering/barycenter
code (it's largely generic over `Map<number, number[]>` internally),
but rejected it — that code has no test suite (this project's web/
package has no test runner or `*.test.ts` files at all; verification is
`tsc -b` + manual browser checks), so a refactor risky enough to touch
the production lineage view's core algorithm would have no safety net
to prove zero regression. A bounded, 1-hop-at-a-time view is also a
more direct, literal implementation of the directive's own acceptance
criterion ("long-running graph expansions are bounded and
cancellable") than a multi-hop view with truncation heuristics would
have been.

## Wiring (`web/src/mathGraphDiscovery.ts`)

`SOURCE_LABEL`/`SOURCE_BADGE_CLASS`/`isExternal` were exported (were
module-private) so the new view reuses the exact same source
labels/colors rather than a second, potentially-diverging copy.
`renderEdge`'s subject/object text became `<button>`s ("View local
dependency graph" tooltip) that set `lineageState` and open the graph,
scoped to their own `repoSlug` group's edges only — never mixing
declarations from different Math-Graph projects into one neighborhood
(relevant now that P8.1's combined pilot DB spans 5 projects in one
export). Only one graph view is open at a time, appended under
whichever group it belongs to.

## Verified

Browser pane rendering was unavailable mid-session (pane reported
hidden), so verification used direct DOM/JS inspection
(`javascript_tool`) instead of screenshots — confirmed:

- Clicking a declaration opens `.mgl` with correct title
  (`Local dependency graph — Mathlib_v429 (external, Math-Graph — not
  independently verified by Mathesis)`), real SVG `<path>` edges, real
  node HTML.
- Clicking a neighbor node re-focuses: verified focus changed from the
  original declaration to `CategoryTheory.Factorisation.comp_h`, with
  its own correct single neighbor.
- Clicking a neighbor's "ⓘ" opened `.mgl-detail` with the real edge's
  evidence: `epistemic state: extracted`, `traversal: visible_only`,
  `edge type: sig`, `license: CC-BY-4.0`, and the exact
  `formal_dependency.csv` locator string.
- Closing removed `.mgl` from the DOM (count 0).
- **Bounding math**, checked against the real P8.1 dataset directly
  (not just code review): `instCommRingUltraProduct
  (FLT.Patching.Ultraproduct)` has 34 real dependents in the export —
  confirmed the neighborhood logic caps this at 12 shown / 22 omitted,
  matching `MAX_NEIGHBORS` exactly. (This specific node wasn't reached
  through the UI's own pagination in this session — verified by running
  the same slice/cap arithmetic the code uses against the live fetched
  JSON, not by a live click-through of that exact node.)
- `read_console_messages`: zero errors throughout.
- `cd web && npx tsc -b`: clean, both before and after all changes.

No Rust changes this pass — `discovery_export.rs`'s `sourceProject`
field (added in P8.2) already carried everything the neighborhood
builder needs.

## What was deliberately not done

- No entity linkage between Mathesis's own judgments and Math-Graph
  declarations — that's the real prerequisite for showing Math-Graph
  nodes *inside* the primary lineage view, and remains explicitly
  deferred (P7's own scope boundary, re-confirmed here rather than
  worked around).
- No multi-hop / full-project graph rendering — bounded to 1 hop per
  view by design, not as a temporary limitation.
- The pilot's derived index (`math-graph-discovery-p8-1-pilot.json`)
  was, again, copied into `web/public/` only for this session's local
  verification and removed afterward — not committed, same as P8.2.

## What's next

Per the directive's own staging, Stage 5 (P8.4, project-by-project
expansion) is next but not started — it's gated on the pilot
"demonstrating useful results, acceptable storage, acceptable query
latency, valid licensing, stable source mappings, understandable graph
presentation," which is a judgment call for the user to make now that
P8.1–P8.3 are all in place and verified.

# Phase 7.3 status — a focused comparative study, before any broadening decision

> Written 2026-09-09, executing the user's P7.3 instruction: a focused
> study using only data already downloaded in P7 (`scratch/math_graph_
> pilot/`, `scratch/p7_2/`) — no new Math-Graph download, no live API,
> no code changes, no merge into the trusted graph. Math-Graph stays
> `visible_only`, separate, and attributed throughout. This is analysis
> only; nothing in the codebase changed this pass.

## 1. Representative declarations

| Declaration | Project | P7.2 category | Why it's representative |
| --- | --- | --- | --- |
| `OrderDual.instMonoid` | 2 (Algebra.Order.Group) | unresolved | The clearest case of the dominant (41/63) failure mode. |
| `OrderDual.instPow_1` | 2 | matched (prime-normalized) | Shows the mismatch isn't monolithic — some "unresolved-looking" names are real declarations under a renamed suffix. |
| `CategoryTheory.Factorisation.comp_h` | 3 (CategoryTheory.Category) | matched (reachability-fixed) | A real, substantial declaration with a genuine proof term — the best case for measuring discovery coverage. |
| `CategoryTheory.Category.mk'` | 3 | unresolved | The one CategoryTheory case that *doesn't* fit the otherwise-clean "reachability explains it" story for that project. |

## 2. Classifying the remaining 43 with schema evidence — three hypotheses tested, two rejected, one confirmed

P7.2 left 43 declarations as "unresolved external semantics" per the
user's own instruction not to guess from name similarity. This pass
tested three *schema-level* signals — Math-Graph's own columns, not
name patterns — before accepting or rejecting each:

**Hypothesis A — `kind` column (`inst` vs `instance`) distinguishes
synthesized from literal declarations.** In this pilot's 63-declaration
sample, all 53 `kind: inst` rows are from Algebra.Order.Group and all 10
`kind ∈ {instance, theorem, def}` rows are from CategoryTheory.Category
— a suspiciously clean split. **Rejected** on further check: queried the
*full* 388,105-row `statement_formal.csv` (already downloaded, no new
fetch) for the global `kind` distribution — `thm`/`theorem`
(142,696/142,517), `definition`/`def` (37,507/20,281),
`instance`/`inst` (18,776/17,018), `structure`/`struct`, `constructor`/
`ctor`, `inductive`/`ind` all appear as near-equal-magnitude abbreviated/
full-word pairs *everywhere* in the dataset. This is an artifact of
Math-Graph mixing two naming vocabularies across its own extraction
batches, unrelated to whether a record is literal or derived. Reported
as a rejected hypothesis, not silently dropped.

**Hypothesis B — `via_proj` marks projection-derived (non-literal)
edges.** Checked `formal_dependency.csv`'s `via_proj` field for edges
sourced from matched vs. unresolved declarations. **Rejected**: `false`
for 100% of edges in both groups (109/109 matched, 181/181 unresolved).
Not informative here.

**Hypothesis C — `edge_type` profile distinguishes them.** Matched
(literal) declarations' edges: `sig` 73, `proof` 27, `def` 8, `docref` 1
— a real mix including genuine proof-term references. Unresolved
declarations' edges: `sig` 99, `def` 82, **`proof` 0** — confirmed
individually for all 42 unresolved `OrderDual.*` declarations, not just
in aggregate. **Confirmed, and load-bearing**: a declaration with zero
`proof`-type outgoing edges never had its own elaborated proof term
analyzed — consistent with not being a literal declaration at all.

**Content check, not just schema**: `OrderDual.instMonoid`'s `def`-type
edges point to `OrderDual.instSemigroup` and `OrderDual.instMulOneClass`
— exactly Mathlib's real `class Monoid extends Semigroup, MulOneClass`.
Checked systematically across all 42: their `def`-edges form a
self-contained subgraph that exactly reconstructs Mathlib's actual
`extends` hierarchy among `Add`/`Mul` → ... → `Monoid`/`Group`/
`CommGroup` (`instDivInvMonoid → instMonoid, instInv, instDiv`;
`instAddZeroClass → instZero, instAdd`; etc.), and their `sig`-edges that
land outside the pilot scope point to the **real** Mathlib classes
themselves (`OrderDual.instMonoid`'s `sig` edge target is the actual
`Monoid` class in `Mathlib.Algebra.Group.Defs`, and the real `OrderDual`
type synonym). This is not a nonsensical or fabricated graph — it is a
semantically correct, verifiable reconstruction of Mathlib's typeclass
hierarchy, expressed as declaration-shaped nodes that do not correspond
to literal Lean declarations.

**Revised classification for the 43**: 41 (`OrderDual.*`, both projects'
prime-normalized subset now excluded) are reclassified from "unresolved
external semantics" to **"materialized typeclass-hierarchy position,
well-evidenced by schema (zero proof-edges) and content (edges exactly
match the real `extends` graph) but not literal Lean declarations."**
This is stronger than P7.1/P7.2's hedged inference — it's now supported
by two independent lines of Math-Graph's own data, not name-pattern
guessing. The remaining 2 (`CategoryTheory.Category.mk'` and one
`OrderDual.*` case without a clean hierarchy explanation) stay labeled
**unresolved external semantics** — the same zero-proof-edge signature
applies, but no corroborating content pattern was found for them
specifically, so no stronger claim is made.

## 3. Discovery coverage — measured, not assumed

**For matched (literal) declarations — comparable, not superior.**
Total edge count, Math-Graph vs. Mathesis's own checker-derived
`publishedDependencies`, for all 8 reachability-fixed/matched
CategoryTheory declarations:

| Declaration | Math-Graph (total edges) | Mathesis checker-derived (published) |
| --- | --- | --- |
| `Factorisation.id_h` | 6 | 8 |
| `Factorisation.instQuiver` | 3 | 5 |
| `Factorisation.comp_h` | 8 | 9 |
| `Factorisation.Hom.ι_h_assoc` | 9 | 11 |
| `Factorisation.ι_π_assoc` | 8 | 10 |
| `Factorisation.comp_h_assoc` | 12 | 11 |
| `Factorisation.Hom.h_π_assoc` | 9 | 11 |
| `RelCat.inhabited` | 3 | 1 |

Same order of magnitude throughout, neither system consistently ahead.
For declarations both systems recognize as real, Math-Graph is not
finding meaningfully more (or less) than Mathesis's own `mathesis-lean-
extract` already does — this supports **"differently normalized
dependency universe,"** not "extra recall," for this class of
declaration.

**For the 41 typeclass-hierarchy entries — genuinely additive, but a
different kind of fact.** Mathesis's own pipeline has no mechanism to
produce a node for "OrderDual's Monoid structure" at all — Lean itself
never names it, so `env.constants` never contains it, so
`mathesis-lean-extract`'s declaration-selection loop structurally cannot
select it (P7.1/P7.2's finding). Math-Graph's materialized hierarchy view
surfaces real, verifiable structure (§2) that Mathesis's literal-
declaration model cannot represent at all today. This is additive
coverage of a **different information type** (typeclass hierarchy
membership) rather than recall of the same kind of fact
(elaborated-proof-term dependencies) that Mathesis's pipeline already
targets.

## 4. Role

Evidence-based, not a preference:

- **Not a canonical graph input**, for either project as currently
  scoped. Project 2: 41/63 (65%) of Math-Graph's declarations in this
  slice don't correspond to Mathesis's own declaration model at all: a
  1:1 merge would misrepresent what a graph node *is*. Project 3: where
  declarations do match, coverage is comparable, not superior — merging
  would add redundancy more than value, at real integration cost
  (namespace/entity-model work from P7).
- **External comparison layer — yes, well-supported**, specifically for
  declarations both systems recognize as literal (project 3's
  reachability-fixed set, and presumably similar cases elsewhere). Cross-
  checking against an independently-built dependency graph over the same
  Mathlib source is a legitimate, low-cost use, and P7.1's
  `source_kind_label` UI infrastructure already supports showing it this
  way if such edges are ever imported for real.
- **Recall-oriented discovery source — yes, but narrowly**: specifically
  for typeclass-hierarchy structure in typeclass-heavy code (project
  2-style), where Math-Graph surfaces real structural facts Mathesis's
  own literal-declaration pipeline cannot produce today. Not a general
  claim that Math-Graph finds more proof-level dependencies — §3 shows
  it doesn't, for the declarations where a fair comparison exists.

## What this pass deliberately does not do

- No new Math-Graph download (all analysis reused `scratch/math_graph_
  pilot/formal_dependency.csv`/`statement_formal.csv`, already local
  from P7).
- No code changes — pure data analysis, so no test suite / web build /
  eval run this pass (nothing in the codebase moved).
- No merge of any Math-Graph edge into `scratch/provenance.db`.
  Everything stays in the isolated P7/P7.2 study data.
- Did not audit project 3's Mathesis-internal checker-only/text-only 57/
  31 pairs (flagged as an open question in `docs/P7_2_STATUS.md`) — that
  is a different comparison axis (Mathesis's own checker vs. its own
  text extraction, not involving Math-Graph), out of scope for what this
  pass was asked to measure.
- Did not decide whether to broaden past these two pilot projects — this
  reports evidence for that decision, consistent with every prior P7.x
  pass in this project.

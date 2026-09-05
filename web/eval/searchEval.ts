/**
 * 検索の固定評価ハーネス。
 *
 * # なぜ要るか
 *
 * これまでこのプロジェクトの検索・クラスタリングの改善は、すべて
 * 「実データを目で見て判断した」で検証されていた。数値は自己申告MSCに
 * 対するNMIと純度だけで、しかもどちらも**細かく割るほど上がる**
 * （アーキテクチャ.txt に明記のとおり）。つまり「検索は良くなったのか」を
 * 答える物差しが1つも無かった。
 *
 * 実際にこの欠落が効いた例がある: 概念のベクトルを文字列embeddingから
 * 文脈ベクトルへ替えたとき、MSC-NMIも純度もほぼ動かなかった（0.63 /
 * 88%前後のまま）のに、出力の中身は別物になった——前者は
 * "elliptic curve" の綴り10種のクラスタを作り、後者は
 * "finite field / elliptic curves / number field / abelian varieties" と
 * いう数論のクラスタを作る。**既存の指標はこの違いを1ビットも
 * 捉えていなかった。**
 *
 * # 何を測るか
 *
 * ページ最上段の統合検索が実際に呼ぶ経路（`hybridSearch` の
 * exact → specialization → closest を詰めた列）に対して:
 *
 *   MRR@10   正解が何位に来たかの逆数の平均。1.0で常に1位。
 *   dead率   4段階すべてが空で返った割合。「該当なし」の行き止まり。
 *   Top-1率  1位が正解だった割合。
 *
 * 正解は `queries.json` に人手で書いた `accept`（コーパスに実在する
 * 表記だけ）。実在しない語を期待値に書くと、直しようのない失敗として
 * 永久に赤くなるだけで何も測れないため。
 */
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import { buildConceptSearchIndex, hybridSearch } from "../src/hybridSearch";
import type { RelatedEdge, RelatedEdgesExport, SearchEntry, TaxonomyExport } from "../src/types";

const here = dirname(fileURLToPath(import.meta.url));
// eval/dist/ から実行されるので2つ上がって web/ に戻る。
const webRoot = resolve(here, "..", "..");

// `form`/`msc`/relationKind はPhase 0で追加した層別化用の任意フィールド（docs/DATA_DICTIONARY.md）。
// 現行のスコアリング・レポートは参照しない — 追加のみで挙動は変えない。
type EvalQuery = {
  q: string;
  accept: string[];
  why: string;
  form?: "single-concept" | "alias" | "typo" | "natural-language" | "word-order" | "compound" | "casing";
  msc?: string;
};
type RelationFixture = {
  related: { pairs: [string, string, string, string?][] };
  unrelated: { pairs: [string, string][] };
  variants: { pairs: [string, string][] };
};

function expandSearchIndex(columns: TaxonomyExport["searchIndex"]): SearchEntry[] {
  const out = new Array<SearchEntry>(columns.phrase.length);
  for (let i = 0; i < columns.phrase.length; i++) {
    out[i] = {
      phrase: columns.phrase[i],
      docFreq: columns.docFreq[i],
      mscCode: columns.mscCode[i],
      clusterId: columns.clusterId[i],
      fieldConcentration: columns.fieldConcentration[i],
    };
  }
  return out;
}

function expandRelated(columns: RelatedEdgesExport): Record<string, RelatedEdge[]> {
  const map: Record<string, RelatedEdge[]> = {};
  for (let i = 0; i < columns.source.length; i++) {
    const targets = columns.targets[i];
    const scores = columns.scores[i];
    const edges = new Array<RelatedEdge>(targets.length);
    for (let j = 0; j < targets.length; j++) edges[j] = { phrase: targets[j], score: scores[j] };
    map[columns.source[i]] = edges;
  }
  return map;
}

function main(): void {
  const taxonomy = JSON.parse(
    readFileSync(resolve(webRoot, "public", "taxonomy.json"), "utf8"),
  ) as TaxonomyExport;
  const related = expandRelated(
    JSON.parse(readFileSync(resolve(webRoot, "public", "taxonomy.related.json"), "utf8")),
  );
  const fixture = JSON.parse(readFileSync(resolve(webRoot, "eval", "queries.json"), "utf8")) as {
    queries: EvalQuery[];
  };

  const index = buildConceptSearchIndex(expandSearchIndex(taxonomy.searchIndex));

  let reciprocalSum = 0;
  let dead = 0;
  let top1 = 0;
  const failures: string[] = [];

  for (const item of fixture.queries) {
    const result = hybridSearch(item.q, index, related, 10);
    // 統合検索が画面へ詰めるのと同じ順序（dynamicTaxonomy.ts::topHits）。
    const ranked = [...result.exact, ...result.specialization, ...result.closest]
      .map((h) => h.phrase)
      .slice(0, 10);
    const anyTier =
      result.exact.length + result.sameConcept.length + result.specialization.length +
      result.closest.length + result.related.length;
    if (anyTier === 0) dead++;

    const accept = new Set(item.accept.map((a) => a.toLowerCase()));
    const rank = ranked.findIndex((p) => accept.has(p.toLowerCase()));
    if (rank === 0) top1++;
    if (rank >= 0) {
      reciprocalSum += 1 / (rank + 1);
    } else {
      failures.push(
        `  "${item.q}" → ${ranked.length === 0 ? "(0件)" : ranked.slice(0, 3).join(" / ")}` +
          `   期待: ${item.accept.join(" | ")}`,
      );
    }
  }

  const n = fixture.queries.length;
  console.log(`検索評価（${n}クエリ、索引${taxonomy.searchIndex.phrase.length}件、` +
    `論文${taxonomy.paperCount.toLocaleString()}件）`);
  console.log(`  MRR@10   ${(reciprocalSum / n).toFixed(4)}`);
  console.log(`  Top-1率  ${((100 * top1) / n).toFixed(1)}%  (${top1}/${n})`);
  console.log(`  dead率   ${((100 * dead) / n).toFixed(1)}%  (${dead}/${n})`);
  if (failures.length > 0) {
    console.log(`\n10位以内に正解が出なかったクエリ（${failures.length}件）:`);
    for (const f of failures) console.log(f);
  }

  evaluateRelations(taxonomy, related);
  evaluateTypedRelations();
}

/**
 * 概念どうしの距離が数学として正しいかを測る。
 *
 * クエリ評価（上）が測るのは「打った語に辿り着けるか」で、これは
 * `queryIndex.ts` の解釈層がほぼ決める。概念ベクトルを差し替えても
 * ほとんど動かない。**旧実装の致命的な欠陥（関連概念が綴りの変種で
 * 埋まる）を捉えられるのはこちらの指標だけ。**
 *
 * 実際、同じコーパス・同じEntity Resolution・同じクラスタリングで
 * ベクトルだけを替えたA/Bでは、既存の指標が**旧方式の方を高く**評価した:
 *
 *   文字列embedding  MSC-NMI 0.6335 / MSC純度 88.8%
 *                    → 最大クラスタ = "hopf algebra" の綴り8種
 *   文脈ベクトル      MSC-NMI 0.6299 / MSC純度 87.9%
 *                    → 最大クラスタ = initial data / cauchy problem /
 *                      weak solutions / schrödinger equation（PDE分野）
 *
 * NMIも純度も、綴りの分類器と分野の分類器を1ビットも区別できていない。
 */
function evaluateRelations(
  taxonomy: TaxonomyExport,
  related: Record<string, RelatedEdge[]>,
): void {
  const fixture = JSON.parse(
    readFileSync(resolve(webRoot, "eval", "relations.json"), "utf8"),
  ) as RelationFixture;

  const clusterOf = new Map<string, number | null>();
  for (let i = 0; i < taxonomy.searchIndex.phrase.length; i++) {
    clusterOf.set(taxonomy.searchIndex.phrase[i], taxonomy.searchIndex.clusterId[i]);
  }
  const relatedTo = (a: string): Set<string> => new Set((related[a] ?? []).map((e) => e.phrase));
  const sameCluster = (a: string, b: string): boolean => {
    const ca = clusterOf.get(a);
    const cb = clusterOf.get(b);
    return ca !== undefined && ca !== null && ca === cb;
  };
  // 「繋がっている」= 相手が関連概念に出るか、同じクラスタにいるか。
  // 方向は問わない（近傍は上位k件で切るので非対称になりうる）。
  const connected = (a: string, b: string): boolean =>
    relatedTo(a).has(b) || relatedTo(b).has(a) || sameCluster(a, b);

  let hit = 0;
  const missed: string[] = [];
  for (const [a, b, why] of fixture.related.pairs) {
    if (connected(a, b)) hit++;
    else missed.push("  " + a + " — " + b + "（" + why + "）");
  }

  let falsePositive = 0;
  const wrong: string[] = [];
  for (const [a, b] of fixture.unrelated.pairs) {
    if (connected(a, b)) {
      falsePositive++;
      wrong.push("  " + a + " — " + b);
    }
  }

  // 綴りの変種が「関連概念」段階に出ているか（同じクラスタなら正しいので数えない）。
  let leaked = 0;
  const leaks: string[] = [];
  for (const [a, b] of fixture.variants.pairs) {
    if (relatedTo(a).has(b) || relatedTo(b).has(a)) {
      leaked++;
      leaks.push("  " + a + " — " + b);
    }
  }

  const nr = fixture.related.pairs.length;
  const nu = fixture.unrelated.pairs.length;
  const nv = fixture.variants.pairs.length;
  console.log("");
  console.log("概念関係の評価");
  console.log("  関連の再現率   " + pct(hit, nr) + "   数学として関連する対が繋がっているか");
  console.log("  無関係の誤結合 " + pct(falsePositive, nu) + "   繋がってはいけない対");
  console.log("  変種の漏れ     " + pct(leaked, nv) + "   綴り違いが「関連概念」に出ていないか");
  report("繋がらなかった関連対", missed);
  report("誤って繋がった無関係対", wrong);
  report("関連概念に漏れた綴り変種", leaks);
}

/**
 * 型付き関係（`taxonomy.relations.json`、`relations.rs`）の自動チェック。
 *
 * この出力は手動サンプル84件で精度を実測した（約36%、`relations.rs`
 * 冒頭コメント）——正誤の判定は人間にしかできない領域なので、ここで
 * 自動的に測れるのは精度ではなく**一貫性**: 同じ2概念が矛盾する向きで
 * 同時に「特殊化」と判定されていないか（AがBの特殊化、かつBもAの
 * 特殊化、という循環）。`classify_pair`はinvCLの比較で片方向しか
 * 返さない設計だが、Hearst経路は論文ごとに独立なので、別々の論文が
 * 矛盾する文を書けば理論上は循環しうる——実際に起きていないかを
 * 数字で確認する。
 */
function evaluateTypedRelations(): void {
  const path = resolve(webRoot, "public", "taxonomy.relations.json");
  let raw: unknown;
  try {
    raw = JSON.parse(readFileSync(path, "utf8"));
  } catch {
    console.log("\n型付き関係: taxonomy.relations.json が無い（`relations`未実行）ためスキップ");
    return;
  }
  const rel = raw as {
    subject: string[];
    object: string[];
    kind: string[];
    status: string[];
  };

  const specializationPairs = new Set<string>();
  let cycles = 0;
  const cycleExamples: string[] = [];
  for (let i = 0; i < rel.subject.length; i++) {
    if (rel.kind[i] !== "specialization_of") continue;
    const forward = `${rel.subject[i]} ${rel.object[i]}`;
    const backward = `${rel.object[i]} ${rel.subject[i]}`;
    if (specializationPairs.has(backward)) {
      cycles++;
      if (cycleExamples.length < 10) cycleExamples.push(`  ${rel.subject[i]} ⊂⊃ ${rel.object[i]}`);
    }
    specializationPairs.add(forward);
  }

  const byStatus: Record<string, number> = {};
  const byKind: Record<string, number> = {};
  for (let i = 0; i < rel.subject.length; i++) {
    byStatus[rel.status[i]] = (byStatus[rel.status[i]] ?? 0) + 1;
    byKind[rel.kind[i]] = (byKind[rel.kind[i]] ?? 0) + 1;
  }

  console.log("\n型付き関係の一貫性チェック");
  console.log(`  総数 ${rel.subject.length}（confirmed ${byStatus.confirmed ?? 0} / grounded ${byStatus.grounded ?? 0}）`);
  console.log(`  内訳 specialization_of ${byKind.specialization_of ?? 0} / equivalent_to ${byKind.equivalent_to ?? 0}`);
  console.log(`  矛盾する循環（A⊂BかつB⊂A） ${cycles}件`);
  report("矛盾する循環の例", cycleExamples);
}

function pct(k: number, n: number): string {
  return ((100 * k) / n).toFixed(1).padStart(5) + "%  (" + k + "/" + n + ")";
}

function report(label: string, lines: string[]): void {
  if (lines.length === 0) return;
  console.log("");
  console.log(label + "（" + lines.length + "件）:");
  for (const l of lines) console.log(l);
}

main();

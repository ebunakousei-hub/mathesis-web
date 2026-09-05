import { defineConfig } from "vite";
import { resolve } from "node:path";

/**
 * 評価ハーネス（`eval/searchEval.ts`）をNodeで実行できる1ファイルへ束ねる
 * だけの設定。
 *
 * なぜバンドルが要るか: 検索の実体は `src/queryIndex.ts` と
 * `src/hybridSearch.ts` にあり、これらは拡張子なしの相対import
 * （`./queryIndex`）で書かれている。Node 24は型注釈こそ剥がせるが
 * この解決はできないので、評価だけのために `.ts` を付けて回るより、
 * 既に依存にあるViteで束ねる方が、**利用者が実際に使うコードそのもの**を
 * 測れる。評価対象が本物のコードでなければ、評価は何の保証にもならない。
 */
export default defineConfig({
  build: {
    ssr: true,
    outDir: "eval/dist",
    emptyOutDir: true,
    target: "node20",
    rollupOptions: {
      input: resolve(__dirname, "eval/searchEval.ts"),
      output: { entryFileNames: "searchEval.js", format: "esm" },
    },
    minify: false,
  },
});

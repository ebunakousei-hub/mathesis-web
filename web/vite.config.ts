import { defineConfig } from "vite";

export default defineConfig(({ command }) => ({
  root: ".",
  // GitHub Pagesはprojectサブパス（https://<user>.github.io/mathesis-web/）で
  // 配信するため、本番ビルドだけbaseを付ける——devサーバー（`npm run dev`）は
  // これまでどおりルート`/`のまま（PA.1、web/README.mdの旧`--base`手動指定を
  // CI再現性のためconfig化）。
  base: command === "build" ? "/mathesis-web/" : "/",
  server: {
    fs: {
      // wasm-pack の出力（../crates/mathesis-wasm/pkg）はワークスペース外なので明示的に許可する
      allow: [".."],
    },
  },
  optimizeDeps: {
    // wasm-bindgen が生成する ES モジュールは事前バンドル対象から外す
    exclude: ["mathesis-wasm"],
  },
}));

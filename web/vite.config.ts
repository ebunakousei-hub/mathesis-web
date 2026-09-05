import { defineConfig } from "vite";

export default defineConfig({
  root: ".",
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
});

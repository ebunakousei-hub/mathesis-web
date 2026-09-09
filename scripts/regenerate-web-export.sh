#!/usr/bin/env bash
# 改善点.txt項目4: 生成物の陳腐化を「思い出して手動で検証」ではなく、
# 「再生成と検証を1コマンドで不可分にする」ことで構造的に防ぐ。
#
# これまでの実際の障害（P6.3で`RelationEdge`に`traversalPolicy`を
# 追加したのに`relations.json`を再生成し忘れ、P7.2のDefinition-of-Done
# 確認まで誰も気づかなかった）は、`web-export`と`verify-release`が
# 別々のコマンドで、間に「再生成し忘れる」余地があったことが原因。
# このスクリプトはその2つ（と前段の`reconcile`）を常に同じ順序・
# 同じパスで実行し、`verify-release`が失敗したら非ゼロ終了する——
# 「生成した」と「検証を通った」が同じ1回の実行の中でしか成立しない。
#
# 使い方: リポジトリルートから `bash scripts/regenerate-web-export.sh`
# 本番`scratch/provenance.db`をこのスクリプト自身が書き換えることは
# ない（reconcile/web-exportはDBを読むだけ）——web/public/の出力ファイル
# だけを上書きする。

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

RELEASE_TAG="v0-baseline-20260905"
PROVENANCE_DB="scratch/provenance.db"
GRAPH_DB="scratch/judgments.db"
TAXONOMY_DB="scratch/papers_100k_fc.db"
OUT_DIR="web/public"
BIN="target/release/mathesis-provenance"

if [ ! -f "$PROVENANCE_DB" ]; then
  echo "error: $PROVENANCE_DB が見つかりません（本番DBはgit管理外・ローカル専用）" >&2
  exit 1
fi

echo "--- building mathesis-provenance (release) ---"
cargo build --release -p mathesis-provenance

echo "--- reconcile: sidecars + assertions.json + provenance-manifest.json ---"
"$BIN" reconcile \
  --graph-db "$GRAPH_DB" \
  --taxonomy-db "$TAXONOMY_DB" \
  --provenance-db "$PROVENANCE_DB" \
  --release "$RELEASE_TAG" \
  --out-dir "$OUT_DIR"

echo "--- web-export: dependencies.json / morphisms.json / relations.json ---"
"$BIN" web-export \
  --db "$PROVENANCE_DB" \
  --release "$RELEASE_TAG" \
  --out-dir "$OUT_DIR"

echo "--- verify-release: fail loudly here, not later, if anything is stale ---"
"$BIN" verify-release \
  --manifest "$OUT_DIR/provenance-manifest.json" \
  --judgments-provenance "$OUT_DIR/judgments.provenance.json" \
  --relations-provenance "$OUT_DIR/taxonomy.relations.provenance.json" \
  --provenance-db "$PROVENANCE_DB" \
  --web-export-manifest "$OUT_DIR/web-export-manifest.json" \
  --web-export-dir "$OUT_DIR" \
  --release "$RELEASE_TAG" \
  --graph-db "$GRAPH_DB" \
  --taxonomy-db "$TAXONOMY_DB"

echo "--- OK: regenerated and verified in one pass ---"

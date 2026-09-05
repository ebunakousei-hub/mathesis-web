# Mathesis

Lean/Coq の証明支援系や arXiv から数学的知識を構造化し、検索・可視化・
関係抽出を行う研究用探索インターフェース。Google的な網羅検索サービスでは
なく、「この関係は何を根拠に、どの抽出/バージョンで、どれだけ確からしいか」
に答える検証可能なリサーチコックピットを目指している。

- **ライブ版**: https://ebunakousei-hub.github.io/mathesis-web/
- **現行アーキテクチャの一次情報**: [web/README.md](web/README.md)
  （検索パイプライン、クラスタリング、系譜ビュー、eval harness、デプロイ手順）
- **次期アーキテクチャ（移行計画）**: [ARCHITECTURE_NEXT.md](ARCHITECTURE_NEXT.md)
  （証拠ベースの拡充レイヤーへの再設計。外部コーパスの再利用と、
  関係の種類・確信度の統一語彙を導入する）
- **リリース履歴・コーパス件数**: [docs/RELEASES.md](docs/RELEASES.md)
- **関係語彙のデータディクショナリ**: [docs/DATA_DICTIONARY.md](docs/DATA_DICTIONARY.md)
- **Phase 1が今どこまで保証しているか**: [docs/P1_STATUS.md](docs/P1_STATUS.md)
  （証拠層の完全性ゲート`mathesis-provenance verify`が何を検証済みで、
  何がまだ保証されていないかを明記）
- **Phase 2: Web読み取りモデルの生成元**: [docs/P2_STATUS.md](docs/P2_STATUS.md)
  （`dependencies.json`/`morphisms.json`/`relations.json`を証拠層から
  直接生成するようになった変更、2026-09-05）
- **Phase 3: 型付きエンティティカタログ**: [docs/P3_STATUS.md](docs/P3_STATUS.md)
  （`subject_ref`/`object_ref`を実在のEntityへ解決する、追加的な最初の増分）

このファイルはルート直下のクレート構成の見取り図。各クレートの詳細な
設計判断は各 `src/lib.rs` 冒頭のドキュメントコメントを参照。

## クレート構成

| Crate | 役割 |
|-------|------|
| `mathesis-ast` | 層1: 数式のAST化、α正規化（de Bruijn）、Canonical Hash |
| `mathesis-graph` | 層2〜4: 判断ノード永続化、射（含意・特殊化・一般化・同値）、戦略・失敗履歴 |
| `mathesis-lean-parse` | Lean 4宣言（theorem/lemma/def/axiom/instance/example）の判断ノード抽出（ブラウザ/WASM兼用の純粋関数） |
| `mathesis-importer` | CLI: Lean/CoqファイルをGraphStoreへインポート、判断間の依存関係抽出 |
| `mathesis-annotate` | CLI: 層3のエッジ（射）を人間が対話的に承認・棄却するアノテーションツール |
| `mathesis-wasm` | ブラウザ向けカーネル（層1+簡易層2）。rusqlite非依存の純粋Rustデータ構造 |
| `mathesis-msc` | MSC2020（Mathematics Subject Classification）をseed ontologyとして読み込む |
| `mathesis-ingest` | arXivメタデータ取り込み（title/abstract/authors/categories/msc-class） |
| `mathesis-taxonomy` | 候補概念フレーズ抽出、embedding、クラスタリング、関係抽出（Hearstパターン等） |
| `mathesis-fulltext` | arXiv LaTeXソースの取得、定理環境・証明・依存関係・引用の抽出 |
| `mathesis-server` | `web/dist` を埋め込んだローカルHTTPサーバ（単体exe） |

## 開発

```bash
# Rustワークスペース全体のテスト
cargo test --all

# フロントエンド
cd web && npm install && npm run dev

# 固定評価セット（検索精度・概念関係の回帰チェック）
cd web && npm run eval
```

## ライセンス

MIT

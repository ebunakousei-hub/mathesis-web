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
- **Phase 4: OpenAlexアダプタ**: [docs/P4_STATUS.md](docs/P4_STATUS.md)
  （実データでは種論文138件が互いに引用し合っておらず、引用リンクは0件——
  実装は正しく検証済み、結果は正直に記録。計画は[docs/P4_PLAN.md](docs/P4_PLAN.md)）
- **Phase 5: `traversalPolicy`配線・`EntityId`をFKとして本採用**:
  [docs/P5_STATUS.md](docs/P5_STATUS.md)
  （項目1: 既定トラバース対象の辺が実データで0件——回帰を避けつつ
  「信頼できる辺だけ」トグルを追加。項目2: `subject_ref`/`object_ref`
  文字列ではなく`subject_entity_id`/`object_entity_id`(FK)を
  `web_export.rs`/`assertion_export.rs`/カタログカバレッジ集計の真実の
  記録として採用——実データで表記ゆれにより孤立していた概念関係3件が
  可視化された。項目3(宣言的`RelationSchema`)は
  [docs/P5_PLAN.md](docs/P5_PLAN.md)のまま未着手）
- **Phase 6: 本物のLean elaborator由来の信頼できる依存辺**:
  [docs/P6_STATUS.md](docs/P6_STATUS.md)
  （テキスト抽出ではなく`Lean.Expr.getUsedConstants`で型検査済みの証明項
  から機械的に取り出した`depends_on`(`epistemic_state: observed`)。実際
  にDeGiorgi論文の一部を`lake build`し、実データで680件を取り込み、
  既存のテキスト抽出と突き合わせ(一致611・text-only 32・checker-only
  69、両方とも実例で理由を確認済み)。「既定トラバース対象だけ」トグルが
  初めて空でなくなった）
- **Phase 6.1: checker-derived依存の定義を厳密化**:
  [docs/P6_1_STATUS.md](docs/P6_1_STATUS.md)、フィルタポリシーは
  [docs/LEAN_DEPENDENCY_POLICY.md](docs/LEAN_DEPENDENCY_POLICY.md)
  （`getUsedConstants`の生出力から自動生成物(再帰子・matcher・等式補題・
  private詳細)を除くフィルタを定義・実装。11件の対抗フィクスチャで検証、
  実データで2件の実バグを発見・修正(モジュール帰属の取り違え、
  `.rec`/`.mk.inj`等5述語のどれも捕まえない生成物)。取り込み件数は
  551件(680から減——生成物として誤って数えられていた宣言・辺が除かれた
  結果)。依存の辺をクリックして根拠(Lean/mathlib版・型/値のどちらから
  見つかったか・「最小依存の証明ではない」という明示)を開けるUIを新設
  ——実装したところ`reconcile`がchecker-derived辺のassertion idを
  一度も拾っていなかった実バグも発見・修正）
- **Phase 6.2: 3プロジェクトでのLean抽出比較調査**:
  [docs/P6_2_STATUS.md](docs/P6_2_STATUS.md)
  （P6.1のフィルタがDeGiorgi固有の慣習に過剰適合していないかを検証。
  Mathlib自身の2つの部分木(型クラス階層が濃い`Algebra.Order.Group`、
  名前空間・生成物が濃い`CategoryTheory.Category`)へ同じロジックを
  無改造のまま適用——1回目の実行で早速、DeGiorgi固有の前提(宣言名が
  ファイルパスと同じ名前空間を持つ)に依存した過剰適合を発見・修正。
  3プロジェクトとも2回実行してbyte-stableを確認、DeGiorgi側の551件は
  無変化。text-only/checker-onlyの食い違いの原因はプロジェクトごとに
  異なり(汎用識別子の衝突・密なファイル内での近接誤帰属・匿名
  コンストラクタ記法によるテキスト側の不可視性)、1つの数字にまとめない）
- **Phase 6.3: 本人確認済みレビューとリリースゲート**:
  [docs/P6_3_STATUS.md](docs/P6_3_STATUS.md)
  （`review_decisions`は既存(旧称Priority 2)だったが、資格
  (`authorization_level`)・失効・リリース一致まで見る「本人確認済み
  accept」の定義(`review::is_authenticated_accept`)と、それをリリース
  ゲートへ確実に反映する経路が無かった——決定的だったのは、意味的関係を
  実際に`reviewed`へ昇格させるCLI(`promote-review`)自体が存在せず、
  本番DBのreview_decisionsが0件・reviewed/verified状態のassertionが
  0件だったこと。実装後、本番データで実際に1件昇格・
  `verify-release`通過・UI表示まで確認してから元に戻した(内容の採否は
  ユーザーの判断であり、機構の検証と混同しない)。過程で`relations.json`
  生成の実バグ2件を発見・修正）
- **Phase 7: Math-Graphのスコープ限定オフライン取り込みパイロット**:
  [docs/P7_STATUS.md](docs/P7_STATUS.md)
  （`uw-math-ai/math-graph`(CC BY 4.0、HuggingFace APIで確認済み)の
  LeanGraphを、P6.2と同じ2つのMathlib名前空間だけに絞って隔離DBへ試験
  取り込み——生API(`api.theoremsearch.com`、無ライセンス)には一切触れず、
  本文テキストも取り込まない(graph structure first)。`epistemic_state:
  extracted`(`observed`ではなく意図的な信頼ポリシーの選択)により既定
  トラバース対象化を防止。P6.2自身のMathlib抽出と突き合わせたところ、
  同じ5ファイルを覆っていながら宣言名の一致はわずか1件——ファイルパス
  一致 対 推移的import到達という、抽出方針そのものの違いとして報告
  （優劣の主張はしない）。本番`scratch/provenance.db`には一切触れていない）
- **Phase 7.1: なぜ62/63件が一致しないかの整合性調査**:
  [docs/P7_1_STATUS.md](docs/P7_1_STATUS.md)
  （テキストgrepでは`@[simps]`由来の自動生成宣言や無名instanceの実在を
  判定できないと分かり、P6.2と全く同じimportからLean elaboratorの環境
  そのものを生ダンプする手法に切り替えて実測。29%(18件)は
  `Factorisation.lean`/`RelCat.lean`/`Action/Synonym.lean`が
  P6.2のエントリから推移的に一切ロードされていないという直接証拠——
  ファイルパス一致 対 推移的import到達の実証。フィルタ・正規化・
  リビジョン差は0件(生成物除外は一度も働く機会が無かった)。残り70%
  (44件)は新しいカテゴリ——Math-Graphの型クラス階層系宣言は、Leanの
  `env.constants`が持つ生の宣言名ではなく、解決済み型クラス階層の
  各段を独自に列挙しているらしいと実データで判明(例:
  `OrderDual.instMonoid`等50件 対 実際に存在する12件の束ねられた
  instance)——データからの推論であり、Math-Graph自身のコードで確認は
  していないと明記。証拠パネルに「Mathesis自身の抽出か外部データセット
  かの出典説明」(`source_kind_label`)を追加、ブラウザで実際にレンダ
  リング確認済み）
- **Phase 7.2: 到達可能性ギャップの修正と拡張比較**:
  [docs/P7_2_STATUS.md](docs/P7_2_STATUS.md)
  （P6.2のフィルタリングロジックは一字一句変えず、未到達だった3ファイル
  へのimportだけを追加した新規ファイルで抽出——`env.header.moduleNames`
  で実際にロードされたことを直接確認、2回実行してbyte-stable。
  63件中62件不一致だったP7.1の内訳を再検証: 7件はファイルが読めるように
  なっただけの実在宣言、11件はLeanの`'`/`''`衝突回避接尾辞をMath-Graph
  側が`_1`/`_2`へ正規化しているだけの実在宣言(実ソースで1件ずつ確認)、
  1件はLeanの自動命名規則へ正規化された実在instance——一致率は1/63から
  20/63(32%)へ。残り43件は「未確認の外部意味論」と明記し、名前の類似
  だけで対応関係を推測しない。副産物として本番`web/public/relations.json`
  がP6.3の`traversalPolicy`追加以降再生成されていなかった実バグを発見・
  修正(データは無変化、追加フィールドのみ)。継続/停止の判断は引き続き
  ユーザー待ち）

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

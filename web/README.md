# Mathesis Web (TypeScript フロントエンド)

Rust カーネル（`mathesis-ast`）を WebAssembly にコンパイルし（`crates/mathesis-wasm`）、
ブラウザ上で数式の正規化・ハッシュ化をライブに行う TypeScript UI。

## なぜ `mathesis-graph`（SQLite）ではなく `mathesis-wasm` なのか

`mathesis-graph::GraphStore` は永続化に rusqlite（バンドルされた C 製 SQLite）を
使っており、`wasm32-unknown-unknown` へのコンパイルには C コンパイラ（clang）が
必要で、この環境では実際に失敗することを確認した。ブラウザの1セッションは
「開いている間だけ生きていればよい」探索用途なのでファイル永続化は本質的に
不要と判断し、`mathesis-wasm` は `mathesis-graph` を経由せず、`mathesis-ast`
（C依存ゼロ）だけを使う独自の軽量インメモリ実装を持つ。層3〜5（射の型付け・
同値類の縮約・戦略タグ・推論エンジン）も同様の方針で、`crates/mathesis-wasm/src/layer3.rs`
に`mathesis-graph`（`morphism.rs`/`quotient.rs`/`inference.rs`）と同じ
アルゴリズム（Union-Find縮約・エッジ合成則・最短導出パスBFS）を独立した
純粋Rustモジュールとして再実装している——SQLite永続化やProposed/Accepted/
Rejectedの3状態は持たず、射は常に人間が明示的に追加したAccepted相当として
扱う簡易版。今後 `mathesis-graph` 側の SQLite 依存を Cargo feature で
任意化すれば、この簡易版をあちらの本実装に置き換えられる（引き続き将来課題）。

## ビルド手順

```bash
# 1. Rust カーネルを wasm にビルド（crates/mathesis-wasm/pkg/ に出力される）
cd ../crates/mathesis-wasm
wasm-pack build --target web --out-dir pkg

# 2. フロントエンドの依存関係をインストール（pkg/ をローカル依存として解決する）
cd ../../web
npm install

# 3. 開発サーバー
npm run dev

# 4. 本番ビルド
npm run build
```

`mathesis-wasm` の Rust コードを変更したら、手順1をやり直してから `npm run dev`
を再起動する（Vite は wasm パッケージの変更を自動検知しない）。

**注意（2026-09-05、Web公開前の監査で発見）**: ワークスペース共通の
`Cargo.toml`は`debug = 1`（flamegraph用、CLIバイナリ向け）としており、
このままだと`wasm-pack build --release`が生成する`.wasm`にビルド
マシンの絶対パス（Windowsユーザー名を含む、`file!()`由来のパニック
位置文字列）がそのまま埋め込まれる——ブラウザへ配信する成果物として
個人情報の漏洩になる。`crates/mathesis-wasm/.cargo/config.toml`に
`--remap-path-prefix`を設定済みなので、このディレクトリから
`wasm-pack build`を実行する限り自動的に絶対パスが`~`に置換される
（ワークスペース全体の`strip`設定は変更していない——CLIバイナリの
flamegraph用途に影響を与えないため）。`crates/mathesis-wasm`以外の
場所から`cargo build`する場合はこの置換は効かないので、`pkg/`を
再生成する際は必ず`wasm-pack build`を`crates/mathesis-wasm/`
ディレクトリから実行すること。

### Node無しで動かす: `mathesis-server`（単体exe）

日常の開発は上記の`npm run dev`を使うが、「動作確認したいだけなのに
毎回wasm-pack・npm installが要る」のは面倒なので、**ビルド済みの
`web/dist`を1本のexeへ埋め込んで配る**`crates/mathesis-server`を
別途用意した。これを使えばNode.js無しの環境でも実行するだけで
ブラウザが自動で開く。

```bash
# 事前に上の手順1〜4（wasm-pack build → npm install → npm run build）を
# 済ませ、web/dist が最新であることを確認してから:
cd ..
cargo build --release -p mathesis-server
./target/release/mathesis-server.exe
```

`web/dist`は`rust-embed`のマクロが**コンパイル時**に丸ごと読み込むため、
`web/dist`を再ビルドしたら`mathesis-server`も再ビルドしないと古い内容の
ままになる——`npm run dev`のような自動検知は無い。起動すると
`http://127.0.0.1:8787`でHTTPサーバが立ち上がり、既定のブラウザが
自動で開く。大きな静的JSON（`taxonomy.related.json`等）は起動時に
一度だけgzip圧縮してキャッシュし、`Accept-Encoding: gzip`を送る
クライアント（実質全ブラウザ）へ圧縮済みのまま返す——下の
「デプロイ時の必須確認事項」と同じ理由で、単体配布でも圧縮を素通しに
しない。

### デプロイ時の必須確認事項: 静的ホスティングの圧縮

`npm run build`が生成する`dist/`には、`taxonomy.json`（5.30MB）・
`taxonomy.related.json`（10.57MB）のような大きな静的JSONが含まれる。
これは圧縮されて初めて実用的なサイズになる——`vite preview`（本番
バンドルの動作確認用サーバ）で実測したところ、gzipで
**taxonomy.json 5.30MB → 0.94MB、taxonomy.related.json 10.57MB →
2.22MB**まで縮む。逆に`vite dev`（開発サーバ）は非圧縮のまま返す
（`content-encoding: null`）ため、開発中の体感だけを見て「軽い」
「重い」を判断しないこと。

**配備先の静的ホスティングが応答をgzip/brotli圧縮するかどうかで、
実際にネットワークを流れる量が約5倍変わる。** Netlify・Vercel・
Cloudflare Pages・GitHub Pages（Cloudflare等のCDN経由）は`.json`を
既定で圧縮するが、素朴な`nginx`（`gzip on;`が未設定）や
`python -m http.server`のような開発用サーバ、圧縮設定を明示しない
S3バケット直配信では非圧縮のまま出てしまう。デプロイ後は必ず
下記コマンドで確認する:

```bash
curl -sI -H "Accept-Encoding: gzip" https://<配備先>/taxonomy.json | grep -i content-encoding
# => content-encoding: gzip （または br）が出なければホスティング側の設定を直す
```

`nginx`なら`gzip on; gzip_types application/json;`、Apacheなら
`mod_deflate`を有効にする。ビルド時に`.json.gz`を事前生成して
static-compression（`vite-plugin-compression`等）で配る方法もあるが、
配備先が確実に圧縮を返すなら二重に持つ必要はない——まずは配備先の
既定動作を上記コマンドで確認するのが先。

## 公開（GitHub Pages、2026-09-05）

ユーザー指示「インターネット上に公開してください。個人情報につながる
ものはすべて削除して下さい」に対応した記録。

**サイズの都合でArtifact機能は使えなかった**: `web/public`配下の
静的JSON合計が約45MB（`taxonomy.papers.json` 19.3MB・
`taxonomy.related.json` 12.9MB・`taxonomy.json` 8.4MB・その他）で、
Claudeのartifact機能の上限（1ページ16MB）を超える。ユーザーに確認の
上、認証済みのGitHubアカウントでGitHub Pagesとして公開する方式を
選んだ。

**個人情報の監査**: 公開前に`web/src`・`web/public`・ビルド成果物
（`web/dist`）を"feruy"（Windowsユーザー名）・メールアドレス・
ローカルの絶対パスで走査した。唯一見つかったのが上記の
`mathesis_wasm_bg.wasm`のパニック位置文字列で、`.cargo/config.toml`の
remap-path-prefixで修正した後、`web/dist`全体を再走査してゼロ件を
確認してから公開した。

**公開範囲**: `web/dist`（ビルド済み静的サイト）だけを別リポジトリ
`ebunakousei-hub/mathesis-web`へpushした——Rustソース・scratch DB・
アーキテクチャ.txt等の内部設計メモは含めていない（Mathesis本体の
作業ディレクトリ自体はこれまでどおりgit管理外のまま）。コミットの
author情報はGitHubのnoreply形式
（`<id>+<username>@users.noreply.github.com`）を使い、実メールアドレスは
一切コミットに含めていない。

**GitHub Pagesのproject subpath対応**: GitHub Pagesの
`https://<user>.github.io/<repo>/`という配信形態では、`vite build`の
既定（絶対パス`/assets/...`）のままだとアセットが404になる。
`src/dynamicTaxonomy.ts`・`src/proofGraph.ts`・`src/searchWorker.ts`に
あった`fetch("/taxonomy.json")`等のハードコードされた絶対パスを
`` fetch(`${import.meta.env.BASE_URL}taxonomy.json`) `` に修正済み
（ローカル開発時は`BASE_URL`が`/`のままなので従来どおり動く）。

公開URL: **https://ebunakousei-hub.github.io/mathesis-web/**

**PA.1（2026-09-09）以降の再公開手順**: `vite build --base=...`を毎回
手で付ける旧手順は廃止した。`vite.config.ts`が`command === "build"`の
ときだけ`base: "/mathesis-web/"`を自動で付ける（devサーバーはこれまで
どおり`/`のまま）。公開は`.github/workflows/deploy-pages.yml`（GitHub
Actions、Pages source = "GitHub Actions"）が担当し、`web/dist`を
コミットする必要も、ローカルから手動でファイルをアップロードする必要も
無い:

- 通常のpush/PR: `cargo test --all` → `wasm-pack build` → `npm run
  build` → `npm run eval` の検証のみ実行——**デプロイはしない**。
- 実際にデプロイするのは、(a) Actionsタブから該当ワークフローを手動で
  `workflow_dispatch`実行するか、(b) `web-release-*`という名前のタグを
  pushしたときだけ（例: `git tag web-release-2026-09-09 && git push
  origin web-release-2026-09-09`）。毎pushで自動デプロイはしない——
  意図的な、追跡可能なリリースにするため。
- 本番相当のデータ検証（`mathesis-provenance verify-release`）は
  ワークフローには含まれていない——本番`scratch/provenance.db`自体を
  このリポジトリにcommitしていないため（`.gitignore`の`/scratch/`参照）。
  この検証は引き続き、`web/public/*.json`を再生成してcommitする**前**に
  ローカルで実行するリリースゲートのまま（`docs/RELEASES.md`参照）。
  **`web/public/*.json`を再生成するときは`reconcile`/`web-export`を
  手で個別に叩かず、必ず`bash scripts/regenerate-web-export.sh`を使う**
  ——再生成と検証を1コマンドに不可分化し、「再生成し忘れる」余地を
  構造的に無くした（過去に`traversalPolicy`フィールド追加時、実際に
  再生成し忘れて本番に古い`relations.json`が残っていたことがある）。
  詳細は`docs/PA_1_STATUS.md`・`docs/PA_2_STATUS.md`。

### 公開直後の外部レビューで発見・修正した点（2026-09-05・同日）

公開してすぐにユーザーから詳細な外部レビューを受け、「信頼性の境界を
明確にする」ことを最優先(P0)として着手した。コードを実際に読んで
裏取りした結果、2つの発見があった:

- **概念関係（`dynamicTaxonomy.ts`のtyped relations）は元々ちゃんと
  ヘッジされていた**——「同値の可能性がある概念」等のラベルと実測精度
  50%を明記したホバーヒントが既に実装済みだった。
- **Lean側の射（`lineageView.ts`のmorphism chip）は違った**——
  `ExportedMorphism.status`（proposed/accepted/rejected）はJSONに
  含まれ、Rust側のdocコメントも「UI側で未承認の候補であることを明示
  する」と明記していたのに、現在の`lineageView.ts::renderMorphisms`は
  これを一切表示していなかった——旧実装（1ホップのチップ列）から現在の
  系譜ビューへ書き換えた際にステータス表示が抜け落ちていた。実データで
  確認すると、`unitBallApproxEps`が数値定数の型シグネチャ一致だけで
  61件と「同値」表示されており（Phase 10で既知だった限界そのもの）、
  未承認のヒューリスティック提案だと画面上では分からない状態だった。

修正した内容: 射チップへの「未承認/承認済み/却下済み」バッジの復元、
`parseStatus`バッジへの「Leanカーネル検証ではない」ツールチップ追加、
依存関係の行への「識別子の名前一致で機械的に検出」注記、
`proofGraphExplain`説明文の明確化、`generatedAtUnix`（export時のUNIX秒、
`mathesis-taxonomy`/`mathesis-graph`両方のexport構造体に追加）による
データ生成日の表示。

## 現状の機能

- **TeXで数式を打つと、打つそばから組版されて見える**（`src/tex.ts`）。
  検索欄の入力に `\cmd` / `^` / `_` / `$…$` のどれかが含まれればTeXとして
  扱い、すぐ下に組版した式を出す。組版は temml（TeX→MathML）で、
  読み込むのは入力がTeXらしくなった時点だけ（203KB/gzip 60KB を別チャンク
  に分離済み。フォントもCDNではなく同梱の9.4KBのwoff2をビルドに取り込む
  ので、配信物は自己完結したまま）。
  これは見た目だけの機能ではない。同じTeXを**2種類の検索語に翻訳**して、
  性質の違う2つの索引にぶつける:
    - **英単語** → 概念タクソノミー（80,727概念、英語の名詞句）。
      `\int` → "integral"、`\nabla` → "gradient"、`\|·\|` → "norm"、
      `H^1` / `W^{1,p}` → "sobolev space"、`L^2` → "lebesgue space"。
      記号を1文字ずつ訳しても "H" と "1" にしかならないので、関数空間の
      記法は並び全体を1つの概念として拾う。
    - **Unicode記号** → 証明グラフ（Lean由来の1,431判断、命題本文が
      `∀ (n : ℕ), (0 < …)` のようにUnicodeで書かれている）。
      `\mathbb{N}` → `ℕ`、`\leq` → `≤`、`\|·\|` → `‖`。
  翻訳の結果は入力欄の下に札で並べて見せる——当たらなかったときに
  「何で検索されたのか」が分かれば、利用者が自分で式を寄せていける。
  記号→英単語の対応は「その記号を打つ人がその概念を探している」と
  言えるものだけに絞ってある（`\forall`→"all"、`\in`→"member"、
  `\to`→"map" のような汎用語は、入れると検索結果が語の一般的な用法に
  埋まるので入れていない）。
- 検索バーに（TeXではない）素の数式を入力すると、Rust/wasm カーネルが
  その場で α正規化・正規化ハッシュを計算して表示する（層1）。
- **「分野から探す」Explorer（MSC分野）**: `crates/mathesis-taxonomy` の
  Phase 1〜5パイプライン（arXiv収集→抽出→**文脈ベクトル**→クラスタリング→
  MSC2020 alignment）の出力であるMSC分野のカード一覧（`src/dynamicTaxonomy.ts`、
  `web/public/taxonomy.json` を読み込み——書き出しは
  `mathesis-taxonomy export <papers.db> web/public/taxonomy.json`——このコマンドは
  `taxonomy.json` / `taxonomy.related.json` / `taxonomy.papers.json` /
  `taxonomy.aliases.json` の4ファイルを並べて書き出す（後3つは検索時にだけ
  遅延読み込みされる別ファイル）。分野数はコーパス規模で変わる（実データ
  142,948論文規模で60分野、後述）。MSC分野→クラスタ→メンバー概念の
  ドリルダウンと、MSC2020に対応の無い新語彙候補クラスタの一覧を切り替えて
  見られる。MSC名は公式データが英語のみのため、この区画のラベルは常に
  英語表記（誤訳をでっち上げるより素直にそうした）。
  **旧・静的な5大分野の円形マップ（`fields.ts`のMAIN_FIELDS/BRIDGE_FIELDS
  を`explorer.ts`のFieldExplorerで描画、モード切替タブでMSC分野と統合
  していた）はユーザー指示により削除した（2026-09-05）**——実データの
  MSC分野一覧が既にあるので、手書き5分野の見取り図は重複していた。
  `fields.ts`は`findLabelById`（デモ判断ノードの分野タグ表示用）だけを
  残し、円形描画専用だった`fieldAndChildrenIds`と`explorer.ts`本体・
  モード切替の状態（`main.ts`の`explorerMode`/URLの`mode`パラメータ）は
  削除した。
- **統合検索（ページ最上段の検索欄）**: 入力に関係子・演算子が含まれれば
  数式として正規化・ハッシュ化し（従来どおり）、含まれなければ
  **概念タクソノミー（80,727概念）と証明グラフ（1,431判断ノード）の
  両方に同じクエリを流して**上位を並べ、クリックでその区画へ送り込む
  （タブを開き、クエリを渡し、スクロールする）。"weak harnack" のように
  両方に該当があるクエリでは概念とLean判断ノードが同時に出る。
  以前この欄は、入力が何であれ数式パーサに通すだけだった——
  "riemann hypothesis" と打つと「パース状態: 完全」と表示され、
  一見成功したように見えて何も検索していなかった（同じページの下に
  該当概念が実在するのに到達できなかった）。
- **系譜ビュー（`src/lineage.ts` / `src/lineageView.ts`）**: 判断ノードを
  1件選ぶと、そこから下へ伸びる依存の鎖を**図と文章の両方**で出す。
  以前ここに出ていたのは「依存している判断 (5)」「参照している判断 (3)」
  という1ホップぶんのチップの列だけだった——依存グラフは実データで
  最大48段（`holder_Moser` から最下層の定義まで47本の辺）まで伸びている
  のに、利用者は1段ずつ手で辿って頭の中で繋ぎ直すしかなかった。
  「この証明は何に依拠するのかを簡単に追える」というこのサイトの主眼に
  対して、それは答えになっていない。
    - **段への割り付けは根からの最長路**で決める（最短路ではない）。
      最短路で割ると「Aに直接依拠し、かつBを経由してもAに依拠する」形の
      とき辺が段を跨いで水平に走り、絵が読めなくなる。
    - **背骨（spine）**——根から最も深い依存へ至る最長の鎖——を各段の
      左端に固定し、縦一本の太い線として描く。証明の骨格はこの鎖で、
      残りは横から刺さる補題である、という読み方を絵の側で提示する。
      段ごとの重心法だけに任せると背骨が左右に振れて追えなくなるため、
      重心法で段内の順序を決めたあと背骨を先頭へ寄せ、段ごと剛体で
      ずらして揃えている（1件ずつ希望位置へ寄せる方式は、段の左端の
      ノードだけが親に貼り付いて残りが右へ押し出され、絵全体が階段状に
      流れた——実データで26件を描くのに2,040px使い、うち800pxが空白に
      なった。段をまとめて動かす方式で1,244pxに収まっている）。
    - 依存辺（証明が実際に参照した判断）と射（含意・特殊化・一般化・
      同値）を**別種の辺として描き分ける**。射はヒューリスティック提案
      （`status: "proposed"`）なので凡例でそう明示する。
    - **証明の概略**: 同じ背骨を 1. 2. 3. と番号を振った文章の列としても
      出す。各段は畳んだ状態では「次に何に依拠するか＋この段で追加で
      要る補題の数」の1行だけで、押すと命題本文と前提が開く。絵で形を
      掴んでから、気になった段だけ中身を読む、という順で辿れる。
      `holder_Moser` では Hölder連続性 ← Moser代表元の評価 ← 二進平均 ←
      振動の減衰 ← Harnack不等式、というDe Giorgi–Nash–Moserの筋が
      そのまま12段の番号付きリストとして出る。
    - **命題本文の中の識別子がリンクになる**。`MeasurableSet
      (unitBallBadAnnulusOne d)` の `unitBallBadAnnulusOne` はそれ自体が
      グラフ上の判断なので、本文からそのまま依存先へ飛べる——「この式の
      この記号は何だったか」を別の欄を探しに行かずに追える。
    - ノードを押せば選択が移り、「ここを起点にして辿り直す」で根を
      差し替えられる。「もう一段深く」で表示段数を増やせる（既定4段、
      最大12段）。
    - 判断から「関係する概念をarXiv 10万論文から探す」で、Leanの識別子を
      語に割って概念タクソノミー区画へ渡す（`holder_Moser` →
      "holder moser" → "moser" / "hölder" などの概念）。証明グラフ側と
      概念側という、性質の違う2つのデータを繋ぐ導線。
- **Leanを貼り付けて試す（`src/leanPlayground.ts`、`crates/mathesis-lean-parse`）**:
  `theorem`/`lemma`/`def`/`axiom`/`instance`/`example`のLean 4宣言を
  貼り付けると、CLIの`mathesis-importer`と**全く同じパーサー**がその場で
  判断ノードと依存関係を抽出する。以前は「Leanパーサーは今のところCLI
  バイナリ内に閉じている」ことが既知のギャップだった——`mathesis-importer`
  のパース処理は`mathesis-graph::GraphStore`（rusqlite、バンドルされた
  C製SQLite）を直接呼んでおり、これは`wasm32-unknown-unknown`へコンパイル
  できない（`mathesis-wasm`が最初から`mathesis-graph`を経由しない設計に
  なっている理由そのもの）。そこでLeanのテキストを判断ノードへ切り出す
  部分——構文の分割・識別子と文脈の抽出・依存関係の名前解決——を
  `mathesis-graph`に一切依存しない新クレート`mathesis-lean-parse`へ
  移し、CLIと`mathesis-wasm`の両方がそこへ依存する形にした。
  **アルゴリズムは1箇所にしかない**——CLIとブラウザで別々の実装を持つと、
  いずれ挙動がずれて「CLIでは拾えるのにブラウザでは拾えない」ような
  食い違いが起きるため。
    - 切り出しは機械的な移動ではなく検証済み: リファクタ前後でCLIを
      実データ（DeGiorgiコーパス、94ファイル）に対して実行し、書き出した
      `judgments.json`が**バイト単位で完全に一致**することを確認している
      （1,431判断・4,797依存関係、どちらも変化なし）。
    - 貼り付けた判断を1件選ぶと、上の系譜ビュー（`LineageView`）を
      **そのまま再利用**して依存関係を図と概略で見せる——静的な
      `judgments.json`用に作ったコンポーネントが、貼り付けだけで閉じた
      小さなグラフにもそのまま使える形にしてあるため、新規UIはほとんど
      要らなかった。射（含意・特殊化等）は貼り付けだけでは分からないので
      空——静的データにしかない情報である旨は暗黙のうちに区別される
      （射の凡例・辺は単に現れない）。
    - 依存関係の名前解決は**その貼り付けの中だけで閉じる**——CLI版の
      `judgment_dependencies`が1回のインポートバッチ内に閉じるのと同じ
      制約をここでも踏襲している。
    - 既定では空。「実データの例を入れる」ボタンで、このページ自身の
      実データ（`fixtures/arxiv/DeGiorgi/DeGiorgi/BallExtension/Core.lean`
      冒頭の3件の`def`、実際に依存し合っている）を試せる——説明のための
      作り話ではなく実データそのもの。
    - サーバー通信は一切無い。貼り付けた内容はブラウザの外へ出ない。
- **hybrid search（Phase 7）+ クエリ解釈層**: 概念タクソノミー区画の
  検索欄は、exact（完全一致）→ same concept（同じクラスタ）→
  specialization（より具体的な複合語）→ related（embedding近傍）→
  closest（入力語の一部だけ一致）の段階で結果を出し分ける
  （アーキテクチャ.txt 5.7）。`taxonomy.json` の `searchIndex` と
  `relatedEdges` だけで完結し、ブラウザはOllamaと通信しない。
  クエリの受け取りは `web/src/queryIndex.ts` の解釈層を通す——
  大小文字・ダイアクリティカルマーク（Kähler↔kahler）・単複を正規化し、
  "the"/"of"/"what is" のような機能語を落とし、語順を問わず
  （"manifold kahler"→"kähler manifolds"）、索引に無い語は綴りを訂正する
  （"reimann"→"riemann"）。全語が一致しなくても一致語数で順位を付けて
  返すので、行き止まりにならない。「どう解釈したか」は必ず画面に出す。
  これが無かった頃は現実的なクエリの67%が全段階0件だった（実測）。
  検索結果の各行はクリックでその語へ検索クエリをピボットでき、
  「クリックした概念から関連概念へ辿る」（5.7のgraph exploration）を
  簡易に実現している。Rust版CLI（`mathesis-taxonomy search`）は
  完全一致しないクエリをその場でOllama embedding化してrelatedまで埋める。
- デモ用の判断ノード一覧（`KernelStore.seedDemoJudgments()`）: アーキテクチャ
  文書の例（ペアノ公理下の加法の結合律と群論下の結合律は文脈が違うので別
  ノード）をそのまま投入し、実際に別ハッシュになることを見せる。
- **取り込まれた証明グラフ（実データ、Phase 10）**: `crates/mathesis-importer`
  がPhase 9で実際にLean 4コーパスを読み込んだ判断ノード（定理・定義・公理等）
  と、`judgment_dependencies`（証明/定義本体が参照している他の判断）、
  `morphisms`（含意・特殊化・一般化・同値の論理的な射、`--propose-morphisms`
  フラグでヒューリスティックが機械的に提案した`Proposed`＝未承認候補のみ）を
  `web/public/judgments.json`として書き出し（`mathesis-import <入力> <db>
  --paper <arxiv_id> --propose-morphisms --export web/public/judgments.json`）、
  `src/proofGraph.ts`がそれを読み込んで表示する。ソースファイル別の一覧→
  判断ノードの詳細（種別・命題・文脈Γ・パース状態・由来ファイル:行）と
  ドリルダウンでき、詳細画面では「依存している判断」「参照している判断」に
  加えて「論理的な射（層3）」がクリック可能なチップとして並び、そこから
  別の判断へジャンプできる（グラフをたどる簡易ナビゲーション）。射は
  未承認である旨のステータスバッジと根拠テキストを添えて表示し、確定した
  事実であるかのように見せない（DeGiorgiコーパスの実データで検証したところ、
  `(n : ℕ) → ℝ`型の数値定数群のようにステートメントの型シグネチャだけでは
  区別が付かない判断ノード群が実際に「同値」候補として大量に提案されており、
  これは層2の判断モデルが定義の本体（右辺の値）を構造化していないという
  既知の限界の表れ——ヒューリスティックが「未承認」に留めている設計の妥当性を
  実データが裏付けた形）。上のデモ判断ノード一覧（手書き5件）とは別物——
  こちらは実際にLean 4リポジトリ（`fixtures/arxiv/DeGiorgi/`、
  Armstrong–Kempe, arXiv:2604.05984, 約56,000行のDe Giorgi–Nash–Moser理論の
  形式化）から取り込んだ1,431件（依存関係4,797本、射2,284本）**を起点に、
  診断⑥（下記）でarXivのLaTeX論文137件ぶんが加わり、現在は4,052件・
  依存関係5,634本・138論文**。
- **Statementノード（診断⑥、`crates/mathesis-fulltext`）**: 上のLean判断
  ノードは「1論文（DeGiorgi）に固定」だったが、`mathesis-fulltext`が
  arXiv論文のLaTeXソースから定理系環境（`\newtheorem`宣言を読んで
  Theorem/Lemma/Corollary/Proposition/Definition/Claim/Remark/
  Conjecture/Axiom/Example/Instanceを判定）・証明の有無・同一論文内の
  `\ref`依存を抽出し、`mathesis-graph`へ`parse_status: informal`の判断
  ノードとして橋渡しする（`bridge-to-graph`コマンド、`bridge_paper`は
  同じarxiv_idを何度呼んでも安全——`graph`に既に登録済みなら即座に
  何もせず返す）。命題文は`mathesis_ast::Expr::Unparsed`（層1が元々
  持っていた「構造化できない残余テキスト」というフォールバック）に
  生のLaTeXのまま包んで保持し、Web側は`«...»`というギュメで囲んで
  表示する——「これは未パースの生テキストだ」という合図はそのまま
  残しつつ、地の文に埋め込まれた`$...$`・`\(...\)`・`\[...\]`の数式
  区間だけを`renderStatementWithMath`（`src/tex.ts`、検索欄のTeX
  入力と同じtemmlを再利用）で組版する（`\label{...}`のような未解決
  コマンドはそのまま生テキストで残る——数式以外は捏造しない）。この
  分岐は`parseStatus === "informal"`のときだけ有効で、Lean由来の命題
  （Lean構文そのものであってTeXではない）は従来通り識別子リンクの
  表示のまま変えていない。パース状態バッジは`informal`（青、
  full/partial/failedの緑橙赤という成功度軸とは別配色）。

  組版に回す前、`statement_text`自体は`crates/mathesis-fulltext/src/
  macroexpand.rs`が論文自身の`\newcommand`/`\def`/`\DeclareMathOperator`
  宣言を展開済みのテキストになっている——著者が`\def\Zset{{\mathbb Z}}`
  のような略記を定義して命題文中で使い回すのは数学論文ではごく普通で、
  展開しないと温MLは未定義コマンドとして赤いエラー表示にしていた。実測
  （188論文）: informal命題文の71.0%が展開前は未知の`\command`を含んで
  いたが、展開後は25.7%（大半はbabel等パッケージ提供のコマンドで、
  論文自身のソースに宣言が無く展開できないもの——想定通りの残存）。
  2引数以上のマクロ・パラメータ付き`\def`は対象外（既知の制約、
  `macroexpand.rs`冒頭コメント参照）。

  実データ200論文のランダム標本で実行したところ、137論文（73%）から
  定理系環境2,679件・証明つき859件・同一論文内依存844本を抽出、
  `JudgmentKind`に対応する種別のもの**2,621件（97.8%）**を判断ノード
  として取り込んだ（現在は**4,052 judgments・5,634依存関係・138papers**）。
  見送りは当初151件（5.6%）だったが、実データを個別に読んで種別
  マッピングを追加修正し**58件（2.2%）まで削減**した——フランス語の
  `\newtheorem`表示名（"Lemme"→Lemma等）・babel系パッケージの翻訳
  マクロ名（`\theoremname`のようにマクロ名自体が種別を表す慣例、展開
  はできないがマクロ名から読み取れる）に加えて、実データを読んで
  初めて分かった3系統——① 非UTF-8ソースを`from_utf8_lossy`で読んだ
  結果アクセント文字がU+FFFDに化けた具体的な観測形、② 前置1970〜80
  年代のLaTeXアクセント記法（`\'e`等）を使った表示名、③ 長い綴りの
  フォント切替コマンド（`\textbf`・`\rmfamily`等）や`\protect`・
  著者独自の1文字書式マクロが表示名を包んでいて剥がせていなかった
  ケース——を`crates/mathesis-fulltext/src/theorem.rs`
  （`MEANINGLESS_LEADING_COMMANDS`）と`bridge.rs`（`kind_synonym`）に
  追加した。残る58件（Question/Problem/Notation/Hypothesis/Step/
  Condition/Algorithm/Acknowledgment等）は既存の種別と対応しない
  genuinely別の概念で、押し込めず見送るのが引き続き正しい。

  `\cite`による論文をまたぐ依存は、著者名・タイトルの文字列一致には
  頼らず、**`\bibitem`本文に明記された"arXiv:1234.56789"のような
  明示的なID記載だけ**を対象に実装した（2026-09-04）。型も揃えた:
  `\ref`はラベル経由で「同じ論文内の特定の1定理」を指すが、`\cite`が
  指すのは「引用文献という1本の論文全体」なので、`judgment_dependencies`
  （JudgmentId同士の辺）ではなく`mathesis-graph`に新設した
  `paper_citations`（論文ID同士の辺、`paper_citation.rs`）へ張る——
  `GraphExport`の`ExportedPaper.cites`として書き出す。
  実データ（200論文サンプル）での実測: 文献引用3,197件中、明示的な
  arXiv ID記載から解決できたもの90件（2.8%、この年代の論文では伝統的な
  書誌形式が主流だったことを示す）。ただし実際に張れた引用辺は**0本**
  ——解決はできても引用先が同じ200論文のランダム標本には入っていない
  ケースが大半で、これは実装の不具合ではなくサンプル規模の問題
  （詳細は`アーキテクチャ.txt`「論文単位の引用（`\cite`→arXiv ID）の
  実装」参照）。

  arXiv取得の403（当初「一時的なブロックか」と推測していた6/200件）は、
  後日`curl`で個別に再確認したところ同じ6本のarxiv_idが時間を置いても
  User-Agentを変えても一貫して`/src/`エンドポイントで403を返し、同時期の
  他の論文（例: 1706.03762）は問題なく取得できた——**セッション単位の
  一時的なブロックではなく、その特定の論文だけがarXiv側でソース配布を
  制限されている**と考えられる（推測を実測で裏取りし直した）。リトライで
  直る性質のものではないため、リトライ機構は追加していない。
- **証明グラフの多段階検索（`src/proofSearch.ts`）**: 上の区画の検索欄は、
  概念タクソノミー区画のhybrid searchと同じ「段階ごとに意味の違うヒットを
  分ける」構成を判断ノードに対して行う4段階になっている。完全一致 →
  識別子の一致（Leanの識別子を`_`と大文字境界で語に割り、クエリの各語を
  前方一致で照合。`measurableSet_unitBallBadAnnulusOne`のような合成語が
  相手なので、部分文字列一致では「ball measurable」のような複数語クエリが
  1件も当たらなかった）→ 命題・文脈の一致（命題本体と仮定コンテキストの
  中身を検索。旧実装ではここが検索対象ですらなく「`Metric.ball`を含む
  判断」を探せなかった）→ グラフ上の関連（ヒットから依存辺・射で1ホップ
  到達する判断を、何件のヒットから到達したかで順位付け＝クエリの「土台」に
  なっている補題が浮かぶ。各行に「なぜ出てきたか」を辺の種類と起点の名前で
  表示する）。種別（definition/theorem）とパース状態（full/partial）の
  絞り込みチップも付く。実測: "ball measurable"は旧0件→識別子16件+命題
  15件+関連25件、"Metric.ball"は旧0件→命題25件、"weak harnack"は旧0件→
  識別子25件。従来当たっていたクエリ（"harnack"・"MeasurableSet"）も維持。
- **層3〜5デモ（射・戦略・推論、`src/layer35.ts`）**: 上のデモ判断ノード
  一覧（手書き5件）に、`KernelStore`（`mathesis-wasm`）が新たに持つ層3〜5
  メソッド（`addMorphism`/`listMorphisms`/`tagStrategy`/`quotientClasses`/
  `shortestDerivation`）へのUIを提供する。射（含意・特殊化・一般化・同値）を
  2つの判断ノード間に手動で張り（常にAccepted相当）、証明戦略タグ
  （`induction`等、datalistで候補提示）を付け、同値射から自動計算される
  同値類（Union-Find縮約）を確認し、推論エンジンに「fromからtoへの単一の
  導出関係に還元できる最短パス」を問い合わせて、エッジ合成則
  （`Specialization ∘ Implication = Implication`等）で畳み込まれた結果を
  見ることができる。実データ側（証明グラフのProposed候補）とは対照的に、
  こちらは人間が明示的に張った射だけを扱う——「未承認の候補を機械的に大量生産
  する層3」と「人間が意味を検証しながら1本ずつ張る層3」の両方を、同じ
  ドメインモデル（`MorphismKind`の4種）の上で対比して見せている。
- **100,000論文スケールでの実データ接続（2026-09-02）**: `taxonomy.json`は
  実際にarXiv"math"セット全体から収集した100,000論文（`mathesis-ingest`、
  21.9分）を通しで処理した出力（58 MSC分野・6,350クラスタ・候補80,727件。
  `taxonomy.json` 6.6MB ＋ 検索時のみ読む `taxonomy.related.json` 10.6MB）。この規模で初めて顕在化したボトルネック・バグ（クラスタ用
  グラフ構築とhybrid searchのrelated段階が使っていたO(n²)総当たりの
  LSHへの置き換え、embedding生成の並列化とそこで見つかったWindowsの
  エフェメラルポート枯渇バグ、学術文章の定型句がRAKE候補を汚染する
  データ品質問題、等）は`crates/mathesis-taxonomy`側で修正済み——詳細は
  アーキテクチャ.txtの「100,000論文スケールテスト」を参照。
- **分野集中度（field concentration）**: 候補フレーズの品質フィルタを、
  手書きのブロックリストから統計量に置き換えた（`crates/mathesis-taxonomy/
  src/concentration.rs`）。フレーズを含む論文の分野分布とコーパス全体の
  分野分布のKLダイバージェンスを取り、「どの分野でも同じ割合で書かれる語
  ＝執筆の定型句」を落とす。実データ100,000論文で手書き17件→統計739件を
  自動除外（消えた複合語54件は目視で全て定型句、本物の概念の巻き込みゼロ）。
  スコアは`searchIndex`に載っていて、概念タクソノミー区画の検索結果の
  各行に表示される（値にマウスを載せると説明が出る。分野ラベル付き論文が
  50件に満たない語には付かない——80,727件中3,822件がスコアを持つ）。
- **検索結果に出典arXivリンク**: 概念タクソノミー区画の検索結果の各行に、
  その語を実際に使っている論文へのarXivリンクが最大3件付く（実データ
  そのまま、`concept_candidates.sample_arxiv_ids`）。「この概念は誰の
  論文に由来するか」を直接たどれる。以前はこのデータがバックエンド
  （SQLite）にしか存在せず、Web版には出していなかった——`taxonomy.json`
  への直接混入は列形式でも実測5.04MBの追加になり、これまでの配信量削減
  （下記）を大きく後退させるため、`taxonomy.related.json`と同じ扱いで
  `taxonomy.papers.json`という別ファイルに分け、最初の検索時にだけ
  遅延読み込みする。フレーズ本体（クリックで検索語をピボット）と
  arXivリンクは別のクリック領域——`<button>`の中に`<a>`を入れる
  （HTML仕様上不正な入れ子で、クリックがボタン側へ伝播してリンク単体を
  押せなくなる）バグを避けるため、行の構造自体を見直した。

- **配信の分割と遅延読み込み**: `taxonomy.json`（5.3MB）は最初の画面と
  検索に要るものだけを持ち、embedding近傍の辺（10.6MB、全体の62%）は
  `taxonomy.related.json` に分けて**最初に検索されたときだけ**取りに行く。
  分野カードを眺めるだけの利用者はこの10.6MBを一切払わない。読み込み中も
  他の4段階は普通に出て、「関連概念」段階だけ理由付きで後から埋まる。
  検索索引は1件ごとのオブジェクトではなく**列**で配信する——キー名を
  80,727回繰り返すのをやめるだけで7.81MB→2.88MBになるため（受け取った
  直後にブラウザ側で普通の形へ戻すので、検索ロジックは列指向にしていない）。
- **wasmが落ちてもページは死なない**: 数式カーネルは動的importで読み込む。
  静的importだった頃は、112KBのwasmの読み込みに失敗しただけで
  `main.ts`自体が実行されず**ページ全体が真っ白**になっていた（wasmを
  一切使わない概念エクスプローラーも証明グラフも巻き添えで消えた）。
  現在は骨格と両データ区画は必ず描画され、数式解析・デモ表・層3〜5デモの
  3区画にだけ理由を表示する。本番ビルドからwasmチャンクを削除して実証済み。
- **URLで共有できる**: 検索語（概念区画・証明グラフ区画それぞれ）と開いて
  いる区画をURLのハッシュに写す。
  `#q=kahler+manifold&pg=weak+harnack&mode=msc` のようなURLを開くと、
  タブの状態も両区画の検索結果も復元される。ブックマーク・共有・戻るが
  効くようになった（以前はURLが常に `/` のままで、再読み込みで全部消えた）。

- **クラスタリングの高度化（LPA → CPM + 相互k近傍）**: 概念グラフの
  クラスタリングを、目的関数を持たないLabel Propagationから
  CPM（Constant Potts Model）のモジュラリティ最適化に置き換え、グラフ
  構築も和集合k近傍から**相互**k近傍に変えた。実データ100k論文で
  MSC-NMI 0.568→0.632、最大クラスタ1,117件→22件。検索の「同一概念」段階が
  実際に良くなり、"fixed point" で fixed point set / theorem / sets /
  theorems / property が返る（以前は群論・線形代数・集合論が混ざった
  1,117件の塊から拾っていた）。品質指標（モジュラリティ・NMI・純度）は
  `cluster` コマンドが毎回表示するので、`--algorithm cpm|louvain|lpa` と
  `--resolution` を変えて数字で比較できる。
- **共起の正規化をJaccard係数に修正**: 上記の高度化後も"finite group"の
  クラスタに"time reversal"/"charge conjugation"のような無関係な物理
  用語が残っていた。原因は`graph.rs`の共起正規化 `count/min(df_i,df_j)`
  （包含係数）が、片方の文書頻度が小さいだけで（他方がどれだけ大きくても）
  高い値になる欠陥——実データを走査すると同型のペアが29,448件見つかり、
  孤立事例ではなかった。`count/(df_i+df_j-count)`（Jaccard係数）に
  置き換え、"finite group"のクラスタは9件（純粋な群論用語のみ）まで
  縮小、混入していた物理用語も意味的に正しい別クラスタへ移った
  （詳細と実測値はアーキテクチャ.txt参照）。
- **検索の順位付けにIDFを導入**: 従来はクエリの語をすべて同じ重みで数えて
  いた（"kähler manifold" の "manifold" と "kähler" が同格）。IDF重み付きの
  F値（β=2、再現率を重く）に変更。教科書どおりのBM25も試したが長さ正規化が
  1語の候補を不当に優遇したため採用しなかった（詳細はアーキテクチャ.txt）。
- **論文収集が増分・再開可能になった**: `mathesis-ingest` は
  ページごとにDBへ保存し、`resumptionToken` を `harvest_state` に残す。
  `--resume` で中断した続きから、`--since-last` / `--from YYYY-MM-DD` で
  前回以降の差分だけを取得できる。以前は全件をメモリに溜めて最後に一括
  保存していたため、途中で落ちると全損し、更新は毎回全件再取得しかなかった。

## 概念の文脈ベクトルと評価セット（`context.rs` / `resolve.rs` / `web/eval/`）

出荷中の`taxonomy.related.json`を全走査して分かったこと: 関連リスト
74,566件のうち**86.5%（64,520件）が「語幹を共有する候補」で6割以上
占められていた**。原因は概念のembeddingが `embed_text(model, phrase)`
——**フレーズ文字列そのもの**をOllamaに投げたもので、cos類似度が
文字列類似度の言い換えにしかなっていなかったこと。

    riemann hypothesis → extended riemann hypothesis, riemann function（以上）
    brownian motion    → brownian motions, ordinary brownian motion, …

zeta functionもL関数もRiemann予想の隣に無く、Wiener過程もマルチンゲールも
Brownian motionの隣に無かった。クラスタも同様で、"elliptic curve"の綴り
10種、"quantum group"の綴り12種がそれぞれ1クラスタを成していた。つまり
**タクソノミー構築のつもりのパイプラインが Entity Resolution を実行して
いた**。

修正は2本立て。

- `crates/mathesis-taxonomy/src/context.rs`（新規、`mathesis-taxonomy
  context`）——概念を「名前」ではなく「**どの概念と同じ論文に現れるか**」で
  表す。共起のPPMI（文脈側周辺分布に指数α=0.75の平滑化）を
  ランダム化対称固有分解で192次元へ落とす。**Ollamaを必要とせず、純Rustで
  決定的**（乱数は`ann.rs`と同じ固定シードの自前PRNG）。142,948論文・
  概念112,933件で96.4秒。文字列embeddingは`cluster --vectors string`で
  比較用に残してある。
- `crates/mathesis-taxonomy/src/resolve.rs`（新規）——表記ゆれを畳む工程を
  独立させ、クラスタリングには解決済み概念だけを渡す。畳むのは大小文字・
  ダイアクリティカル・ハイフン/空白・単数複数**だけ**。"compact kähler
  manifold"と"kähler manifold"は別概念として残す——**畳み損ねは重複が
  残るだけだが、畳み過ぎは別の概念を消す**。実データで88,097候補 →
  72,632概念。畳んだ事実はWeb側に「同じ概念の別表記: …」として出す
  （黙って畳むと、打った表記が結果に無い理由が分からない）。

実測した効果:

    綴り変種が6割以上を占める関連リスト   86.5% → 1.2%
    riemann hypothesis → riemann zeta function, consecutive zeros, rh, zeta,
                         zero-free region, zagier
    navier-stokes equations → incompressible euler equations, global existence,
                         local existence, viscous, global weak solution
    quantum groups     → drinfeld, weyl group element, cartan calculus,
                         billig, canonical basis, lie derivative
    zeta function      → riemann, functional equations, poles, classical
                         riemann hypothesis, good reduction, etale cohomology

"elliptic curves"の「同一概念（同じクラスタ）」段階も、綴り10種から
finite field / number field / abelian varieties / rational points /
modular forms / zeta function / galois group ——数論幾何そのもの——に
変わった。

### 評価セット（`npm run eval`）

これまで検索・クラスタリングの改善は全て目視で検証されており、数値は
自己申告MSCに対するNMIと純度だけだった。**この2つは今回の欠陥を1ビットも
捉えていなかった**——同じコーパス・同じER・同じクラスタリングでベクトル
だけを替えたA/Bで、既存の指標は**旧方式の方を高く**評価する:

    文字列embedding  NMI 0.6335 / 純度 88.8%  最大クラスタ = hopf algebra の綴り8種
    文脈ベクトル      NMI 0.6299 / 純度 87.9%  最大クラスタ = initial data /
                     cauchy problem / weak solutions / sobolev spaces（PDE分野）

そこで固定の評価セットを`web/eval/`に置いた。`queries.json`は40クエリ
（完全一致・機能語入り・語順違い・タイポ・単複・ハイフン固有名・2概念
並置）に人手で正解を付けたもの、`relations.json`は概念対33組（数学として
関連する17組・無関係6組・綴り変種10組）。評価対象は**利用者が実際に使う
コードそのもの**で、`src/hybridSearch.ts`をViteで束ねてNodeで走らせる。

現在値（142,948論文、索引93,923件——2026-09-04のEntity Resolution
修正後、`search_index`が解決済み概念単位になったため112,933から変化。
下記参照）:

    MRR@10 0.9667 / Top-1率 95.0% / dead率 0.0%（40クエリ）
    関連の再現率 47.1%(8/17) / 無関係の誤結合 0.0%(0/6) / 変種の漏れ 0.0%(0/10)

作った初日に実バグを1件検出した: `resolve.rs::singularize` の
「末尾が as なら複数形ではない」という一般規則（"atlas"のためのもの）が
**"algebras"を"algebra"に畳めなくしていた**。数学の語彙では-asで終わる
単数形より複数形（algebras / formulas / areas）の方が圧倒的に多いので、
一般規則をやめて例外表に移した——目視では見つからなかった種類のバグ。

関連の再現率は当初58.8%(10/17)だったが、2026-09-04に`search_index`・
`taxonomy.related.json`の近傍計算を解決済み概念単位（プールしたベクトル）
に直した後は47.1%(8/17)——**2組減った**。無関係の誤結合・変種の漏れは
どちらも0%のまま変わっていない（実害のある劣化ではない）ので、この
修正自体は差し戻していない。原因と考えられるのは、表記ゆれを畳んで
別表記のノードが消えたことで上位k件の「競争相手」の顔ぶれが変わり、
以前は僅差で上位6件に入っていた対が押し出されたこと——`min_sim`や
`k`を個別に調整すれば戻せる可能性はあるが、今回のスコープ
（検索結果の重複解消）には含めない。関連の再現率58.8%も47.1%も
**達成目標ではなく現在地**であることに変わりはない。繋がらなかった9組
はどれも「2ホップなら繋がるが直接の上位6件には入らない」型で、
近傍数kを12へ上げても変わらない（配信量が15.5MB→26.7MBに増えるだけ）
ことを以前実測済み。関係の**種類**を持たない限りこれが上限に近い。

## 型付き関係（`relations.rs` / `relations.json`）

「特殊化」段階（`search.rs::is_specialization`）は文字列包含だけの判定で、
`quantum group`を`group`の特殊化として返す一方、楕円曲線⊂アーベル
多様体のような語彙が重ならない真の特殊化は1件も検出できなかった。
そこで2つの独立経路から型付き関係を集めるモジュール
`crates/mathesis-taxonomy/src/relations.rs`（`mathesis-taxonomy
relations`コマンド）を追加した。

- **分布的非対称包含**（Weeds & Weir 2003のWeedsPrec、Lenci & Benotto
  2012のinvCL）。`context.rs`が作るPPMI行列の生の行から、狭い概念の
  文脈が広い概念の文脈にほぼ包含される非対称性を検出する。根拠文は
  持たない（`status: Proposed`）。
- **Hearstパターン**。その論文が既に抽出済みのフレーズだけを対象に
  （任意の名詞句を解析しようとしない）、"is a" / "such as" / "is
  equivalent to" 等の接続表現が2つの既知フレーズの間にあれば、その
  文そのものとarXiv IDを根拠として拾う（`status: Grounded`）。両経路が
  一致すれば`status: Confirmed`。

**Confirmed（両経路一致）を84件全て人手で確認したところ、正しかったのは
約36%**。主な誤りは「述語的名詞句」（"the channel capacity is a convex
function of ..." のような性質の主張を分類関係と誤認）と「主語の誤帰属」
（構文解析なしには真の主語を特定できない）——`relations.rs`冒頭に実例
つきで明記した既知の限界。だからこそ**根拠文の無いProposed（約10万件）
はWebへ出さない**——Confirmed/Grounded（根拠文つき）だけを
`relations.json`として書き出し（P2以降`mathesis-provenance web-export`が
証拠層から直接生成する。`taxonomy.relations.json`という同名ファイルを
`mathesis-taxonomy export`自身が書いていた時期があったが、
`docs/P2_STATUS.md`の変更で廃止した）、検索結果に「より一般的
な概念」「より特殊な概念」「同値の可能性がある概念」として表示、必ず
根拠文とarXivリンクを併記して読者がその場で判断できるようにしてある。

**2026-09-03の追加修正で約50%（38件中19件）まで改善した。** 実測した
2つの主要な誤りパターンを狭く塞ぐ表層的なヒューリスティックを3回に
分けて追加した: (1) 目的語の主辞が"function"/"measure"/"feature"/
"description"等、性質・役割を表す一般名詞なら述語的名詞句とみなして
見送るブラックリスト、(2) 主語候補が同じ節内（カンマをまたがない）で
前置詞句の内部にあれば真の主語ではないとみなして見送るチェック。3回目の
修正では、固定長60文字の窓に無関係な手前の節のカンマが入り込み
「窓内にカンマがあれば前置詞を無視する」という初期実装が本物の手がかり
（"the transition **to a** superconducting state..."の"to"）ごと
握りつぶしていたバグも見つけて直した——直近のカンマより後ろだけを見る
ように修正。Confirmed件数は95→38に減った（より高い基準で少数精鋭に
絞った結果）。バッジのツールチップにも実測精度50%を明記——
「確認済み」ではなく「統計と本文が一致した」以上の確実性は主張しない。

全件確認を通じて、当初の「述語的名詞句」「主語の誤帰属」という2分類
では捉えきれない、より根深い誤りの型が3つ見えてきた（いずれも今回は
未対処、`relations.rs`冒頭に詳細を記載）:
- **定理固有の主張の一般化**（残った誤りの中で最多）。"the generic
  fiber is a reductive group"のような文は、その論文のその定理の
  仮定下でだけ成り立つ主張であって、一般的な分類関係ではない。
- **列挙パターンの係り先誤り**。"such as"/"including"が直前の名詞句
  ではなく、もっと離れた真の被修飾語に係っている場合。
- **候補フレーズ抽出そのものの不備**。文法的に不完全な候補フレーズや、
  形容詞脱落で別概念になったフレーズ——上流の`concepts.rs`の問題。

これらは構文解析かLLMベースの文単位判定を要する——表層パターンの
追加調整で対応できる範囲を超えつつある。**この後、実際にLLM
（qwen2.5:3b/7b-instruct、ローカルOllama）で2種類の判定を試した**
——①Hearstヒットの根拠文が一般的な定義かこの証明限りの結論かを文単位で
判定（2026-09-04、言い回しで結果が大きく揺れ不採用）、②根拠文の無い
Proposed候補を、根拠文なしでLLM自身の知識だけで正誤判定（2026-09-05）。
②は当初3Bのみで試して採用側が不安定という結論だったが、モデルを7Bに
上げた3B→7B cascadeとして`crates/mathesis-taxonomy/src/llm_judge.rs`に
実装済み——却下側は実データでも安定・良好だったが、**採用側は手作り
テストでは満点でも実データ計450件ユニーク（2バッチ計600件、うち150件
重複）では妥当な採用がわずか1件のみで、accept側／Web反映ルート
（新しいstatus追加）は正式に凍結した**。詳細は`relations.rs`冒頭の
コメント参照。

実データで正しく拾えた例: cyclic codes⊂linear codes、moonshine
module⊂vertex operator algebra、right censored data⊂censored data、
string theory⊂quantum gravity、doubly stochastic matrices≡birkhoff
polytope。

**追記（2026-09-05）: 上の「候補フレーズ抽出そのものの不備」を実際に
調査・修正した。** LLM cascadeの実データ収量が低かった一因は分類器
ではなく上流（`concepts.rs`/`rake.rs`）の候補品質にあるという仮説を、
本番100kコーパスの`concept_candidates`を直接検査して検証した。2種類の
欠陥を発見・修正（詳細は`rake.rs`冒頭コメント参照）:
①"one"/"two"/"three"が汎用ストップワードだったせいで、"genus zero"
（100件）は候補に残るのに**"genus one"「genus two"「genus three"だけが
存在しない**という非対称な欠落があった——genus/characteristic等の
数詞は数学的に別の対象を指す固有の値であり、一般化して失ってよい情報
ではない。②"associated"「based"「following"のような、関係節を導入する
だけの分詞・動詞が前置詞や補語ごと欠落した断片（"algebras associated"
374件・"following question"123件・"admits"単体1206件等）として大量に
残っていた。両方とも、修正後も"associated primes"「generating
function"「defining relations"「induced representations"「universal
cover"「lie algebra"「implied volatility"のような正当な用語は実データで
確認しつつ保持した（同じ語幹でも先頭の用法だけ正当な場合は意図的に
ストップワード化を見送っている）。
本番DBに対して`extract`→`context`→`cluster`→`align`→`relations`→
`export`のパイプライン全体を再実行し（候補113,391件、うち定型句として
除外829件）、Web版Explorerで実際に"genus one"を検索して57件（"genus
one curves"18件等の複合語含む）がヒットすることを確認した。

副産物として、Confirmed/Groundedに矛盾する循環（AがBの特殊化かつBもAの
特殊化）が無いかを自動チェックする仕組みも`web/eval/searchEval.ts::
evaluateTypedRelations`に追加した。実データで実際に1件見つかり
（"Einstein nilradical is a nilpotent Lie algebra" と "a nilpotent Lie
algebra ... is an Einstein nilradical" が同じ論文の別々の文から）、
`relations::remove_specialization_cycles`でどちらが正しいか判定できない
矛盾ペアを両方落とすようにした。

## 概念地図（診断⑤、`conceptMap.ts` / `conceptMapView.ts`）

診断⑤「実際の可視化はLineageView1つだけで、他は`<section>`の縦積み。
ただし今グラフを描いても綴りの星座しか出ない——距離も向きも無い空間の
地図は描けないので、①②の後でなければ意味が無い」への対応。①（文脈
ベクトル）と②（型付き関係）が実装済みになったことで、意味のある
地図を描ける下地ができた。

全概念72,632件を一度に描く「銀河ビュー」は意図的に作っていない——
大半の辺が重なるだけの「毛玉」になることは可視化の定石として知られて
おり、かつこの規模のforce-directedレイアウトはブラウザで現実的な
時間に収まらない。代わりに、検索結果の各行に「地図で見る」ボタンを
付け、その概念を中心に**意味的近傍（コサイン類似度の近傍1〜2ホップ、
型付き関係）だけを局所的にレイアウトする**——ノードをクリックすると
そこを新しい中心として地図を作り直す、Googleマップのパン操作に近い
探索体験。MSC分野→クラスタ→概念という既存のドリルダウンが「広い→
狭い」の俯瞰を既に提供しているので、この地図はその末端に実データの
詳細を足す形になる。

レイアウトはFruchterman-Reingold法（総当たりの反発力＋辺に沿った
ばね引力、冷却スケジュール付き反復300回）——ノード数がたかだか数十件
なので、専用の次元圧縮（t-SNE/UMAP）を実装するまでもなく、既に持って
いる辺（近傍のコサイン類似度・型付き関係の確信度）をそのままばねの
強さに使えば十分。初期配置は中心概念名のハッシュを種にした決定的
疑似乱数（mulberry32、Rust側の固定シードPRNGの流儀をTypeScript側でも
踏襲）で、同じクエリなら毎回同じレイアウトになる。矢じりマーカーと
配色（`--lin-special`/`--lin-equiv`）は既存の`lineageView.ts`の
パターンをそのまま再利用した。

データは一切新設していない——`taxonomy.related.json`・`taxonomy.
relations.json`はどちらも①②の対応で既にexportされていたものを
そのまま使う。Rust側の変更はゼロ、フロントエンドだけで完結した。

実機ブラウザで"moonshine module"を検索→地図を開くと24ノード・36辺
（"vertex tensor category"「"WZNW models"「"vertex operator algebra"
等、実際に意味的に近い概念が並ぶ）、ノードクリックでの再センタリング
も動作確認済み。

## 配信の不可分性への対応（診断④、`searchWorker.ts`）

配信量は 47.4MB → 28.5MB → 初回6.6MB（`relatedEdges`の別ファイル化と
列形式化）まで削ったあと、**論文を題名つきの文書として載せたことで
再び増えた**——現在は初回`taxonomy.json`9.0MB。だが実測して分かった
本当の壁はバイト数ではなく**不可分性**だった: `JSON.parse`が378ms、
`queryIndex.ts::buildConceptSearchIndex`（転置索引・IDF計算）が254ms、
合わせて632msがメインスレッドを**不可分に**塞ぎ、その間ページは
クリックにも入力にも応答しない。gzip後1.6MBという転送量自体は軽いのに
体感が重かったのはこのため。

対応した内容:

- **`taxonomy.json`のfetch・parse・索引構築を丸ごとWeb Worker
  （`searchWorker.ts`）へ移した。** `ConceptSearchIndex`の中身
  （`Map`・`Int32Array`・`Float64Array`・配列・文字列・数値）はいずれも
  構造化複製に対応しているため、Worker側で完成させた索引を
  `postMessage`でメインスレッドへそのまま渡せる——メインスレッド側の
  検索ロジック（`hybridSearch`等）は一切変更せずに済んだ。
- **ヘッドシャード**（`taxonomy.head.json`、文書頻度上位800件・27KB、
  `export.rs::build_head_shard`）を新設し、主スレッドで直接・同期的に
  読み込む。フル索引がWorker上で出来上がるまでの間、この小さな索引で
  即答する——人気の高い概念（＝検索されやすい概念でもあるはず）に限れば、
  ページを開いた直後の1打鍵目から結果が出る。フル索引が届き次第
  自動的に切り替わり、その間は「全件の索引を読み込み中」という
  ヒントを表示して「該当なし」と区別できるようにした
  （`fullIndexLoadingHint`）。
- `taxonomy.related.json`（10.57MB）は意図的にWorkerへ含めていない
  ——「利用者が実際に検索するまで取りに行かない」という既存の遅延
  読み込みを崩さないため。

`npx vite build`はWorkerを別チャンク（`searchWorker-*.js`、2KB弱、
共有ロジックは`index.js`側のチャンクと共有）として正しく分離し、
`vite dev`・`vite preview`（本番相当のバンドル）の両方で実機ブラウザ
確認済み——検索・型付き関係・出典論文・表記ゆれの全機能が従来どおり
動作する。静的ホスティングのままで実現でき、サーバAPI化は要らない。

**まだやっていないこと**: README（旧版）で提案していた「シャード化
した二進索引」（語彙辞書とポスティングを語のハッシュで分割し、
クエリが触るシャードだけを取得、デコードをwasmで行う）自体は未実装。
今回測り直した結果、実際の壁は**不可分性**であって**バイト数**では
なかった（gzip後1.6MBは軽い）ため、Workerへ移すだけで診断された問題は
解消したと判断し、バイナリ形式への作り直しは見送った。コーパスが
今よりさらに桁違いに大きくなり、9.0MB自体の転送/parse時間が
支配的になった場合は、あらためて実測してから判断する。

## 証拠層への追跡（Phase 1, `mathesis-provenance`、2026-09-05）

ARCHITECTURE_NEXT.md（証拠ベースの再設計）のPhase 1で、判断グラフの
依存関係・射と概念タクソノミーの型付き関係それぞれに、証拠層
（`crates/mathesis-provenance`が持つ`RelationAssertion`/`Evidence`/
`SourceRecord`/`Release`）への追跡情報を足した。

**この節はPhase 1時点のアーキテクチャの記録**——最初は`judgments.json`/
`taxonomy.relations.json`という既存2ファイルの形を一切変えず、
`mathesis-provenance reconcile`が別途サイドカー
（`judgments.provenance.json`/`taxonomy.relations.provenance.json`）を
書き出し、フロントエンドが追加でfetchして突き合わせる方式だった。
**Phase 2（`docs/P2_STATUS.md`）でこの方式は終わった**——今は
`dependencies.json`/`morphisms.json`/`relations.json`を
`mathesis-provenance web-export`が証拠層**だけ**を入口に直接生成し、
`mathesis-graph`/`mathesis-taxonomy`はそれらの辺を一切書き出さない。
以下の「完全性ゲート」「まだやっていないこと」は今も有効だが、
「レガシー互換モード」の節（サイドカーが無くても動く）は
`judgments.provenance.json`/`taxonomy.relations.provenance.json`
（今は`verify`専用の内部ファイル）にのみ当てはまる——
`dependencies.json`等はもう「無くてもいい追加情報」ではなく必須データ。

**Provenanceボタンとassertion詳細パネル**（外部レビューの2回目、
2026-09-05）: 射チップと型付き関係の根拠段落にある「Provenance:
assertion #123 (release ...)」はボタンで、クリックすると
`provenancePanel.ts`がネイティブ`<dialog>`を開き、`assertions.json`
（8,969件のassertion全件の詳細を1回だけ取得してキャッシュする辞書）
から関係の種類・主語目的語・認識状態・Evidence各行（種別・根拠文の引用
・抽出元・メトリック・ソースの由来）・レビュー決定・既定のトラバース
対象かどうかを表示する。新しいモーダル基盤は増やしていない——このコード
ベースが元々ツールチップをnative `title`属性で済ませてきたのと同じ最小
主義で、ネイティブ`<dialog>`をそのまま使う。

**完全性ゲート**（同、2026-09-05）: `mathesis-provenance reconcile`は
書き出す前に、`mathesis-graph`/`mathesis-taxonomy`の元データが持つ辺
**全件**が証拠層の`RelationAssertion`へ実際に引けるかを検証する
（2026-09-05時点のv0-baseline: 依存関係5,634/5,634・射2,284/2,284・
型付き関係1,051/1,051、全件一致）。1件でも引けなければ非ゼロ終了する。
さらに`mathesis-provenance verify`は、`provenance-manifest.json`
（採用したアダプタ版・入力DBのSHA-256・件数を記録した機械可読な出所
情報）とサイドカー・証拠層DB本体を突き合わせ、assertion欠落・Evidence
無し・SourceRecord参照切れ・release不整合・サイドカー内の重複キーの
曖昧解決・入力ファイルの改変（ハッシュ不一致）を検出する——1回きりの
確認ではなく、リリースのたびに回すゲートとして作った
（`crates/mathesis-provenance/tests/verify_test.rs`に異常系のテストが
一通り揃っている）。**安定化パス（同日・第2版、`docs/P2_STATUS.md`）**
以降、実際にCI/リリース時に叩くべき唯一のコマンドは
`mathesis-provenance verify-release`——`verify`（上記）に加え、
`dependencies.json`/`morphisms.json`/`relations.json`自身のハッシュ
検証と、今のProvenanceStoreから再生成した内容との構造的一致（「古い
コミット/リリースから生成されたexportがそのまま残っている」の検出）を
1つのコマンド・1つの終了コードにまとめたもの
（`crates/mathesis-provenance/src/release_gate.rs`、
`tests/release_gate_test.rs`）。

**レガシー互換モード / 開発時の可視化**: `dependencies.json`/
`morphisms.json`はP2以降レガシーの代替経路が無い必須ファイルなので、
欠落・形式異常はどちらも`loadError`（全環境で見える表示）になる。
`relations.json`（`taxonomy.aliases.json`/`taxonomy.papers.json`と同じ
遅延読み込み系列）は404（旧リリースにファイルが無い）なら黙って
今までどおり表示する——意図的な後方互換動作。一方で404以外の失敗
（形が壊れている、サーバエラー、パース失敗）は`util.ts::reportProvenanceIssue`
で報告する: 本番ビルドでは`console.error`だけに留め閲覧者の画面は
静かなまま、開発時（`import.meta.env.DEV`）だけ画面右下に警告バナーを
出す。「ファイルが無ければ動く」という互換性を、「壊れたリリースを
黙って正常扱いにする」にしないための区別（`util.ts::assertArrayShape`
が配列でないJSONを即座に例外にする——0件の正常なデータセットと
壊れた形を取り違えない）。

**まだやっていないこと**: レビューワークフロー（`ReviewDecision`は
レガシーの`Accepted`を保存する器としてのみ機能し、実際のレビュー画面は
無い）、型付きエンティティカタログ（`subject_ref`/`object_ref`は
`"kind:id"`形式のタグ付き文字列で、ARCHITECTURE_NEXT.md §5.2の正式な
カタログではない）、Lean elaborator由来の`verified`状態。詳細は
`docs/P1_STATUS.md`（Phase 1が今どこまで保証しているか）、
`docs/P2_STATUS.md`（Web読み取りモデルを証拠層から直接生成する変更、
2026-09-05）、`docs/P3_STATUS.md`（型付きエンティティカタログの
最初の増分——`assertions.json`の`subjectLabel`/`objectLabel`はここから
来る）、`docs/DATA_DICTIONARY.md`の「Known limitations」参照。

## 今後の課題（意図的に今回は着手していない）

- 配信時の圧縮（gzip/brotli）については「## ビルド手順」内の
  「デプロイ時の必須確認事項」に実測値と確認コマンドを明記済み——
  ここに残っていた「配備手順に明記すべき」というTODOは解消した。
- 候補フレーズの品質フィルタは統計（`concentration.rs` の分野集中度）に
  置き換え済みだが、その統計には原理的な限界がある——測っているのは
  「分野への固有さ」であって「概念らしさ」ではないため、書き方の作法
  そのものが分野と相関している定型句（"mild assumptions" 0.688 は統計・
  PDE系の作法、"rigorous proof" 0.609 は数理物理の作法）は中身が空でも
  高いスコアを得て生き残る。統計は「分野ラベル付き論文が50件以上」
  （`MIN_LABELED_PAPERS`）で初めて発動するため、コーパスが小さいほど
  この限界が顕著になる——実際に10,000論文規模でベンチマークしたところ、
  100,000論文規模では見えなかった定型句クラスタ（"simple proof / …"・
  "wide class / …"等）が複数見つかった。取りこぼしは
  `concepts.rs::REGISTER_PHRASES` に明示リストとして残しているが、これは
  本質的にいたちごっこ——同じ語群の一部を塞ぐと、別の顔ぶれ（"proof"を
  核に別の形容詞・動詞と組んだ語）が同じ順位に浮上することを実地で確認
  済み。原理的な解決はPhase 8以降のLLM judge（統計で判別がつかなかった
  少数にだけ掛ける）であり、今回も着手していない。
- `taxonomy.json` / `taxonomy.related.json` は静的スナップショット（`mathesis-taxonomy export` を
  再実行するたびに手動で書き出し直す必要がある）。ライブDBに接続する
  バックエンドAPIにするかは、実際にオンライン更新が要るとわかってから判断。
- ~~`mathesis-importer` の Lean パーサーは今のところ CLI バイナリ内に閉じている~~
  → 解消済み。`mathesis-lean-parse`（`mathesis-graph`に依存しない純粋な
  パーサークレート）へ切り出し、CLIと`mathesis-wasm`の両方がそこへ依存する
  形にした。詳しくは「現状の機能」の「Leanを貼り付けて試す」の項を参照。
- hybrid searchのrelated段階は、ブラウザではエクスポート時に事前計算した
  近傍（`relatedEdges`）を引くだけなので、完全一致しないクエリでは常に
  空になる。CLI版のようにその場でOllama embedding化する経路をブラウザにも
  持たせるには、ブラウザ→Ollamaの直接通信（CORS・混在コンテンツの検討が
  要る）か、薄いバックエンドAPIを挟むかの判断が必要——現時点では
  taxonomy.jsonを静的スナップショットに保つ方針（5.6参照）を優先し、
  着手していない。
- ~~entity resolution（"Kähler manifold"と"kahler manifold"のような表記ゆれを
  同一concept IDへ寄せる、5.4 Step 3）は未実装~~ → 解消済み。この行自体が
  古い記述だった——`resolve.rs`（表記ゆれの畳み込み本体）は「概念の文脈
  ベクトルと評価セット」の節が示す通り既にこのドキュメントの別の箇所に
  実装・実測済みと明記されており、クラスタリング・関係抽出はとっくに
  解決済み概念を単位にしていた。実際に欠けていたのは`main.rs::run_export`
  が書き出す`search_index`・`taxonomy.related.json`の近傍計算・
  `taxonomy.papers.json`の3つだけが、Entity Resolutionを経由せず解決前の
  生候補（112,933件）をそのまま単位にしていたこと——「未実装」という
  この行の記述を鵜呑みにせず着手前にコードを読んでいれば、この行が
  何年も前から古かったことにもっと早く気付けた（診断⑥の教訓
  [[fulltext-bridge-already-built-before-graph-wiring]]と同型）。
  2026-09-04にこの3箇所を解決済み概念単位に直し（`アーキテクチャ.txt`
  「Entity Resolutionが検索索引に届いていなかった欠陥の修正」参照）、
  検索索引は112,933候補→93,923解決済み概念になった。ついでに
  ドイツ語由来の音訳（kaehler/schroedinger/goedel）がサーバ側・
  クエリ側（`queryIndex.ts`）の両方で畳めていなかったことも見つけて
  直した。
- `judgments.json`も`taxonomy.json`と同じく静的スナップショット。取り込む
  Leanリポジトリを変えるたびに`mathesis-import ... --export`を再実行する
  必要がある。
- 証明グラフ区画の検索（`proofSearch.ts`、4段階）は、命題本体を
  「文字列として」しか見ていない。`IsCompact (sphereTwoControl d)` を
  `mathesis-ast` の式ASTとして構造照合すれば「この形の命題」「この
  述語を適用している判断」のような検索ができるはずだが、
  `judgments.json` は命題を文字列で持っており（`canonical_hash` は
  あるが式木は載っていない）、そこまでは着手していない。
  検索結果の「グラフ上の関連」段階は今も1ホップ固定（深さ方向の追跡は
  判断を1件選んだあとの系譜ビューが担う）。
- 実データ側の`morphisms`は`--propose-morphisms`のヒューリスティック候補
  （`status: "proposed"`）のみで、人間によるレビュー・承認
  （`GraphStore::accept_morphism`）はDeGiorgiコーパスに対してまだ行っていない
  ——Web UIからのレビュー/承認操作（Accept/Reject）も今のところ無く、
  表示専用。
- 層3〜5デモ（`layer35.ts`）はページを再読み込みすると内容が消える
  （`mathesis-wasm`のインメモリ`KernelStore`はブラウザの1セッション限りで、
  他のデモ判断ノード同様に永続化しない設計のため）。

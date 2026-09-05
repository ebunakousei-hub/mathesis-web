//! 型付き関係（アーキテクチャ.txt 5.3 の `Relation` 列挙）の抽出。
//!
//! # なぜ要るか
//!
//! 5.3で定義された `Relation`（IsA/PartOf/SpecializationOf/…）は、
//! 2026-09-03のアーキテクチャ批評まで**一行も実装されていなかった**。
//! 出荷されていた「特殊化」（`search.rs::is_specialization`）は
//! 「候補がクエリ語を連続して含み語数が多い」という文字列包含にすぎず、
//! `quantum group`（Hopf代数であって群ではない）を`group`の特殊化として
//! 返す一方、楕円曲線⊂アーベル多様体のような語彙が重ならない真の特殊化を
//! 1件も検出できなかった。
//!
//! ここでは2つの独立した経路から型付き関係を集め、両方が一致した場合・
//! 片方だけの場合で信頼度を分ける（`mathesis-graph`の射が
//! `status: proposed`で未承認を明示する既存の設計を踏襲）。
//!
//! ## 経路1: 分布的非対称包含（Distributional Inclusion Hypothesis）
//!
//! `context.rs`が概念×概念のPPMI行列を作っている。この生の行（＝ある概念が
//! どんな概念と共起するかの重み付き集合）に対し、Weeds & Weir (2003) の
//! 非対称包含度を計算する:
//!
//!   WeedsPrec(u→v) = Σ_{k∈N(u)∩N(v)} PPMI(u,k)  /  Σ_{k∈N(u)} PPMI(u,k)
//!
//! 「uの文脈のうち、vの文脈にも含まれる割合」。狭い概念（elliptic curve）の
//! 文脈は広い概念（abelian variety）の文脈にほぼ包含されるが、逆は成り立た
//! ない、という分布仮説そのもの。Lenci & Benotto (2012) のinvCLで両方向を
//! 合成し、非対称性が強いペアだけを特殊化として採用する。
//!
//! この経路は**方向を持つが根拠文を持たない**——`status: Proposed`。
//!
//! ## 経路2: Hearstパターン（論文本文からの直接抽出）
//!
//! アブストラクトには"A is a special case of B"のような関係を明示する文が
//! 実際に書かれている。ここでは任意の名詞句を抽出しようとはせず
//! （英語NP境界の判定は誤りやすい）、**その論文が既にconcept_candidatesの
//! フレーズとして抽出済みの語だけ**を対象にする——`mathesis-lean-parse::
//! find_referenced_names`が「既知の判断名との完全一致」だけを見て任意の
//! 識別子を解析しようとしなかったのと同じ設計判断。2つの既知フレーズの
//! 出現位置の間に決まった接続表現（"is a" / "such as" / "is equivalent
//! to" / "is a generalization of" 等）が挟まっていれば、根拠文
//! （その文そのもの）とarXiv IDを持つ関係として拾う。
//!
//! この経路は**根拠文を持つ**——`status: Grounded`。両経路が同じ
//! (subject, object, kind)に一致した場合のみ`status: Confirmed`。
//!
//! # 既知の限界（実データで確認済み、未解決）
//!
//! 実データ（142,948論文）で最初に走らせたとき、2種類の誤検出が見つかった。
//!
//! 1. **同格パターンの誤爆**（修正済み）。当初 "X, a Y," という裸の
//!    同格パターンを入れていたが、英語のカンマは同格以外にも導入節・
//!    従属節境界・列挙など極めて多くの役割を持つため、
//!    "Focusing on energy efficiency, a noncooperative game is
//!    proposed in which …"（"energy efficiency"は次節の主語ですらない）
//!    のような文法的に誤った関係を量産した。ANCHORSから外し、
//!    動詞を介するため誤読されにくい"is a"系だけを残した。
//!
//! 2. **述語的名詞句の誤検出**（部分的に緩和・2026-09-03の追加修正）。
//!    "the channel capacity is a convex function of the stochastic
//!    channel matrix" は文法的に正しい"is a"パターンだが、これは
//!    「channel capacityという対象がconvexという性質を持つ」という
//!    **性質の主張**であって、「channel capacityはconvex functionsの
//!    一種である」という**分類関係**ではない。深い統語解析なしに
//!    「性質」と「分類」を一般に区別することはできないが、実測した
//!    84件の誤りの多くは物性・統計量を表す一般名詞（function, measure,
//!    operator, invariant, matrix, ...）が"is a"直後の目的語になって
//!    いたため、`PREDICATE_NOMINAL_HEAD_BLACKLIST`で裸の"is a"/"is an"/
//!    "are a"（"is a kind of"のような分類語を含む長い接続表現は対象外）
//!    にのみこの語をobjectの主辞（最後の単語）として持つヒットを
//!    見送るようにした。網羅的な解決ではなく、実測で見つかった主要な
//!    パターンを狭く塞いだだけ——このブラックリストに無い性質名詞は
//!    今も誤検出しうる。
//!
//! 3. **主語の誤帰属**（部分的に緩和・2026-09-03の追加修正）。
//!    "every simple weight module ... with a nontrivial
//!    finite-dimensional weight space, is a Harish-Chandra module" の
//!    ような文で、"is a"の直前に最も近い既知フレーズ（"weight space"）
//!    を拾ってしまうが、真の主語は離れた場所にある"module"——構文解析
//!    なしに一般には検出できない。ただしこの誤りの多くは、"is a"直前の
//!    候補フレーズが前置詞句（"with a ... weight space,"）の内部に
//!    あるという表層的な手がかりで検出できる。`subject_is_pp_internal`
//!    が「候補フレーズの直前・同じ節内（カンマをまたがない）に前置詞が
//!    ある」ケースを検出し、その場合はヒットごと見送る（正しい主語を
//!    推測しようとはしない——`resolve.rs`と同じ「間違って作るより
//!    取りこぼす方が安全」という判断）。前置詞句の外にある誤帰属
//!    （例えば別の理由で近くの語が誤って主語に選ばれる場合）は今も
//!    残りうる。
//!
//!    いずれも構文解析ではなく表層的な語彙パターンによる緩和なので、
//!    「直った」と主張はしない——`web/eval/searchEval.ts`の手動サンプル
//!    再確認で実測してから精度の数字を更新すること
//!    （[[hearst-relations-measured-precision]]参照）。分布的経路
//!    （経路1）もこの種のペアに正の非対称包含スコアを与えることが
//!    あり、`status: Confirmed`（両経路一致）になっても誤りうる。
//!    Web側は`status`を必ずバッジとして見せ、Confirmedであっても
//!    「統計的な傾向と本文の一文が一致した」以上の確実性を主張しない。
//!
//! ## 実測結果（2026-09-03、3回の修正サイクル）
//!
//! Confirmedを全件人手確認: 当初36%（84件中30件）→ 2と3の修正後45%
//! （42件中19件）→ さらに窓計算のバグ（下記）を直した後50%（38件中
//! 19件）。的を絞った表層パターンの修正で精度はほぼ倍になったが、
//! 依然として半数近くが誤りである。残った誤りを分類すると、当初の
//! 「述語的名詞句」「主語の誤帰属」という2分類では捉えきれない、
//! 今回新たに判明した3つの型が主要因になっている（構文解析なしには
//! いずれも一般には解決できない）:
//!
//! - **定理固有の主張の一般化**（最多、約4割）。"the generic fiber is
//!   a reductive group"のような文は、この論文のこの定理の**仮定の下で
//!   だけ**成り立つ主張であって、「generic fiber一般がreductive groupの
//!   一種である」という定義的分類ではない。"is equivalent to"/
//!   "coincides with"にも同型の誤りがある（"infinite-dimensional
//!   weight space ≡ weight lattice"は特定の証明の中での一致であって、
//!   2つの概念が一般に同じものという意味ではない）。表層的な手がかりが
//!   無く（前置詞句でも性質名詞でもない）、今回は対処していない。
//! - **列挙パターンの係り先誤り**。"such as"/"including"/"like"が、
//!   直前の名詞句ではなく、もっと離れた真の被修飾語に係っている場合
//!   （"methods to construct solutions to the constraint equations,
//!   including the conformal method"で"including"は"methods"に係る
//!   のに"constraint equations"の特殊化として拾ってしまう）。
//! - **候補フレーズ抽出そのものの不備**。動詞やその他の非名詞的要素が
//!   混入した文法的に不完全なフレーズ（"one-player game played"）や、
//!   形容詞が脱落したことで別概念になってしまうフレーズ（"completely
//!   reducible"だけでは名詞句として成立しない）——`concepts.rs`の候補
//!   抽出の質に依存する上流の問題で、`relations.rs`単体では直せない。
//!
//! これらは`relations.rs`の対象外の問題（定理の適用範囲の判定、
//! 構文的な係り受け解析、候補フレーズ抽出の質）であり、次に精度を
//! 上げるには本格的な構文解析かLLMベースの文単位の判定が必要——
//! 表層パターンの追加調整では対応できる範囲を超えつつある。
//!
//! ## LLMベースの文単位検証を試して見送った記録（2026-09-04）
//!
//! 上記3分類を解決するため、Ollama経由のローカルLLM（`embed.rs`と同じ
//! アーキテクチャ）にHearstヒット1件ごとの妥当性を判定させる`verify.rs`
//! を実装し、実測した。結果は**いずれも十分な信頼性に達せず、不採用**
//! とした（コードは削除済み——理由は下記）。
//!
//! 試した内容: qwen2.5:3b-instruct / qwen2.5:7b-instruct の2モデル、
//! JSON強制出力＋3軸個別判定（well_formed/general_claim/
//! correctly_attached）、few-shot例つき、単一軸への統合（chain-of-
//! thought＋末尾JSON）、自己整合性（温度0.7で5サンプルの多数決）——
//! 複数の設計変更を実データ由来の14件の既知ケース（正誤既知）で評価。
//!
//! 実測: JSON強制出力は「常にfalse」に潰れる（3B・7Bとも）。
//! chain-of-thought＋貪欲デコードの3Bで最良12/14（85.7%）を1回得たが、
//! 語句を少し変えるだけで7/14まで落ちる——安定した能力ではなく、
//! 特定の言い回しに乗っただけの可能性が高い。7Bは3Bより遅い
//! （1件40〜60秒 対 10〜30秒）上に精度も低い（7/14、常にfalse寄りに
//! 潰れる）。自己整合性（5サンプル多数決）は投票が2-3/3-2のように
//! 割れることが多く、本物の一般的分類（cyclic codes⊂linear codes等、
//! これまでの実データ確認で正しいと分かっている例）の4〜5割を誤って
//! 却下する一方、誤った候補も4/7しか捕まえられなかった——公平な
//! コイントスと同等かそれ以下で、フィルタとして使うと良い関係を壊す
//! 方が多くなりかねない。
//!
//! **結論**: ローカルの3B/7B instructモデルは、「一般的な定義」と
//! 「この証明限りの個別の結論」を文単位で見分けるという課題に対して
//! 実用に足る安定性を持たない。この判定はより大きな（クラウドの）
//! モデルか、実際の構文解析器を要する可能性が高い——今回はどちらも
//! 環境制約・スコープの都合で選ばず、②は前回測定した50%
//! （38件中19件）の状態で確定させることにした。無効な判定を出す
//! フィルタを追加することは、それ自体が「作り話」の一種になりうる
//! ——実測せずに「精度が上がった」と称することは、この項目全体が
//! 一貫して避けてきたことそのものである。
//! 関連: [[llm-verification-not-reliable-for-relations]]
//!
//! ## LLM分類（統計のみのProposed候補への直接判定）を試して見送った記録（2026-09-05）
//!
//! 上の実験とは対象が異なる: あちらは「Hearstヒットの根拠文1文が正しい
//! 一般的主張か」という文単位の妥当性判定。こちらは`status: Proposed`
//! （根拠文を持たない、経路1の分布的非対称包含のみの候補、実データで
//! 142,948論文から103,096件・全関係の99%）を対象に、根拠文抜きで
//! 「(subject, kind, object)という主張そのものが数学的に正しいか」を
//! LLM自身の知識で判定させる、という別の課題として改めて試した——
//! `web/eval/`ではなく実際のconcept_relationsテーブルから層別抽出した
//! 30件（confidence帯4段×kind2種）を手動でground truth付けした上で、
//! さらに「常にfalseに潰れていないか」を検出するための健全性チェックとして
//! 教科書的に明確な正例8件（cyclic group⊂abelian group等）を追加した
//! 計38件で評価。モデルはqwen2.5:3b-instruct（前回実験で3Bが7Bより
//! 精度・速度とも上だったため）、prompt構成は前回唯一最良だった
//! 「reasoning→末尾JSON」（JSON強制出力は前回「常にfalse」に潰れると
//! 分かっているため使わない）。**言い回しを変えた2種のprompt**で独立に
//! 全件実行し、前回の失敗の核心だった「言い回し耐性」を直接検証した。
//!
//! 実測（実データ由来28件、malformed/incorrectの既知ラベル）: **2種の
//! prompt双方で28/28件を正しく却下**（malformed/incorrect、根拠：
//! 実データの`Proposed`上位候補は"ira gessel"（人名）・スペイン語の
//! 文断片・"links related"のような壊れた語句断片・無関係な概念同士の
//! 共起ノイズが大半で、除外すべき理由が表層的にも明確なケースが
//! 支配的だったため——前回のHearst文検証がより微妙な語用論的判定
//! だったのとは対照的）。
//!
//! 一方、教科書的に明確な**正例**8件では非対称な結果: 1つ目のprompt
//! では8/8正しく採用したが、**言い回しを変えただけの2つ目のprompt**
//! では5/8に低下——しかも失敗3件（"prime numberはnatural numberの
//! 特殊化か"等）はいずれも、モデル自身が出したreasoning文では正しく
//! 「Yes、〜という理由で真」と述べていながら、末尾のJSON `verdict`
//! フィールドだけが矛盾して`"incorrect"`になるという、reasoning文と
//! 出力ラベルが食い違う不具合だった。実際に使うのはJSONフィールドの
//! 方なので、この不整合はそのまま誤った却下として現れる。
//!
//! **この時点での結論**: 「却下（malformed/incorrect）」の判定は2種の
//! 言い回しで安定して100%だったが、「採用（correct）」の判定は言い回しを
//! 変えるだけで81%（13/16）まで落ち、しかも失敗はreasoningとJSONの
//! 内部矛盾という前回と同型の「言い回し依存の不安定さ」だった。この時点
//! では、統計で判別できない候補を確定関係として格上げする機能は実装しない
//! と判断した。コードはセッションのスクラッチパッドに残しただけで本体には
//! 追加しなかった。
//!
//! ## 追記: ユーザー指摘による再検証とcascade実装（2026-09-05・同日）
//!
//! ユーザーから3点の指摘を受けた: (1) 採用側n=8は小さすぎる、
//! (2) reasoning文とJSON verdictの不整合は意味論ではなく出力形式の
//! 問題では、(3) 却下は3B・採用は7Bという非対称モデル割当を試すべき。
//! 同じ38件の資産で追加検証したところ:
//!
//! - 温度0・同一prompt・3種のseedで29/29件が完全に同一の判定——言い回し
//!   を変えたときの揺れはサンプリングノイズではなく本物の言い回し依存
//!   だったと確認（指摘(1)への回答: n=8→29に拡張すると1つ目のprompt
//!   自体も82.8%に低下、8/8は小標本の偶然）。
//! - CoT結論文をそのままラベルにする方式（JSON verdict無し）は72.4%
//!   ——指摘(2)は部分的に当たっていた（一部はJSON形式の問題）が、
//!   reasoning自体が自己矛盾する例も残っており、形式だけの問題では
//!   なかった。
//! - **qwen2.5:7b-instructで同じ29件を実行したところ、2種の言い回し
//!   双方で29/29（満点・安定）**。却下側も27/28。速度は1件約10秒
//!   （前回実験の別課題では40〜60秒だったのとは対照的）——指摘(3)が
//!   的中し、3Bの採用側の弱さはモデル規模の問題だったと判明した。
//!
//! これを受けて3B→7B cascadeを`crates/mathesis-taxonomy/src/
//! llm_judge.rs`として実装し（3Bが却下と判定すればそのまま信頼、
//! 3Bが採用と判定した場合だけ7Bに再確認させる）、`mathesis-taxonomy
//! llm-judge`コマンド（DBは変更しない検証専用）で実データ
//! （103,096件の`Proposed`からstride抽出150件）に対して実行した。
//!
//! **実データでの結果は手作りテストより厳しかった**: 5.3秒/件・7Bへの
//! 昇格率3.3%とコスト面は設計通りだったが、最終的な「採用」件数は
//! 150件中わずか**2件（1.3%）**で、しかもその2件を筆者が数学的に
//! 精査したところ両方とも怪しい（1件は主語自体が"curves"を欠いた
//! 断片、もう1件は主張された包含関係自体が疑わしい）。却下側
//! （incorrect/malformed、148件）を無作為に目視確認した範囲では判断は
//! 概ね妥当だった。結論: 却下側のcascade設計は実データでも実証された
//! ため`llm_judge.rs`は本体に残したが、**採用側の出力は無条件に信頼
//! できない**——実データの`Proposed`には手作りベンチマークが想定した
//! ような「教科書的にクリーンな正例」がほとんど無く、大半が無関係な
//! ノイズか、専門知識と文脈が要る境界的なペアで占められているため。
//! Web側への反映（Confirmed/Grounded相当への格上げ）は、この出力を
//! そのまま自動昇格させる設計ではなく、人手レビューを挟む前提でなければ
//! 着手しないことにした。
//! 関連: [[llm-classification-rejects-reliably-but-not-accepts]]
//!
//! ## 追記2: 追加バッチによるrule-of-three検証と正式凍結（2026-09-05・同日）
//!
//! ユーザー提案（150件中2件はどちらも怪しく「収量が小さい」ではなく
//! 「確認できた真陽性が実質ゼロ」と読むべき、安価な追加バッチで
//! rule-of-threeの見積もりを固めてから凍結判断せよ）に従い、
//! stride抽出450件を追加実行した（5.5秒/件・7Bへの昇格9件[2.0%]、
//! correct 2件・incorrect 312件・malformed 136件）。
//!
//! **判明した誤算**: 第1バッチ(limit=150→stride=687)と第2バッチ
//! (limit=450→stride=229)は687=3×229という整数倍の関係だったため、
//! 第2バッチ450件中150件は第1バッチと同一の抽出位置だった——真に
//! ユニークな新規カバレッジは300件のみ。実際、第2バッチの"correct"の
//! 1つは第1バッチの怪しい正解と文字列が完全一致（重複）。もう1つ、
//! 第1バッチで"correct"だった"fixed topological type ⊂ stable
//! curves"は同じ抽出位置で再実行すると**"incorrect"に変わっていた**
//! ——温度0・seed固定でも同一入力の判定が変わりうることが分かり、
//! cascadeの決定性への留保が増えた。
//!
//! 一方、第2バッチで新規に見つかった"correct"の1件——"symmetric
//! stable processes ⊂ stable processes"——は数学的に精査した結果、
//! 対称安定過程は安定過程の特殊ケースであり**正しい**包含関係だった。
//! 2バッチを通じて唯一、妥当性に疑問の余地がない正例。
//!
//! **正式凍結**: 重複を除いた実質600件弱のユニーク母集団に対し、
//! 妥当な正例はこの1件のみ。accept側／Web反映ルートを正式に凍結する
//! ——却下側の`llm_judge.rs`は資産として残すが、Proposed→Confirmed/
//! Grounded相当への自動格上げは実装しない。今後は分類器自体の改善
//! ではなく`concepts.rs`の候補フレーズ抽出品質や`\ref`/`\cite`ベース
//! の決定的抽出など上流側への投資の方がROIが高いと考えられる
//! （未着手）。
//! 詳細: [[llm-classification-rejects-reliably-but-not-accepts]]
//! （更新済み）参照。

use std::collections::hash_map::Entry;
use std::collections::HashMap;

use crate::context::SparseSym;
use mathesis_ingest::model::Paper;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RelationKind {
    /// subjectはobjectの特殊化（subject IsA object、より狭い）。
    SpecializationOf,
    /// 同一概念の異なる呼び方・定式化（"brownian motion" / "wiener process"）。
    EquivalentTo,
}

/// 関係の裏付けの強さ。`mathesis-graph`の射の`status: proposed`と同じ
/// 精神——確定した事実であるかのように見せない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationStatus {
    /// 分布的非対称包含のみ。根拠文は無い。
    Proposed,
    /// Hearstパターンのみ。根拠文（実際の論文の一文）を持つが、
    /// 統計的な裏付けは無い（1論文の言い回しにすぎない可能性がある）。
    Grounded,
    /// 両経路が一致。最も信頼できる。
    Confirmed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RelationEdge {
    pub subject: String,
    pub object: String,
    pub kind: RelationKind,
    pub status: RelationStatus,
    /// 経路1の場合はinvCL、経路2の場合は根拠文の件数から決めた値
    /// （複数の論文が同じ関係を述べるほど高い）。両方揃えばこれらの
    /// 合成——単純な比較はできないので、確認済みかどうか（`status`）を
    /// 主指標にし、`confidence`は同じstatus内での順序付けにのみ使う。
    pub confidence: f32,
    pub evidence_sentence: Option<String>,
    pub evidence_arxiv_id: Option<String>,
}

// ============================================================
// 経路1: 分布的非対称包含
// ============================================================

pub type Adjacency = Vec<HashMap<u32, f32>>;

pub fn build_adjacency(matrix: &SparseSym) -> Adjacency {
    let mut adj: Adjacency = vec![HashMap::new(); matrix.n];
    for &(i, j, w) in &matrix.entries {
        adj[i as usize].insert(j, w);
        adj[j as usize].insert(i, w);
    }
    adj
}

/// WeedsPrec(u→v)。「uの文脈のうち、vの文脈にも含まれる重みの割合」。
/// uに文脈が無ければ0（孤立概念は特殊化の判定対象にならない）。
pub fn weeds_prec(adj: &Adjacency, u: usize, v: usize) -> f32 {
    let nu = &adj[u];
    if nu.is_empty() {
        return 0.0;
    }
    let total: f32 = nu.values().sum();
    if total <= 0.0 {
        return 0.0;
    }
    let nv = &adj[v];
    let included: f32 = nu.iter().filter(|(k, _)| nv.contains_key(k)).map(|(_, w)| w).sum();
    included / total
}

/// Lenci & Benotto (2012) のinvCL。uがvに包含され、かつvがuに包含され
/// ない度合いが強いほど大きい——「uはvの特殊なケース」という非対称性の
/// 強さそのものを1つの数値にする。
pub fn inv_cl(adj: &Adjacency, u: usize, v: usize) -> f32 {
    let p_uv = weeds_prec(adj, u, v);
    let p_vu = weeds_prec(adj, v, u);
    (p_uv * (1.0 - p_vu)).max(0.0).sqrt()
}

#[derive(Debug, Clone)]
pub struct DistributionalParams {
    /// 両方向のWeedsPrecがこれ以上なら「同値」とみなす。
    pub equivalent_min: f32,
    /// 一方向のinvCLがこれ未満なら特殊化とはみなさない。
    pub specialization_min: f32,
    /// 逆方向とのinvCLの差がこれ未満なら「向きが弱く区別が付かない」として
    /// 見送る（非対称性そのものが特殊化の証拠なので、ほぼ対称なら不採用）。
    pub margin: f32,
}

/// 候補ペア(u,v)を分類する。返り値は (subject, object, kind, confidence)。
/// 特殊化ならsubjectが狭い方（obj寄りではない方）。判定できなければNone。
pub fn classify_pair(
    adj: &Adjacency,
    u: usize,
    v: usize,
    params: &DistributionalParams,
) -> Option<(usize, usize, RelationKind, f32)> {
    let p_uv = weeds_prec(adj, u, v);
    let p_vu = weeds_prec(adj, v, u);
    if p_uv >= params.equivalent_min && p_vu >= params.equivalent_min {
        return Some((u, v, RelationKind::EquivalentTo, p_uv.min(p_vu)));
    }
    let icl_uv = (p_uv * (1.0 - p_vu)).max(0.0).sqrt();
    let icl_vu = (p_vu * (1.0 - p_uv)).max(0.0).sqrt();
    if icl_uv >= params.specialization_min && icl_uv - icl_vu >= params.margin {
        return Some((u, v, RelationKind::SpecializationOf, icl_uv));
    }
    if icl_vu >= params.specialization_min && icl_vu - icl_uv >= params.margin {
        return Some((v, u, RelationKind::SpecializationOf, icl_vu));
    }
    None
}

// ============================================================
// 経路2: Hearstパターン
// ============================================================

/// 接続表現とその効果。「1つ目に出現した既知フレーズ」を`first`、
/// 「2つ目に出現した既知フレーズ」を`second`として、glueテキスト
/// （2つの出現の間の文字列）がこの接続表現と一致したときの関係の向き。
enum Effect {
    /// firstはsecondの特殊化（"X is a Y"型）。
    FirstSpecializesSecond,
    /// secondはfirstの特殊化（"Y such as X"型、"X generalizes Y"型）。
    SecondSpecializesFirst,
    Equivalent,
    /// "we generalize X to Y"型。glueが"to"/"into"であるだけでは
    /// 無関係な文にまで誤爆するので、firstの直前にgeneralize系の動詞が
    /// あるときだけ有効にする（`extract_hearst_hits`側で確認）。
    /// 有効ならfirstがsecondの特殊化（firstを一般化した先がsecond）。
    GeneralizingTo,
}

/// `text_before`（対象出現の直前、最大40文字程度）に"generalize"系の
/// 動詞（英米つづり・活用形いずれも）が含まれるか。
fn has_generalizing_verb_nearby(text_before: &str) -> bool {
    const VERB_FORMS: &[&str] = &[
        "generalize ", "generalizes ", "generalizing ", "generalized ",
        "generalise ", "generalises ", "generalising ", "generalised ",
    ];
    VERB_FORMS.iter().any(|v| text_before.contains(v))
}

/// 接続表現の一覧。glueを正規化（小文字化・句読点をスペースに置換・
/// 連続空白を1つに畳む・前後trim）した文字列が**完全一致**したときだけ
/// 効果を適用する——部分一致にすると無関係な文にも誤爆するため。
///
/// 英語のNP境界を解析しようとしない設計（モジュール冒頭参照）の裏返しで、
/// このリストに無い言い回しは取りこぼす。取りこぼしは重複が漏れるだけ
/// だが、誤った関係を作るよりましという判断——`resolve.rs`と同じ側に
/// 倒している。
const ANCHORS: &[(&str, Effect)] = &[
    ("is a", Effect::FirstSpecializesSecond),
    ("is an", Effect::FirstSpecializesSecond),
    ("are a", Effect::FirstSpecializesSecond),
    ("is a kind of", Effect::FirstSpecializesSecond),
    ("are a kind of", Effect::FirstSpecializesSecond),
    ("is a type of", Effect::FirstSpecializesSecond),
    ("are a type of", Effect::FirstSpecializesSecond),
    ("is a special case of", Effect::FirstSpecializesSecond),
    ("are special cases of", Effect::FirstSpecializesSecond),
    ("is a particular case of", Effect::FirstSpecializesSecond),
    ("is a special type of", Effect::FirstSpecializesSecond),
    ("is a subclass of", Effect::FirstSpecializesSecond),
    ("are a subclass of", Effect::FirstSpecializesSecond),
    // 同格パターン "X, a Y," は意図的に含めていない。実データ
    // （142,948論文）で試したところ、英語のカンマは同格以外の役割
    // （導入節・従属節境界・列挙）でも極めて頻繁に使われ、
    //   "Focusing on energy efficiency, a noncooperative game is
    //    proposed in which …"（"energy efficiency"は次節の主語ではない）
    //   "For a Gaussian process, a natural definition of the integral
    //    follows from …"（同格ではなく単なる導入節）
    // のような**文法的に誤った**関係を量産した。"is a"系（動詞を介する
    // ため誤読されにくい）は残すが、カンマだけを手がかりにする同格は
    // 精度が低すぎたため見送る——`resolve.rs`が畳み過ぎを避けたのと
    // 同じ判断（誤った関係を作るより取りこぼす方が安全）。
    ("such as", Effect::SecondSpecializesFirst),
    ("including", Effect::SecondSpecializesFirst),
    ("like", Effect::SecondSpecializesFirst),
    ("for example", Effect::SecondSpecializesFirst),
    ("for instance", Effect::SecondSpecializesFirst),
    ("generalizes", Effect::SecondSpecializesFirst),
    ("generalize", Effect::SecondSpecializesFirst),
    ("is a generalization of", Effect::SecondSpecializesFirst),
    ("is a natural generalization of", Effect::SecondSpecializesFirst),
    ("are generalizations of", Effect::SecondSpecializesFirst),
    // "we generalize X to Y" のような、動詞が2つのフレーズの**前**に来る
    // パターン。glueだけでは "to" という語だけが挟まり、"to"単体を接続
    // 表現にすると無関係な文にまで誤爆する。そこで「直前に
    // generalize系の動詞があるときだけ」という条件を`extract_hearst_hits`
    // 側で別途チェックし、ここでは通常のglueマッチが起きない
    // プレースホルダとしてのみ扱う（`GeneralizingTo`は特別扱い）。
    ("to", Effect::GeneralizingTo),
    ("into", Effect::GeneralizingTo),
    ("is equivalent to", Effect::Equivalent),
    ("are equivalent to", Effect::Equivalent),
    ("if and only if", Effect::Equivalent),
    ("iff", Effect::Equivalent),
    ("is also known as", Effect::Equivalent),
    ("is also called", Effect::Equivalent),
    ("also known as", Effect::Equivalent),
    ("coincides with", Effect::Equivalent),
    ("is the same as", Effect::Equivalent),
    ("are the same as", Effect::Equivalent),
];

fn normalize_glue(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last_was_space = true;
    for c in raw.chars() {
        let mapped = if c.is_alphanumeric() { c.to_ascii_lowercase() } else { ' ' };
        if mapped == ' ' {
            if !last_was_space {
                out.push(' ');
            }
            last_was_space = true;
        } else {
            out.push(mapped);
            last_was_space = false;
        }
    }
    out.trim().to_string()
}

/// 末尾の冠詞（a/an/the）を1つだけ取り除く。冠詞は接続表現の一部ではなく
/// 直後の名詞句（＝既知フレーズ）の冠詞であることが多い
/// （"is equivalent to a Wiener process" の"a"等）。取り除いた結果が
/// 空になる場合（glue全体が冠詞1語だけ、＝同格パターン）は取り除かない
/// ——そちらは別の判定で扱う。
fn strip_trailing_article(glue: &str) -> Option<String> {
    let mut words: Vec<&str> = glue.split(' ').collect();
    if words.len() < 2 {
        return None;
    }
    let last = *words.last().unwrap();
    if last == "a" || last == "an" || last == "the" {
        words.pop();
        Some(words.join(" "))
    } else {
        None
    }
}

/// 裸の"is a"/"is an"/"are a"（"is a kind of"のような分類語を含む長い
/// 接続表現は対象外——そちらは目的語が何であれ分類関係を明示している）
/// の目的語の主辞（最後の単語）がこの一覧に入っていれば、性質の主張
/// （述語的名詞句）である可能性が高いとみなしてヒットを見送る。
/// 84件の手動確認で実際に誤りだった例（"convex function", "energy
/// function"等）から一般化した、数学論文でよく性質・統計量を表す名詞。
/// 網羅的ではない——ここに無い語で同種の誤りが今も起こりうる。
const PREDICATE_NOMINAL_HEAD_BLACKLIST: &[&str] = &[
    "function", "functional", "functions", "functionals",
    "measure", "measures", "map", "maps", "mapping", "mappings",
    "operator", "operators", "invariant", "invariants",
    "quantity", "quantities", "statistic", "statistics",
    "metric", "metrics", "norm", "norms", "matrix", "matrices",
    "vector", "vectors", "sequence", "sequences", "series",
    "polynomial", "polynomials", "distribution", "distributions",
    "parameter", "parameters", "coefficient", "coefficients",
    "bound", "bounds", "constant", "constants", "value", "values",
    "number", "numbers", "estimate", "estimates", "solution",
    "solutions", "condition", "conditions", "property", "properties",
    // 2026-09-03の実測(42件のConfirmed全件確認)で追加で見つかった誤り:
    // "time delay ... is a common feature of quantum scattering theory"
    // （time delayは「性質」を主張されているだけで、common featuresの
    // 一種ではない）、"The diamond cone is a combinatorial description
    // for a basis"（diamond coneが何を表すかの役割の説明であって、
    // combinatorial descriptionsという分類の一員ではない）。
    "feature", "features", "description", "descriptions",
];

const BARE_IS_A_ANCHORS: &[&str] = &["is a", "is an", "are a"];

fn object_is_predicate_nominal(object_phrase: &str) -> bool {
    object_phrase
        .rsplit(' ')
        .next()
        .map(|last| PREDICATE_NOMINAL_HEAD_BLACKLIST.contains(&last))
        .unwrap_or(false)
}

/// 主語候補（`Effect::FirstSpecializesSecond`のfirst occurrence）の
/// 直前・同じ節内（カンマをまたがない）に前置詞があれば、その候補は
/// 前置詞句の内部にあり真の節の主語ではない可能性が高い——
/// "... with a nontrivial finite-dimensional weight space, is a
/// Harish-Chandra module"の"weight space"のような誤帰属を防ぐ。
/// カンマが window 内にあれば（＝前置詞句がこの候補より前の別の節に
/// あれば）誤帰属のリスクは下がるため対象外とする。
const CLAUSE_INTERNAL_PREPOSITIONS: &[&str] = &[
    "of", "with", "in", "on", "for", "by", "via", "under", "within",
    "among", "between", "using", "from", "over", "without", "upon",
    "into", "onto", "through", "throughout", "despite", "during",
    "after", "before", "above", "below", "near", "toward", "towards",
    "against", "along", "across", "about", "to",
    // "to"は実データで見つかった実例で欠けていた: "the transition to a
    // superconducting state is a second-order phase transition"で
    // "superconducting state"を主語と誤認した（真の主語は
    // "the transition"）。GeneralizingToのglueとしての"to"とは無関係
    // （こちらは候補フレーズ**直前**の語を見る別のチェック）。
];

fn subject_is_pp_internal(window_text: &str) -> bool {
    // 固定長の窓は、直前の節とは無関係な、もっと手前にある別の節の
    // カンマまで拾ってしまうことがある——実データで見つかった実例:
    // "..., examining the thermodynamical potential, a mathematical
    // proof that the transition to a superconducting state is a
    // second-order phase transition." では、"potential,"のカンマが
    // 60文字の窓に入ってしまい、"to a"という真の手がかりが「カンマが
    // 窓内にあるから無視」で見逃されていた。直近のカンマより後ろだけを
    // 見れば、そのカンマが実際に手前の節との境界であっても問題ない。
    let clause = window_text.rfind(',').map(|i| &window_text[i + 1..]).unwrap_or(window_text);
    clause.split(|c: char| !c.is_alphanumeric()).any(|word| CLAUSE_INTERNAL_PREPOSITIONS.contains(&word))
}

struct Occurrence<'a> {
    start: usize,
    end: usize,
    phrase: &'a str,
}

/// `haystack`（小文字化済み）の中で`phrase`が単語境界を守って出現する
/// 全位置を返す。`subfields`の中の`field`のような部分一致を弾く。
fn find_word_boundary_occurrences<'a>(haystack: &str, phrase: &'a str) -> Vec<Occurrence<'a>> {
    let mut out = Vec::new();
    if phrase.is_empty() {
        return out;
    }
    let bytes = haystack.as_bytes();
    let mut from = 0;
    while let Some(rel) = haystack[from..].find(phrase) {
        let start = from + rel;
        let end = start + phrase.len();
        let before_ok = start == 0 || !is_word_byte(bytes[start - 1]);
        let after_ok = end >= bytes.len() || !is_word_byte(bytes[end]);
        if before_ok && after_ok {
            out.push(Occurrence { start, end, phrase });
        }
        from = start + 1;
        if from >= haystack.len() {
            break;
        }
    }
    out
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric()
}

/// `idx`以下で最も近いUTF-8文字境界を返す。`str`のスライスは文字境界を
/// またぐとパニックするため、バイトオフセットを固定長だけ遡らせる処理
/// （`GeneralizingTo`の直前文脈チェック）で必要になる。
fn safe_boundary_back(text: &str, idx: usize) -> usize {
    let mut i = idx.min(text.len());
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// 出現位置を含む「文」を切り出す（表示用）。前後最も近い区切り記号
/// （. ! ?）まで、見つからなければテキストの端まで。
fn enclosing_sentence(text: &str, start: usize, end: usize) -> String {
    let before = text[..start].rfind(['.', '!', '?']).map(|i| i + 1).unwrap_or(0);
    let after = text[end..].find(['.', '!', '?']).map(|i| end + i + 1).unwrap_or(text.len());
    text[before..after].trim().to_string()
}

#[derive(Debug, Clone)]
pub struct HearstHit {
    pub subject: String,
    pub object: String,
    pub kind: RelationKind,
    pub sentence: String,
    pub arxiv_id: String,
}

/// `known_phrases`は、この論文に既に紐付いている候補フレーズのうち
/// 2語以上のもの（呼び出し側で`word_count >= 2`にあらかじめ絞る——
/// `cluster`がクラスタリング対象を複合語に絞っているのと同じ理由:
/// 単語1語は広い分野名・定型語が混じりやすくノイズになる）。
pub fn extract_hearst_hits(paper: &Paper, known_phrases: &[String]) -> Vec<HearstHit> {
    let text = format!("{} {}", paper.title, paper.abstract_text);
    let lower = text.to_lowercase();

    let mut occurrences: Vec<Occurrence> = Vec::new();
    for phrase in known_phrases {
        occurrences.extend(find_word_boundary_occurrences(&lower, phrase));
    }
    if occurrences.len() < 2 {
        return Vec::new();
    }
    occurrences.sort_by_key(|o| o.start);

    let mut hits = Vec::new();
    for pair in occurrences.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if b.start < a.end {
            continue; // 重なる出現（片方がもう片方の部分文字列）はスキップ。
        }
        let glue_raw = &lower[a.end..b.start];
        // Hearstパターンは短い接続表現なので、離れすぎたペアは見ない
        // （無関係な文をまたいだ誤爆を防ぐ）。
        if glue_raw.len() > 40 {
            continue;
        }
        let glue = normalize_glue(glue_raw);
        // 直接一致しなければ、末尾の冠詞（a/an/the）を1つ落として
        // もう一度試す——"is equivalent to a Wiener process" の"a"は
        // 接続表現の一部ではなく次の名詞句の冠詞。
        let matched = ANCHORS
            .iter()
            .find(|(anchor, _)| *anchor == glue)
            .or_else(|| {
                strip_trailing_article(&glue)
                    .and_then(|stripped| ANCHORS.iter().find(|(anchor, _)| *anchor == stripped))
            });
        let Some((anchor_text, effect)) = matched else {
            continue;
        };
        if matches!(effect, Effect::FirstSpecializesSecond) {
            // 主語誤帰属の緩和: "a"（subject候補）の直前が前置詞句の
            // 内部なら、真の主語ではない可能性が高いので見送る。
            let window_start = safe_boundary_back(&lower, a.start.saturating_sub(60));
            if subject_is_pp_internal(&lower[window_start..a.start]) {
                continue;
            }
            // 述語的名詞句の緩和: 裸の"is a"系（"is a kind of"等の分類語を
            // 含む長い接続表現は対象外）でobjectの主辞が性質・統計量を
            // 表す一般名詞なら、分類ではなく性質の主張の可能性が高い。
            if BARE_IS_A_ANCHORS.contains(anchor_text) && object_is_predicate_nominal(b.phrase) {
                continue;
            }
        }
        let (subject_span, object_span, kind) = match effect {
            Effect::FirstSpecializesSecond => (a, b, RelationKind::SpecializationOf),
            Effect::SecondSpecializesFirst => (b, a, RelationKind::SpecializationOf),
            Effect::Equivalent => (a, b, RelationKind::EquivalentTo),
            Effect::GeneralizingTo => {
                // "we generalize X to Y" 型。"to"/"into"だけでは無関係な
                // 文にまで誤爆するので、firstの直前にgeneralize系の動詞が
                // あるときだけ採用する。UTF-8の文字境界をまたがないよう
                // 安全な位置まで戻す（Kählerのような非ASCII語が直前に
                // あってもパニックしない）。
                let window_start = safe_boundary_back(&lower, a.start.saturating_sub(40));
                if !has_generalizing_verb_nearby(&lower[window_start..a.start]) {
                    continue;
                }
                (a, b, RelationKind::SpecializationOf)
            }
        };
        let sentence = enclosing_sentence(&text, a.start.min(b.start), a.end.max(b.end));
        hits.push(HearstHit {
            subject: subject_span.phrase.to_string(),
            object: object_span.phrase.to_string(),
            kind,
            sentence,
            arxiv_id: paper.arxiv_id.clone(),
        });
    }
    hits
}

// ============================================================
// 統合
// ============================================================

/// 経路1・経路2の結果を1本化する。同じ(subject, object, kind)が両方に
/// あれば`Confirmed`、片方だけなら`Grounded`（Hearst）または
/// `Proposed`（分布的）。Hearst側は複数論文からの根拠がありうるので、
/// 最初に見つかった1件を代表の根拠文として残す（複数件を全部持たせると
/// 配信量が増えるだけで、利用者は1件読めば同じ判断ができる）。
pub fn merge(
    distributional: Vec<(String, String, RelationKind, f32)>,
    hearst: Vec<HearstHit>,
) -> Vec<RelationEdge> {
    let mut by_key: HashMap<(String, String, RelationKind), RelationEdge> = HashMap::new();

    for (subject, object, kind, confidence) in distributional {
        by_key.insert(
            (subject.clone(), object.clone(), kind),
            RelationEdge { subject, object, kind, status: RelationStatus::Proposed, confidence, evidence_sentence: None, evidence_arxiv_id: None },
        );
    }

    for hit in hearst {
        let key = (hit.subject.clone(), hit.object.clone(), hit.kind);
        match by_key.entry(key) {
            Entry::Occupied(mut o) => {
                // 分布的経路が先に見つけていた辺。両経路が一致したので
                // Confirmedへ格上げし、根拠文もここで初めて付く
                // （分布的経路には根拠文という概念自体が無いため）。
                let e = o.get_mut();
                e.status = RelationStatus::Confirmed;
                if e.evidence_sentence.is_none() {
                    e.evidence_sentence = Some(hit.sentence);
                    e.evidence_arxiv_id = Some(hit.arxiv_id);
                }
            }
            Entry::Vacant(v) => {
                v.insert(RelationEdge {
                    subject: hit.subject,
                    object: hit.object,
                    kind: hit.kind,
                    status: RelationStatus::Grounded,
                    confidence: 1.0,
                    evidence_sentence: Some(hit.sentence),
                    evidence_arxiv_id: Some(hit.arxiv_id),
                });
            }
        }
    }

    let mut out: Vec<RelationEdge> = by_key.into_values().collect();
    out.sort_by(|a, b| a.subject.cmp(&b.subject).then_with(|| a.object.cmp(&b.object)));
    remove_specialization_cycles(out)
}

/// 特殊化は半順序であるべきなので、AがBの特殊化かつBもAの特殊化、という
/// 矛盾する循環が残っていてはいけない。分布的経路（`classify_pair`）は
/// 非対称性そのものを判定条件にしているので原理的に両方向を出さないが、
/// Hearst経路は論文ごとに独立に動くため、**同じ論文の別々の文**が
/// 矛盾する主張をすると循環になりうる。実データで実際に1件見つかった:
///
///   "An Einstein nilradical is a nilpotent Lie algebra" （定義的な事実）
///   "a nilpotent Lie algebra ... is an Einstein nilradical"
///   （特定の例がEinstein nilradicalという性質を満たす、という主張）
///
/// 同じ"is a"パターンが定義的な分類と個別の性質充足の両方に使われる
/// （モジュール冒頭の「述語的名詞句の誤検出」と同型の限界）ため、
/// どちらが正しいか自動では判定できない。判定できない以上、両方を
/// 残して矛盾したまま見せるより、**両方落とす**方が安全
/// ——`resolve.rs`が「畳み過ぎるよりいたちごっこの方が安全」としたのと
/// 同じ判断を、逆方向（残し過ぎより両方落とす方が安全）に適用している。
fn remove_specialization_cycles(edges: Vec<RelationEdge>) -> Vec<RelationEdge> {
    let forward: std::collections::HashSet<(String, String)> = edges
        .iter()
        .filter(|e| e.kind == RelationKind::SpecializationOf)
        .map(|e| (e.subject.clone(), e.object.clone()))
        .collect();
    edges
        .into_iter()
        .filter(|e| {
            e.kind != RelationKind::SpecializationOf || !forward.contains(&(e.object.clone(), e.subject.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adjacency_from_pairs(n: usize, pairs: &[(usize, usize, f32)]) -> Adjacency {
        let matrix = SparseSym { n, entries: pairs.iter().map(|&(i, j, w)| (i as u32, j as u32, w)).collect() };
        build_adjacency(&matrix)
    }

    #[test]
    fn weeds_prec_is_one_when_narrow_context_is_fully_included_in_broad_context() {
        // 概念0（狭い）の文脈{2,3}は概念1（広い）の文脈{2,3,4,5}に完全に
        // 含まれる。WeedsPrec(0→1)は1.0、逆は1.0未満のはず。
        let adj = adjacency_from_pairs(
            6,
            &[(0, 2, 1.0), (0, 3, 1.0), (1, 2, 1.0), (1, 3, 1.0), (1, 4, 1.0), (1, 5, 1.0)],
        );
        assert!((weeds_prec(&adj, 0, 1) - 1.0).abs() < 1e-6);
        assert!(weeds_prec(&adj, 1, 0) < 1.0);
    }

    #[test]
    fn inv_cl_is_asymmetric_and_favors_the_narrower_direction() {
        let adj = adjacency_from_pairs(
            6,
            &[(0, 2, 1.0), (0, 3, 1.0), (1, 2, 1.0), (1, 3, 1.0), (1, 4, 1.0), (1, 5, 1.0)],
        );
        assert!(inv_cl(&adj, 0, 1) > inv_cl(&adj, 1, 0), "0(狭い)→1(広い)の方が非対称性が強いはず");
    }

    #[test]
    fn classify_pair_detects_specialization_direction() {
        let adj = adjacency_from_pairs(
            6,
            &[(0, 2, 1.0), (0, 3, 1.0), (1, 2, 1.0), (1, 3, 1.0), (1, 4, 1.0), (1, 5, 1.0)],
        );
        let params = DistributionalParams { equivalent_min: 0.95, specialization_min: 0.3, margin: 0.1 };
        let result = classify_pair(&adj, 0, 1, &params);
        assert!(result.is_some());
        let (subject, object, kind, _) = result.unwrap();
        assert_eq!((subject, object), (0, 1), "狭い方(0)がsubjectであるべき");
        assert_eq!(kind, RelationKind::SpecializationOf);
    }

    #[test]
    fn classify_pair_detects_equivalence_when_both_directions_are_high() {
        let adj = adjacency_from_pairs(4, &[(0, 2, 1.0), (0, 3, 1.0), (1, 2, 1.0), (1, 3, 1.0)]);
        let params = DistributionalParams { equivalent_min: 0.95, specialization_min: 0.3, margin: 0.1 };
        let result = classify_pair(&adj, 0, 1, &params);
        assert_eq!(result.map(|(_, _, k, _)| k), Some(RelationKind::EquivalentTo));
    }

    #[test]
    fn classify_pair_abstains_when_directions_are_too_symmetric() {
        // 0と1は文脈を部分的にしか共有しない({3,4}が共通、0は独自に5を、
        // 1は独自に6を持つ)。WeedsPrec(0→1)=WeedsPrec(1→0)=2/3で完全に
        // 対称——同値の閾値には届かず、非対称性も無いので特殊化とも
        // 言えない。どちらの判定も下すべきではない。
        let adj = adjacency_from_pairs(
            7,
            &[(0, 3, 1.0), (0, 4, 1.0), (0, 5, 1.0), (1, 3, 1.0), (1, 4, 1.0), (1, 6, 1.0)],
        );
        let params = DistributionalParams { equivalent_min: 0.95, specialization_min: 0.3, margin: 0.3 };
        assert!(classify_pair(&adj, 0, 1, &params).is_none());
    }

    fn paper(id: &str, title: &str, abstract_text: &str) -> Paper {
        Paper {
            arxiv_id: id.to_string(),
            title: title.to_string(),
            abstract_text: abstract_text.to_string(),
            authors: vec![],
            categories: vec![],
            msc_codes: vec![],
            submitted: "2020-01-01".to_string(),
        }
    }

    #[test]
    fn is_a_pattern_yields_specialization_in_first_to_second_direction() {
        let p = paper("1", "A note", "An elliptic curve is a smooth projective curve of genus one.");
        let known = vec!["elliptic curve".to_string(), "smooth projective curve".to_string()];
        let hits = extract_hearst_hits(&p, &known);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].subject, "elliptic curve");
        assert_eq!(hits[0].object, "smooth projective curve");
        assert_eq!(hits[0].kind, RelationKind::SpecializationOf);
        assert_eq!(hits[0].arxiv_id, "1");
    }

    #[test]
    fn such_as_pattern_reverses_direction() {
        let p = paper("2", "A survey", "Classical groups such as symplectic groups arise naturally.");
        let known = vec!["classical groups".to_string(), "symplectic groups".to_string()];
        let hits = extract_hearst_hits(&p, &known);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].subject, "symplectic groups");
        assert_eq!(hits[0].object, "classical groups");
        assert_eq!(hits[0].kind, RelationKind::SpecializationOf);
    }

    #[test]
    fn generalizes_pattern_reverses_direction() {
        let p = paper("3", "On generalizations", "This paper generalizes elliptic curves to abelian varieties.");
        let known = vec!["elliptic curves".to_string(), "abelian varieties".to_string()];
        let hits = extract_hearst_hits(&p, &known);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].subject, "elliptic curves");
        assert_eq!(hits[0].object, "abelian varieties");
        assert_eq!(hits[0].kind, RelationKind::SpecializationOf);
    }

    #[test]
    fn equivalent_pattern_is_detected() {
        let p = paper("4", "Note", "A Brownian motion is equivalent to a Wiener process in this sense.");
        let known = vec!["brownian motion".to_string(), "wiener process".to_string()];
        let hits = extract_hearst_hits(&p, &known);
        assert!(hits.iter().any(|h| h.kind == RelationKind::EquivalentTo));
    }

    #[test]
    fn unrelated_sentence_yields_no_hits() {
        let p = paper("5", "Note", "We study elliptic curves and separately discuss abelian varieties in section 3.");
        let known = vec!["elliptic curves".to_string(), "abelian varieties".to_string()];
        assert!(extract_hearst_hits(&p, &known).is_empty());
    }

    #[test]
    fn bare_comma_appositive_does_not_produce_a_false_relation() {
        // 回帰テスト。実データ（142,948論文）で標準の同格パターン
        // ("X, a Y,") を試したところ、カンマが導入節・従属節境界にも
        // 使われる英語の性質上、明らかに誤った関係を量産した:
        //   "Focusing on energy efficiency, a noncooperative game is
        //    proposed in which …"（"energy efficiency"は次節の主語ではない）
        // ANCHORSから同格パターンを外したので、このような文はもう
        // ヒットしないはず。
        let p = paper(
            "6",
            "Note",
            "Focusing on energy efficiency, a noncooperative game is proposed in which users choose their transmit powers.",
        );
        let known = vec!["energy efficiency".to_string(), "noncooperative game".to_string()];
        assert!(extract_hearst_hits(&p, &known).is_empty());
    }

    #[test]
    fn substring_of_another_known_phrase_does_not_produce_a_spurious_pair() {
        // "elliptic curve" と "elliptic curve theory" が両方既知フレーズ
        // でも、片方がもう片方に完全に含まれる出現は関係のペアにしない。
        let p = paper("6", "Note", "We develop elliptic curve theory here.");
        let known = vec!["elliptic curve".to_string(), "elliptic curve theory".to_string()];
        let hits = extract_hearst_hits(&p, &known);
        assert!(hits.is_empty(), "{hits:?}");
    }

    #[test]
    fn word_boundary_check_rejects_partial_word_matches() {
        let hay = "we study subfields of number fields here";
        let occ = find_word_boundary_occurrences(hay, "field");
        assert!(occ.is_empty(), "『field』は『subfields』『fields』の部分文字列としてマッチしてはいけない");
    }

    #[test]
    fn predicate_nominal_property_noun_is_not_treated_as_a_classification() {
        // 実データで見つかった実際の誤検出。"channel capacity"は
        // "convex function"の一種ではなく、"convexという性質を持つ"と
        // 主張しているだけ。目的語の主辞"function"がブラックリストに
        // あるので、裸の"is a"はヒットを作らないはず。
        let p = paper(
            "7",
            "Note",
            "The channel capacity is a convex function of the stochastic channel matrix.",
        );
        let known = vec!["channel capacity".to_string(), "convex function".to_string()];
        assert!(extract_hearst_hits(&p, &known).is_empty());
    }

    #[test]
    fn subclass_of_pattern_is_not_blocked_by_predicate_nominal_filter() {
        // ブラックリストは裸の"is a"系にのみ適用される。"is a subclass
        // of"のように分類語を含む長い接続表現は、objectの主辞が性質
        // 名詞と紛らわしくても対象外——今回の場合はそもそも"codes"は
        // ブラックリストに無いが、分類語つきの接続表現が誤って見送られ
        // ないことも確認する。
        let p = paper("8", "Note", "Cyclic codes are a subclass of linear codes.");
        let known = vec!["cyclic codes".to_string(), "linear codes".to_string()];
        let hits = extract_hearst_hits(&p, &known);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].subject, "cyclic codes");
        assert_eq!(hits[0].object, "linear codes");
    }

    #[test]
    fn subject_inside_prepositional_phrase_is_not_misattributed() {
        // 実データで見つかった実際の誤検出のパラフレーズ。"is a"の直前に
        // 最も近い既知フレーズ"weight space"は"with a ... weight space"
        // という前置詞句の内部にあり、真の主語ではない。この場合は
        // 正しい主語を推測しようとはせず、ヒット自体を見送るべき。
        let p = paper(
            "9",
            "Note",
            "Every simple weight module with a nontrivial finite-dimensional weight space is a harish-chandra module.",
        );
        let known =
            vec!["weight module".to_string(), "weight space".to_string(), "harish-chandra module".to_string()];
        assert!(extract_hearst_hits(&p, &known).is_empty(), "{:?}", extract_hearst_hits(&p, &known));
    }

    #[test]
    fn subject_after_bare_to_preposition_is_not_misattributed() {
        // 実データで見つかった実際の誤検出。"the transition to a
        // superconducting state is a second-order phase transition"の
        // 真の主語は"the transition"であって"superconducting state"では
        // ない。"to"がCLAUSE_INTERNAL_PREPOSITIONSに無かったため最初の
        // 修正では見逃していた回帰テスト。
        let p = paper(
            "11",
            "A mathematical proof",
            "The transition to a superconducting state is a second-order phase transition.",
        );
        let known = vec!["superconducting state".to_string(), "second-order phase transition".to_string()];
        assert!(extract_hearst_hits(&p, &known).is_empty());
    }

    #[test]
    fn an_unrelated_comma_further_back_in_the_window_does_not_hide_the_true_preposition() {
        // 実データで見つかった実際の誤検出（2度目）。固定長60文字の窓に、
        // 直前の節とは無関係な手前の節のカンマ（"...potential, a
        // mathematical proof..."）が入り込み、`window_text.contains(',')`
        // で即座に「前置詞句内部ではない」と誤判定していた。直近のカンマ
        // より後ろだけを見るように修正した後の回帰テスト。
        let p = paper(
            "14",
            "A mathematical proof",
            "On the basis of this study we then give, examining the thermodynamical potential, \
             a mathematical proof that the transition to a superconducting state is a \
             second-order phase transition.",
        );
        let known = vec!["superconducting state".to_string(), "second-order phase transition".to_string()];
        assert!(extract_hearst_hits(&p, &known).is_empty(), "{:?}", extract_hearst_hits(&p, &known));
    }

    #[test]
    fn feature_and_description_head_nouns_are_treated_as_predicate_nominal() {
        // 実データで見つかった実際の誤検出2件。いずれも「性質・役割の
        // 説明」であって分類関係ではない。
        let p1 = paper(
            "12",
            "Note",
            "Time delay is a common feature of quantum scattering theory.",
        );
        let known1 = vec!["time delay".to_string(), "common feature".to_string()];
        assert!(extract_hearst_hits(&p1, &known1).is_empty());

        let p2 = paper("13", "Note", "The diamond cone is a combinatorial description for a basis.");
        let known2 = vec!["diamond cone".to_string(), "combinatorial description".to_string()];
        assert!(extract_hearst_hits(&p2, &known2).is_empty());
    }

    #[test]
    fn subject_after_comma_is_not_treated_as_prepositional_phrase_internal() {
        // カンマが窓の中にあれば、直前の前置詞は別の節に属すると判断し
        // 誤帰属の疑いを晴らす——正しい"is a"ヒットを不必要に潰さない
        // ための回帰テスト。
        let p = paper(
            "10",
            "Note",
            "In the context of number theory, an elliptic curve is a special case of an abelian variety.",
        );
        let known = vec!["elliptic curve".to_string(), "abelian variety".to_string()];
        let hits = extract_hearst_hits(&p, &known);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].subject, "elliptic curve");
        assert_eq!(hits[0].object, "abelian variety");
    }

    #[test]
    fn merge_marks_confirmed_when_both_paths_agree() {
        let distributional =
            vec![("elliptic curves".to_string(), "abelian varieties".to_string(), RelationKind::SpecializationOf, 0.7)];
        let hearst = vec![HearstHit {
            subject: "elliptic curves".to_string(),
            object: "abelian varieties".to_string(),
            kind: RelationKind::SpecializationOf,
            sentence: "An elliptic curve is a special case of an abelian variety.".to_string(),
            arxiv_id: "math/0001".to_string(),
        }];
        let merged = merge(distributional, hearst);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].status, RelationStatus::Confirmed);
        assert!(merged[0].evidence_sentence.is_some());
    }

    #[test]
    fn merge_keeps_distributional_only_edges_as_proposed() {
        let distributional =
            vec![("kähler manifold".to_string(), "symplectic manifold".to_string(), RelationKind::SpecializationOf, 0.6)];
        let merged = merge(distributional, vec![]);
        assert_eq!(merged[0].status, RelationStatus::Proposed);
        assert!(merged[0].evidence_sentence.is_none());
    }

    #[test]
    fn merge_keeps_hearst_only_edges_as_grounded() {
        let hearst = vec![HearstHit {
            subject: "quantum groups".to_string(),
            object: "hopf algebras".to_string(),
            kind: RelationKind::SpecializationOf,
            sentence: "Quantum groups are a special case of Hopf algebras.".to_string(),
            arxiv_id: "math/0002".to_string(),
        }];
        let merged = merge(vec![], hearst);
        assert_eq!(merged[0].status, RelationStatus::Grounded);
    }

    #[test]
    fn merge_drops_both_edges_of_a_contradictory_specialization_cycle() {
        // 実データで見つかった実例。同じ論文の別々の文が矛盾する向きの
        // "is a" を主張した場合、AもBもBもAもの特殊化として残ってしまう。
        // 半順序として矛盾するので、両方落とすべき。
        let hearst = vec![
            HearstHit {
                subject: "einstein nilradical".to_string(),
                object: "nilpotent lie algebras".to_string(),
                kind: RelationKind::SpecializationOf,
                sentence: "An Einstein nilradical is a nilpotent Lie algebra.".to_string(),
                arxiv_id: "0802.2137".to_string(),
            },
            HearstHit {
                subject: "nilpotent lie algebras".to_string(),
                object: "einstein nilradical".to_string(),
                kind: RelationKind::SpecializationOf,
                sentence: "a nilpotent Lie algebra is an Einstein nilradical.".to_string(),
                arxiv_id: "0802.2137".to_string(),
            },
        ];
        let merged = merge(vec![], hearst);
        assert!(merged.is_empty(), "矛盾する循環は両方落とすべき: {merged:?}");
    }

    #[test]
    fn merge_keeps_non_contradictory_edges_when_a_cycle_exists_elsewhere() {
        // 循環の除去が無関係な辺まで巻き込んでいないことを確認する。
        let hearst = vec![
            HearstHit {
                subject: "a".to_string(),
                object: "b".to_string(),
                kind: RelationKind::SpecializationOf,
                sentence: "A is a B.".to_string(),
                arxiv_id: "1".to_string(),
            },
            HearstHit {
                subject: "b".to_string(),
                object: "a".to_string(),
                kind: RelationKind::SpecializationOf,
                sentence: "B is an A.".to_string(),
                arxiv_id: "1".to_string(),
            },
            HearstHit {
                subject: "c".to_string(),
                object: "d".to_string(),
                kind: RelationKind::SpecializationOf,
                sentence: "C is a D.".to_string(),
                arxiv_id: "2".to_string(),
            },
        ];
        let merged = merge(vec![], hearst);
        assert_eq!(merged.len(), 1);
        assert_eq!((merged[0].subject.as_str(), merged[0].object.as_str()), ("c", "d"));
    }
}

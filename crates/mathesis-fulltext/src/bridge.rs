//! `paper_theorems`/`theorem_dependencies`（`mathesis-fulltext`が抽出した
//! LaTeX定理環境）を`mathesis-graph`の判断ノードへ橋渡しする（診断⑥
//! 「当初目的の未達」への対応）。
//!
//! # 設計判断: 何を素直に受け入れ、何を見送るか
//!
//! - **`statement`は`mathesis_ast::Expr::Unparsed`に包む。** `Judgment`の
//!   `statement`フィールドは元々「層1が構造化した式」を指すが、LaTeXの
//!   定理文は形式言語ではなく自然文であり、構造化しようがない。
//!   `Expr::Unparsed`は層1が既に持っていた「これ以上構造化できない
//!   残余テキスト」というシステム境界のフォールバックで、無理に新しい
//!   仕組みを作らず既存の開いた口を使う。
//! - **`parse_status: Informal`。** Full/Partial/Failedはいずれも
//!   「形式言語をどこまで構造化できたか」という軸で、LaTeXの定理文は
//!   そもそもその軸に乗らない——「パースに失敗した」のではなく
//!   「最初から自然文として扱う」という別の状態であることを示すため、
//!   専用の値を`mathesis-graph`側に追加した。
//! - **種別（kind）は既知のものだけ受け入れ、それ以外は見送る。**
//!   `\newtheorem`の表示名は著者が自由に決められる開いた語彙
//!   （"Main Theorem"・"Fact"・独自の日本語表記等もありうる）だが、
//!   `JudgmentKind`は閉じた列挙で、`GraphStore::get_judgment`は未知の
//!   文字列に対して`.expect()`でパニックする。安全側に倒し、
//!   `JudgmentKind::from_str`が受理する語（Theorem/Lemma/Definition/
//!   Axiom/Conjecture/Example/Instance/Corollary/Proposition/Claim/
//!   Remark）と、`kind_synonym`が把握しているフランス語表示名・babel
//!   翻訳マクロ名だけを取り込む——`relations.rs`が未知の接続表現を無視して
//!   取りこぼす方を選んだのと同じ判断。見送った件数は呼び出し側
//!   （CLI）が報告する。
//! - **判断ノード間の依存は同一論文内のみ。`\cite`は判断ではなく論文単位の
//!   辺にする。** `to_label`が同じ論文の別の定理を指す場合だけ
//!   `record_judgment_dependency`を張る——`\ref`はラベル経由で「同じ論文
//!   内の特定の1定理」を指すが、`\cite`が指すのは「引用文献という1本の
//!   論文全体」であって、その論文の**どの定理**を参照しているかという
//!   情報を`\cite`自体は持たない。型が違う以上、`judgment_dependencies`
//!   （JudgmentId同士の辺）に無理に押し込めば「論文Aのこの定理は論文B
//!   全体に依存する」ことを表す代表ノードをでっち上げることになる。
//!   代わりに`mathesis_graph::paper_citation`（`paper_citations`
//!   テーブル、論文ID同士の辺）を新設し、`bridge_all_paper_citations`が
//!   別のパスとしてここへ書き込む。
//!
//!   誤結合のリスク（著者名・タイトルの文字列一致に頼らざるを得ない、
//!   `theorem.rs`冒頭のコメント参照）は解消していない——ただし解決先を
//!   `crate::citation::extract_arxiv_id`に限定することで**そもそも
//!   その一致を行わない**形にした: `\bibitem`本文に著者自身が明記した
//!   "arXiv:1234.56789"のような具体的なIDだけを読み、それ以外
//!   （著者名・誌名・年だけの伝統的な書誌情報）は判定材料が無いので
//!   見送る。さらに、解決できたarXiv IDが**このグラフに既に橋渡し済み
//!   （＝判断を1件以上持つ）論文**でなければ辺を張らない——存在するか
//!   分からない論文への辺を作り話さない。
//! - **`source_line`は`mathesis-fulltext`側で計算済みの実際の行番号**
//!   （`theorem.rs::byte_offset_to_line`）——「文書内の出現順」を行番号の
//!   代わりに流用するような、実データに無い数字を作り話しない。

use crate::store::{BridgeTheorem, FulltextStore};
use anyhow::Result;
use mathesis_ast::Expr;
use mathesis_graph::{GraphStore, Hypothesis, JudgmentKind, NewJudgment, ParseStatus, SourceRef};
use std::collections::{HashMap, HashSet};

/// 1論文分の橋渡し結果。
#[derive(Debug, Clone, Default)]
pub struct BridgeStats {
    pub judgments_inserted: usize,
    /// (種別文字列, 件数) — `JudgmentKind`に対応が無く見送った定理。
    pub kinds_skipped: HashMap<String, usize>,
    pub dependencies_created: usize,
    pub dependencies_unresolved: usize,
}

impl BridgeStats {
    fn merge(&mut self, other: BridgeStats) {
        self.judgments_inserted += other.judgments_inserted;
        for (k, v) in other.kinds_skipped {
            *self.kinds_skipped.entry(k).or_insert(0) += v;
        }
        self.dependencies_created += other.dependencies_created;
        self.dependencies_unresolved += other.dependencies_unresolved;
    }
}

/// `JudgmentKind::from_str`が受理する英語の種別名の同義語——実データで
/// 見送りとして観測された(アーキテクチャ.txt診断⑥の実測、151/2,679件の
/// 内訳)フランス語の表示名と、babel言語パッケージの「翻訳マクロ」名を
/// 英語の種別へ写す。翻訳ではなく**マクロ名自体が種別を表す**ことに注意:
/// `\newtheorem{thm}{\theoremname}`はLaTeXエンジンが実際に走れば現在の
/// 言語（`\usepackage[french]{babel}`等）に応じた訳語へ展開されるが、この
/// 抽出器はLaTeXエンジンを走らせないため展開後の訳語は分からない——しかし
/// `\theoremname`というマクロ名自体がbabelのtranslatorモジュールの慣例に
/// 従って「これはTheorem種別だ」という情報を、展開せずとも運んでいる。
/// これは既知の種別への**言い換えの認識**であって、未知の概念を近い種別へ
/// 押し込める推測ではない——`clean_display_name`のフォント切替コマンド
/// 除去や`FALLBACK_THEOREM_ENVS`と同じ「観測した具体的なパターンだけを
/// 閉じた対応表で受け止める」方針を踏襲する。フランス語の
/// Proposition/Conjectureは英語と綴りが同じなので`from_str`がそのまま
/// 受理し、ここに個別のエントリは要らない。
fn kind_synonym(lower: &str) -> Option<JudgmentKind> {
    Some(match lower {
        "théorème" | "theoreme" => JudgmentKind::Theorem,
        "lemme" => JudgmentKind::Lemma,
        "corollaire" => JudgmentKind::Corollary,
        "définition" => JudgmentKind::Definition,
        "remarque" => JudgmentKind::Remark,
        "exemple" => JudgmentKind::Example,
        "axiome" => JudgmentKind::Axiom,
        "\\theoremname" | "\\thmname" | "\\theoname" => JudgmentKind::Theorem,
        "\\lemmaname" => JudgmentKind::Lemma,
        "\\propname" | "\\propositionname" => JudgmentKind::Proposition,
        "\\coroname" | "\\corollaryname" => JudgmentKind::Corollary,
        "\\defname" | "\\definame" | "\\definitionname" => JudgmentKind::Definition,
        "\\remaname" | "\\remarkname" => JudgmentKind::Remark,
        "\\exampname" | "\\examplename" => JudgmentKind::Example,
        "\\conjname" | "\\conjecturename" => JudgmentKind::Conjecture,
        "\\claimname" => JudgmentKind::Claim,
        // 複数形。著者が複数の例/注釈をまとめて1つの環境で宣言する場合
        // （実データ`1112.2959`系で"Examples"・"Remarks"を確認済み）。
        "examples" => JudgmentKind::Example,
        "remarks" => JudgmentKind::Remark,
        // 非UTF-8のTeXソースを`String::from_utf8_lossy`で読んだ結果、
        // アクセント付き文字がU+FFFD（置換文字）に化けた具体的な観測形
        // （実データ`0901.3200`: "Théorème"の`é`・`è`が2箇所とも化けている）。
        // 一般的な文字化け修復ロジックは作らない——この特定の壊れ方だけを
        // 既知の同義語として受け止める、他のケースへは汎化しない。
        "th\u{fffd}or\u{fffd}me" => JudgmentKind::Theorem,
        // 前置1970〜80年代のLaTeXアクセント記法（`\'e`=é、`` \`e ``=è）を
        // 使った表示名（実データ`math/0011226`: "Th\'eor\`eme"・
        // "D\'efinition"）。`theorem.rs`の`NON_PROVABLE_KINDS`コメントが
        // 既に明記している通り、この抽出器は非エスケープ化（`\'e`→`é`の
        // ような一般変換）を行わない方針——ここでも同じ方針を踏襲し、
        // 一般化はせず実際に観測した具体的な綴りだけをそのまま受け止める。
        "th\\'eor\\`eme" => JudgmentKind::Theorem,
        "d\\'efinition" => JudgmentKind::Definition,
        _ => return None,
    })
}

fn map_kind(latex_kind: &str) -> Option<JudgmentKind> {
    let lower = latex_kind.to_lowercase();
    kind_synonym(&lower).or_else(|| JudgmentKind::from_str(&lower))
}

/// 1論文ぶんの定理・依存関係を`graph`へ取り込む。同じ`arxiv_id`を複数回
/// 呼んでも安全——`graph`に既にその`arxiv_id`のpaperが登録済みなら、
/// 定理を読みにさえ行かず即座に何もせず返す。以前は「`bridge_all_new_papers`
/// が事前に`find_paper_by_arxiv_id`で除外するから大丈夫」と呼び出し側の
/// 規律に頼っており、`bridge_paper`単体を直接2回呼ぶとJudgmentが重複する
/// 既知の欠陥だった（`アーキテクチャ.txt`診断⑥「残っている不足」参照）。
/// この関数自身が冪等性を持つように直したことで、呼び出し側の運用規律に
/// 依存しなくなった——`bridge_all_new_papers`側の事前チェックは、進捗
/// コールバックを「新規に橋渡しした論文だけ」に絞る報告目的でそのまま残す
/// （二重にチェックしても`find_paper_by_arxiv_id`はインデックス参照なので
/// 実測上のコストは無視できる）。
pub fn bridge_paper(fulltext: &FulltextStore, graph: &GraphStore, arxiv_id: &str) -> Result<BridgeStats> {
    if graph.find_paper_by_arxiv_id(arxiv_id)?.is_some() {
        return Ok(BridgeStats::default());
    }
    let theorems = fulltext.theorems_for_paper(arxiv_id)?;
    let dependencies = fulltext.dependencies_for_paper(arxiv_id)?;
    if theorems.is_empty() {
        return Ok(BridgeStats::default());
    }

    let title = fulltext.paper_title(arxiv_id)?;
    let paper_id = graph.intern_paper(arxiv_id, title.as_deref())?;

    let mut stats = BridgeStats::default();
    let mut order_to_judgment = HashMap::new();
    let mut label_to_judgment = HashMap::new();

    for thm in &theorems {
        let Some(kind) = map_kind(&thm.kind) else {
            *stats.kinds_skipped.entry(thm.kind.clone()).or_insert(0) += 1;
            continue;
        };
        let judgment_id = insert_theorem_judgment(graph, arxiv_id, thm, kind, paper_id)?;
        stats.judgments_inserted += 1;
        order_to_judgment.insert(thm.order, judgment_id);
        if let Some(label) = &thm.label {
            label_to_judgment.insert(label.clone(), judgment_id);
        }
    }

    for dep in &dependencies {
        let resolved = order_to_judgment
            .get(&dep.from_order)
            .zip(label_to_judgment.get(&dep.to_label));
        match resolved {
            Some((&from, &to)) => {
                graph.record_judgment_dependency(from, to)?;
                stats.dependencies_created += 1;
            }
            None => stats.dependencies_unresolved += 1,
        }
    }

    Ok(stats)
}

fn insert_theorem_judgment(
    graph: &GraphStore,
    arxiv_id: &str,
    thm: &BridgeTheorem,
    kind: JudgmentKind,
    paper_id: mathesis_graph::PaperId,
) -> Result<mathesis_graph::JudgmentId> {
    let statement = graph.intern_expr(&Expr::Unparsed(thm.statement_text.clone()))?;
    let judgment = NewJudgment {
        kind,
        name: thm.label.clone(),
        context: Vec::<Hypothesis>::new(),
        statement,
        definition_body_raw: None,
        source: SourceRef { file: format!("arxiv:{arxiv_id}"), line: thm.source_line },
        raw_text: thm.statement_text.clone(),
        parse_status: ParseStatus::Informal,
        source_paper: Some(paper_id),
    };
    Ok(graph.insert_judgment(&judgment)?)
}

/// `mathesis-fulltext`が定理を持つと記録している全論文を、まだ橋渡しして
/// いないものだけ`graph`へ取り込む。「まだ橋渡ししていない」は
/// `graph.find_paper_by_arxiv_id`が見つからないことで判定する——`bridge_paper`
/// 自身も同じチェックを内部で行うので二重にはなるが、ここでの事前フィルタは
/// 主に`on_progress`コールバックを「実際に新規追加した論文だけ」に絞る
/// 報告目的（インデックス参照なので、二重チェックの実測コストは無視できる）。
pub fn bridge_all_new_papers(
    fulltext: &FulltextStore,
    graph: &GraphStore,
    mut on_progress: impl FnMut(&str, &BridgeStats),
) -> Result<BridgeStats> {
    let mut total = BridgeStats::default();
    for arxiv_id in fulltext.list_theorem_arxiv_ids()? {
        if graph.find_paper_by_arxiv_id(&arxiv_id)?.is_some() {
            continue; // 既に橋渡し済み（前回の実行分）。
        }
        let stats = bridge_paper(fulltext, graph, &arxiv_id)?;
        on_progress(&arxiv_id, &stats);
        total.merge(stats);
    }
    Ok(total)
}

/// 論文単位の引用辺（`\cite`、`mathesis_graph::paper_citation`）を張る。
#[derive(Debug, Clone, Default)]
pub struct CitationStats {
    pub citations_created: usize,
    /// arXiv IDへの解決はできたが、引用先がこのグラフにまだ橋渡しされて
    /// いない（＝判断を1件も持たない、存在するかどうか分からない）ため
    /// 見送った件数。
    pub citations_unresolved: usize,
}

/// `bridge_all_new_papers`とは独立した、別のパスとして実行する。ある論文
/// Aが論文Bを引用していても、`list_theorem_arxiv_ids`の走査でAがBより
/// 先に来れば、Aの処理時点ではBがまだ橋渡しされていない——全論文の
/// 橋渡しが出揃った**後**にもう一度全体を舐めることで、走査順序に依存
/// せず引用辺を取りこぼさない。
///
/// 何度呼んでも安全（`record_paper_citation`はINSERT OR IGNORE）。
/// 「新規に追加した論文だけ」のような絞り込みは行わず、定理を持つ全論文を
/// 毎回舐め直す——`find_paper_by_arxiv_id`はインデックス参照なので、
/// 絞り込みを実装する複雑さに見合わない。
pub fn bridge_all_paper_citations(fulltext: &FulltextStore, graph: &GraphStore) -> Result<CitationStats> {
    let mut stats = CitationStats::default();
    for arxiv_id in fulltext.list_theorem_arxiv_ids()? {
        let Some(from_paper) = graph.find_paper_by_arxiv_id(&arxiv_id)? else {
            continue; // この論文自体がまだ橋渡しされていない（判断0件）。
        };
        let resolved = fulltext.bibitem_citations_for_paper(&arxiv_id)?;
        if resolved.is_empty() {
            continue;
        }
        let theorems = fulltext.theorems_for_paper(&arxiv_id)?;
        // 同じ引用先を複数の定理が引用するのは普通にあるので、論文単位で
        // 重複排除してから解決する（DB往復を減らす——正しさは
        // `record_paper_citation`のINSERT OR IGNOREが既に保証している）。
        let mut targets: HashSet<&str> = HashSet::new();
        for thm in &theorems {
            for cite_key in &thm.cites {
                if let Some(target_arxiv_id) = resolved.get(cite_key) {
                    targets.insert(target_arxiv_id.as_str());
                }
            }
        }
        for target_arxiv_id in targets {
            match graph.find_paper_by_arxiv_id(target_arxiv_id)? {
                Some(to_paper) if to_paper.id != from_paper.id => {
                    graph.record_paper_citation(from_paper.id, to_paper.id)?;
                    stats.citations_created += 1;
                }
                // 自己引用は実データでは起きないはずだが、起きても実害の
                // 無い自明な閉路なので静かに無視する。
                Some(_) => {}
                None => stats.citations_unresolved += 1,
            }
        }
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theorem::{ProofMatch, TheoremRecord};

    fn record(order: usize, kind: &str, label: Option<&str>, depends_on: &[&str], line: u32) -> TheoremRecord {
        TheoremRecord {
            kind: kind.to_string(),
            label: label.map(str::to_string),
            order,
            has_proof: false,
            proof_match: ProofMatch::None,
            depends_on_labels: depends_on.iter().map(|s| s.to_string()).collect(),
            cites: vec![],
            statement_text: format!("Statement text for {kind} #{order}."),
            line,
        }
    }

    fn setup() -> (FulltextStore, GraphStore) {
        let fulltext = FulltextStore::open_in_memory().unwrap();
        let graph = GraphStore::open_in_memory().unwrap();
        (fulltext, graph)
    }

    fn record_citing(order: usize, kind: &str, label: Option<&str>, cites: &[&str], line: u32) -> TheoremRecord {
        TheoremRecord {
            kind: kind.to_string(),
            label: label.map(str::to_string),
            order,
            has_proof: false,
            proof_match: ProofMatch::None,
            depends_on_labels: vec![],
            cites: cites.iter().map(|s| s.to_string()).collect(),
            statement_text: format!("Statement text for {kind} #{order}."),
            line,
        }
    }

    #[test]
    fn bridge_paper_inserts_a_judgment_per_mappable_theorem() {
        let (mut fulltext, graph) = setup();
        fulltext
            .save_theorems(
                "math/0001",
                &[record(0, "Theorem", Some("main"), &[], 10), record(1, "Corollary", Some("cor"), &["main"], 20)],
            )
            .unwrap();

        let stats = bridge_paper(&fulltext, &graph, "math/0001").unwrap();

        assert_eq!(stats.judgments_inserted, 2);
        assert!(stats.kinds_skipped.is_empty());
        assert_eq!(stats.dependencies_created, 1, "corからmainへの\\refが依存辺になるべき");
        assert_eq!(graph.find_paper_by_arxiv_id("math/0001").unwrap().unwrap().arxiv_id, "math/0001");
    }

    #[test]
    fn bridge_paper_skips_theorems_with_an_unrecognized_kind_but_keeps_the_rest() {
        let (mut fulltext, graph) = setup();
        fulltext
            .save_theorems("math/0002", &[record(0, "Fact", Some("f1"), &[], 5), record(1, "Lemma", Some("l1"), &[], 8)])
            .unwrap();

        let stats = bridge_paper(&fulltext, &graph, "math/0002").unwrap();

        assert_eq!(stats.judgments_inserted, 1, "LemmaだけがJudgmentKindに対応する");
        assert_eq!(stats.kinds_skipped.get("Fact"), Some(&1));
    }

    #[test]
    fn bridge_paper_counts_a_dependency_as_unresolved_when_its_target_kind_was_skipped() {
        let (mut fulltext, graph) = setup();
        fulltext
            .save_theorems(
                "math/0003",
                &[record(0, "Fact", Some("f1"), &[], 1), record(1, "Lemma", Some("l1"), &["f1"], 2)],
            )
            .unwrap();

        let stats = bridge_paper(&fulltext, &graph, "math/0003").unwrap();

        assert_eq!(stats.judgments_inserted, 1);
        assert_eq!(stats.dependencies_unresolved, 1, "参照先(f1)のFactは取り込まれていない");
        assert_eq!(stats.dependencies_created, 0);
    }

    #[test]
    fn bridge_paper_on_a_paper_with_no_theorems_is_a_harmless_no_op() {
        let (fulltext, graph) = setup();
        let stats = bridge_paper(&fulltext, &graph, "math/9999").unwrap();
        assert_eq!(stats.judgments_inserted, 0);
        assert!(graph.find_paper_by_arxiv_id("math/9999").unwrap().is_none(), "定理が無い論文はpapersにも登録しない");
    }

    #[test]
    fn bridge_all_new_papers_skips_a_paper_already_bridged_in_a_previous_run() {
        let (mut fulltext, graph) = setup();
        fulltext.save_theorems("math/0004", &[record(0, "Theorem", Some("t"), &[], 1)]).unwrap();
        bridge_paper(&fulltext, &graph, "math/0004").unwrap();

        let mut seen = Vec::new();
        let total = bridge_all_new_papers(&fulltext, &graph, |id, _| seen.push(id.to_string())).unwrap();

        assert!(seen.is_empty(), "既に橋渡し済みの論文を再処理してはいけない");
        assert_eq!(total.judgments_inserted, 0);
    }

    #[test]
    fn bridge_paper_called_twice_directly_does_not_duplicate_judgments() {
        // 過去の欠陥（アーキテクチャ.txt診断⑥「残っている不足」）の再現:
        // `bridge_all_new_papers`を経由せず`bridge_paper`を直接2回呼ぶ
        // 運用でも、Judgmentが重複してはいけない。
        let (mut fulltext, graph) = setup();
        fulltext.save_theorems("math/0005", &[record(0, "Theorem", Some("t"), &[], 1)]).unwrap();

        let first = bridge_paper(&fulltext, &graph, "math/0005").unwrap();
        let second = bridge_paper(&fulltext, &graph, "math/0005").unwrap();

        assert_eq!(first.judgments_inserted, 1);
        assert_eq!(second.judgments_inserted, 0, "2回目は既に橋渡し済みとして何もしないべき");
    }

    #[test]
    fn bridge_all_paper_citations_links_papers_across_the_bridge_order() {
        // "A→Bを引用"のBが、Aより後に橋渡しされる場合でも取りこぼさない
        // ことを確認する——`bridge_all_new_papers`の1パスだけでは
        // `list_theorem_arxiv_ids`の走査順序次第でBがまだ無い。
        let (mut fulltext, graph) = setup();
        fulltext
            .save_theorems("math/citing", &[record_citing(0, "Theorem", Some("t"), &["smith99"], 1)])
            .unwrap();
        fulltext.save_bibitem_citations("math/citing", &[("smith99".to_string(), "math/cited".to_string())]).unwrap();
        fulltext.save_theorems("math/cited", &[record(0, "Theorem", Some("u"), &[], 1)]).unwrap();

        bridge_all_new_papers(&fulltext, &graph, |_, _| {}).unwrap();
        let stats = bridge_all_paper_citations(&fulltext, &graph).unwrap();

        assert_eq!(stats.citations_created, 1);
        assert_eq!(stats.citations_unresolved, 0);
        let citing = graph.find_paper_by_arxiv_id("math/citing").unwrap().unwrap();
        let cited = graph.find_paper_by_arxiv_id("math/cited").unwrap().unwrap();
        assert_eq!(graph.citations_of(citing.id).unwrap(), vec![cited.id]);
    }

    #[test]
    fn bridge_all_paper_citations_counts_a_resolved_but_unbridged_target_as_unresolved() {
        let (mut fulltext, graph) = setup();
        fulltext
            .save_theorems("math/citing", &[record_citing(0, "Theorem", Some("t"), &["smith99"], 1)])
            .unwrap();
        // "math/nonexistent"はarXiv IDとしては解決できたが、このグラフには
        // 一度も橋渡しされていない（＝存在するかどうか分からない）。
        fulltext
            .save_bibitem_citations("math/citing", &[("smith99".to_string(), "math/nonexistent".to_string())])
            .unwrap();

        bridge_all_new_papers(&fulltext, &graph, |_, _| {}).unwrap();
        let stats = bridge_all_paper_citations(&fulltext, &graph).unwrap();

        assert_eq!(stats.citations_created, 0);
        assert_eq!(stats.citations_unresolved, 1);
    }

    #[test]
    fn bridge_all_paper_citations_deduplicates_multiple_theorems_citing_the_same_paper() {
        let (mut fulltext, graph) = setup();
        fulltext
            .save_theorems(
                "math/citing",
                &[
                    record_citing(0, "Theorem", Some("t1"), &["smith99"], 1),
                    record_citing(1, "Lemma", Some("t2"), &["smith99"], 2),
                ],
            )
            .unwrap();
        fulltext.save_bibitem_citations("math/citing", &[("smith99".to_string(), "math/cited".to_string())]).unwrap();
        fulltext.save_theorems("math/cited", &[record(0, "Theorem", Some("u"), &[], 1)]).unwrap();

        bridge_all_new_papers(&fulltext, &graph, |_, _| {}).unwrap();
        let stats = bridge_all_paper_citations(&fulltext, &graph).unwrap();

        assert_eq!(stats.citations_created, 1, "同じ引用先への辺は1本だけ（record_paper_citationの冪等性とは別に、ここでも重複カウントしない）");
    }

    #[test]
    fn bridge_all_paper_citations_is_idempotent_when_run_twice() {
        let (mut fulltext, graph) = setup();
        fulltext
            .save_theorems("math/citing", &[record_citing(0, "Theorem", Some("t"), &["smith99"], 1)])
            .unwrap();
        fulltext.save_bibitem_citations("math/citing", &[("smith99".to_string(), "math/cited".to_string())]).unwrap();
        fulltext.save_theorems("math/cited", &[record(0, "Theorem", Some("u"), &[], 1)]).unwrap();
        bridge_all_new_papers(&fulltext, &graph, |_, _| {}).unwrap();

        bridge_all_paper_citations(&fulltext, &graph).unwrap();
        bridge_all_paper_citations(&fulltext, &graph).unwrap();

        assert_eq!(graph.paper_citation_count().unwrap(), 1, "2回実行しても辺が重複してはいけない");
    }

    #[test]
    fn map_kind_recognizes_french_newtheorem_display_names() {
        assert_eq!(map_kind("Théorème"), Some(JudgmentKind::Theorem));
        assert_eq!(map_kind("Lemme"), Some(JudgmentKind::Lemma));
        assert_eq!(map_kind("Corollaire"), Some(JudgmentKind::Corollary));
        assert_eq!(map_kind("Définition"), Some(JudgmentKind::Definition));
        assert_eq!(map_kind("Remarque"), Some(JudgmentKind::Remark));
        // Proposition/Conjectureは英語と綴りが同じなので、専用の同義語が
        // 無くても既存の`from_str`だけで通る。
        assert_eq!(map_kind("Proposition"), Some(JudgmentKind::Proposition));
    }

    #[test]
    fn map_kind_recognizes_babel_translator_macro_names_without_expanding_them() {
        // `\theoremname`はLaTeXエンジンが走れば言語ごとの訳語に展開される
        // マクロだが、展開せずマクロ名自体から種別を読み取る。
        assert_eq!(map_kind("\\theoremname"), Some(JudgmentKind::Theorem));
        assert_eq!(map_kind("\\coroname"), Some(JudgmentKind::Corollary));
        assert_eq!(map_kind("\\propname"), Some(JudgmentKind::Proposition));
        assert_eq!(map_kind("\\remaname"), Some(JudgmentKind::Remark));
    }

    #[test]
    fn map_kind_still_rejects_genuinely_unknown_kinds() {
        assert_eq!(map_kind("Question"), None);
        assert_eq!(map_kind("Fact"), None);
    }

    #[test]
    fn map_kind_recognizes_plural_forms() {
        assert_eq!(map_kind("Examples"), Some(JudgmentKind::Example));
        assert_eq!(map_kind("Remarks"), Some(JudgmentKind::Remark));
    }

    #[test]
    fn map_kind_recognizes_a_mojibake_theoreme_from_a_non_utf8_source() {
        // 実データ(0901.3200): 非UTF-8ソースを`from_utf8_lossy`で読んだ結果、
        // "Théorème"のアクセント2箇所がU+FFFDに化けた具体的な観測形。
        assert_eq!(map_kind("Th\u{fffd}or\u{fffd}me"), Some(JudgmentKind::Theorem));
    }

    #[test]
    fn map_kind_recognizes_old_style_escaped_accent_display_names() {
        // 実データ(math/0011226): `\'e`/`` \`e ``という前置LaTeXアクセント
        // 記法をそのまま使った表示名。
        assert_eq!(map_kind("Th\\'eor\\`eme"), Some(JudgmentKind::Theorem));
        assert_eq!(map_kind("D\\'efinition"), Some(JudgmentKind::Definition));
    }
}

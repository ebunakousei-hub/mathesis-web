//! 論文コーパス全体にわたる候補フレーズの集計と、MSC2020への grounding。
//! アーキテクチャ.txt 5.4「候補語抽出 → Entity Resolution」の前半部分。
//! 表記ゆれの正規化（Kähler manifold / Kähler manifolds を同一視する等の
//! 本格的なEntity Resolution）はここでは行わない——単数/複数の簡易な
//! ゆらぎ吸収のみをMSC grounding判定に使い、それ以外はPhase 3以降の課題として
//! 明示的に残す。

use crate::concentration::{self, FieldPrior};
use crate::rake;
use mathesis_ingest::model::Paper;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

/// 分野集中度（`concentration.rs`）で**原理的に**捕まえられない定型句。
///
/// 集中度が測っているのは「分野への固有さ」であって「概念らしさ」では
/// ない。そのため、**書き方の作法そのものが分野と相関している**定型句は、
/// 中身が空でも高い集中度を得てしまう。実データ（100,000論文）で確認した
/// 具体例:
///
///   - "mild assumptions" 0.688 / "suitable assumptions" 0.391 —— 「緩い
///     仮定の下で」という言い回しは統計・PDE系の論文の作法で、その作法が
///     分野に偏っているだけ。仮定の中身は何も指していない。
///   - "rigorous proof" 0.609 —— 「厳密な証明を与える」は数理物理
///     (math-ph) の論文の作法。"rigorous" 単体でも0.568と偏っている。
///   - "main theorem" 0.235 / "short proof" 0.304 —— 閾値のすぐ上に
///     残ってしまった、ほぼ分野一様な定型句。
///
/// この残りをさらに統計で削ろうとして、構成語それぞれの集中度が低い
/// フレーズを落とす規則も実データで試したが、"fixed points"（"fixed"
/// 0.085 / "points" 0.205 でどちらも一様なのに実在の概念）を巻き込んで
/// しまい成立しなかった。「書き方の作法か、対象を指す名前か」は分野分布
/// からは見えない——これはアーキテクチャ.txt 5.4 で Phase 8以降に置いた
/// LLM judge の仕事であり、その時は全80,735候補ではなく統計で判別が
/// つかなかったこの少数にだけ掛ければよい。
///
/// 旧実装ではこの手のリストが除外の**主**機構で、コーパスを増やすたびに
/// 人手で伸ばす必要があった（17件）。現在は集中度が731件を自動で落とし、
/// ここはその既知の取りこぼしだけを埋める補助に後退している。
///
/// 2026-09-03、10,000論文規模（100k論文規模とは別のコーパス）でCPM
/// クラスタリングをベンチマークした際、下の8件より多くの定型句クラスタが
/// 生き残っているのを実際に確認した——"simple proof / elementary proof /
/// alternative proof / …"（17件のクラスタ全体が証明の性質を形容する語
/// だけで構成）、"wide class / general class / special class / …"
/// （クラスの一般性を形容するだけ）、"revised version / expanded
/// version / …"（版の性質）、"part ii / last section / final section /
/// main part / …"（論文自身の節・部を指す語——ただし同じクラスタに
/// 混ざっていた"real part"（複素数の実部、正当な概念）はここには入れて
/// いない）等。原因は集中度フィルタの前提——分野ラベル付き論文が
/// 50件以上無いと判定しない（`MIN_LABELED_PAPERS`）——が、10,000論文
/// 規模では単語1語よりずっと出現頻度の低い複合語に対してほとんど
/// 発動しないこと。コーパスが小さいほど、このリストへの依存度が
/// 上がるということ自体が、統計だけに頼ることの限界を示している。
const REGISTER_PHRASES: &[&str] = &[
    "main theorem",
    "short proof",
    "rigorous proof",
    "mild assumptions",
    "suitable assumptions",
    "additional assumptions",
    "additional assumption",
    "general assumptions",
    // 証明の性質を形容するだけの語（中身を指していない）。この一群は
    // 直そうとした2回とも別の顔ぶれで再発した——1回目に見つけた語を
    // 落として再クラスタリングしたら、"direct proof / proof relies /
    // simplified proof / proof involves / proof follows / proof
    // consists / …" という**別の16件クラスタ**が同じ順位に浮上した。
    // "proof"という1語を核に、一般的な形容詞・動詞のどれと組み合わせて
    // も定型句になるという構造上の問題であって、個別の語を消しても
    // 語彙が入れ替わるだけ——ここに挙げた語で全て塞がる保証は無い
    // （いたちごっこであることを承知の上で残す。原理的な解決は
    // 引き続きPhase 8以降のLLM judgeの仕事）。
    "simple proof",
    "simple proofs",
    "elementary proof",
    "alternative proof",
    "easy proof",
    "shorter proof",
    "complete proof",
    "complete proofs",
    "direct proof",
    "proof relies",
    "simplified proof",
    "detailed proof",
    "proof involves",
    "proof follows",
    "proof consists",
    "simpler proof",
    // クラス・集合の一般性を形容するだけの語
    "wide class",
    "general class",
    "special class",
    "special classes",
    "broad class",
    "larger class",
    "wider class",
    "different classes",
    // 性質一般を形容するだけの語（"properties"自体は本物の概念の一部にも
    // なるが、これらの形容詞と組んだときは定型句にしかならない）
    "basic properties",
    "general properties",
    "main properties",
    "following properties",
    "nice properties",
    "special properties",
    "interesting properties",
    "remarkable properties",
    // 版・改訂の性質を形容するだけの語
    "revised version",
    "expanded version",
    "final version",
    "effective version",
    "general version",
    "simplified version",
    "refined version",
    "relative version",
    // 論文自身の節・部を指す語（"real part"は複素数の実部を指す正当な
    // 概念なので、意図的にここへ入れていない）
    "part ii",
    "part iii",
    "last section",
    "final section",
    "main part",
    "last part",
    "third part",
    // 例示を導入するだけの語
    "interesting examples",
    "illustrative examples",
    "important example",
    "important examples",
    "specific example",
    "typical example",
    // 「予想」という語を導入するだけの語（"standard conjectures"・
    // "long standing conjecture"のように固有の内容を持ちうる語形は
    // 意図的にここへ入れていない）
    "general conjecture",
    "conjecture stated",
    "conjecture made",
    "conjecture holds",
];

#[derive(Debug, Clone, PartialEq)]
pub struct ConceptCandidate {
    pub phrase: String,
    /// `phrase` の語数。1語の広い分野名（"topology"等）と複合語の専門用語
    /// （"kähler manifolds"等）を区別する表示に使う——1語の候補は抄録の
    /// 定型語との区別が付きにくいノイズが混じりやすいため。
    pub word_count: usize,
    /// このフレーズを含む論文の数（同一論文内での重複は1回に数える）
    pub doc_freq: usize,
    /// 出現した各論文でのRAKEスコアの平均
    pub mean_score: f64,
    /// MSC2020のいずれかのコード名に同一フレーズ（単数/複数ゆれのみ吸収）が
    /// 見つかった場合、その最初に一致したコード
    pub msc_code: Option<String>,
    /// 目視確認用に、このフレーズを含む論文のarXiv IDを最大3件
    pub sample_arxiv_ids: Vec<String>,
    /// 分野集中度（`concentration.rs`）。このフレーズを含む論文の分野分布が
    /// コーパス全体の分野分布からどれだけ離れているか。低いほど「どの分野でも
    /// 同じ割合で書かれる＝執筆の定型句」。分野ラベルの付いた論文が
    /// `concentration::MIN_LABELED_PAPERS` 件に満たない場合は判定に足る
    /// 証拠が無いので `None`（そのフレーズは定型句として落とさない）。
    pub field_concentration: Option<f32>,
}

impl ConceptCandidate {
    /// 分野集中度が閾値を下回る＝コーパス全体とほぼ同じ分野分布で現れる＝
    /// 数学的内容を運ばない執筆定型句、と統計的に判定できる状態。
    /// スコアが無い（証拠不足）候補は常に false。
    pub fn is_field_uniform_boilerplate(&self) -> bool {
        matches!(self.field_concentration, Some(c) if c < concentration::GENERIC_MAX_CONCENTRATION)
    }

    /// 集中度では落とせないと分かっている定型句（`REGISTER_PHRASES`）。
    fn is_known_register_phrase(&self) -> bool {
        REGISTER_PHRASES.contains(&self.phrase.as_str())
    }

    fn is_boilerplate(&self) -> bool {
        self.is_field_uniform_boilerplate() || self.is_known_register_phrase()
    }
}

pub struct ExtractionResult {
    pub candidates: Vec<ConceptCandidate>,
    /// (arxiv_id, phrase) — `candidates` に残ったフレーズのみを含む
    pub paper_links: Vec<(String, String)>,
    /// 文書頻度は満たしたが、分野集中度が低く定型句と判定して落とした候補。
    /// 何が落ちたかを `extract` のレポートで確認できるようにする（手書きの
    /// ブロックリスト時代は「何を落としているか」がソースを読まないと
    /// 分からなかった）。集中度の降順ではなく昇順＝定型句らしい順に並ぶ。
    pub dropped_as_boilerplate: Vec<ConceptCandidate>,
}

/// フレーズ1件ぶんの集計値。3つの別々のHashMap（doc_freq/score_sum/
/// samples）に分けていた旧実装では、フレーズの出現1回ごとに同じ文字列を
/// 3回clone・3回ハッシュしていた。1つのHashMapにまとめることで、
/// clone・ハッシュ計算をそれぞれ1回に減らす（後段のベンチマーク参照）。
#[derive(Default)]
struct Accum {
    doc_freq: usize,
    score_sum: f64,
    samples: Vec<String>,
    /// 分野ラベルの付いていた論文の件数（`field_counts` の母数）。
    labeled_papers: usize,
    /// 分野id → 観測質量。ほとんどのフレーズは数分野にしか現れないので、
    /// フレーズごとにHashMapを確保するより線形探索付きのVecの方が
    /// 小さく速い（分野数の上限はコーパス全体のラベル数＝実データで152）。
    field_counts: Vec<(u32, f64)>,
}

impl Accum {
    fn add_field(&mut self, id: u32, weight: f64) {
        match self.field_counts.iter_mut().find(|(existing, _)| *existing == id) {
            Some((_, mass)) => *mass += weight,
            None => self.field_counts.push((id, weight)),
        }
    }
}

/// 全論文のtitle+abstractから候補フレーズを抽出し、`min_df` 件以上の論文に
/// 出現するものだけを残す。1論文だけに出現するフレーズは、コーパス全体で
/// 見て「概念」と呼ぶには根拠が弱いノイズとして意図的に捨てる
/// （アーキテクチャ.txt 5.4の bottom-up パイプライン、大量のtermから
/// 頻度で刈り込む段に相当）。
///
/// 文書頻度の足切りを生き延びた候補のうち、分野集中度（`concentration.rs`）が
/// 低いもの——コーパス全体とほぼ同じ分野分布で現れるもの——は執筆上の
/// 定型句として落とす。以前はここに手書きのフレーズブロックリストが
/// あったが、コーパスを増やすたびに新しい定型句を人手で足す必要があり
/// 保守が破綻していた（アーキテクチャ.txt「100,000論文スケールテスト」①）。
pub fn extract_candidates(papers: &[Paper], min_df: usize) -> ExtractionResult {
    let prior = FieldPrior::from_papers(papers);
    let mut acc: HashMap<String, Accum> = HashMap::new();
    let mut raw_links: Vec<(String, String)> = Vec::new();

    for p in papers {
        let (field_ids, field_weight) = prior.label_ids(p);
        let text = format!("{} {}", p.title, p.abstract_text);
        for (phrase, score) in rake::score_document(&text) {
            raw_links.push((p.arxiv_id.clone(), phrase.clone()));
            let entry = acc.entry(phrase).or_default();
            entry.doc_freq += 1;
            entry.score_sum += score;
            if entry.samples.len() < 3 {
                entry.samples.push(p.arxiv_id.clone());
            }
            if !field_ids.is_empty() {
                entry.labeled_papers += 1;
                for &id in &field_ids {
                    entry.add_field(id, field_weight);
                }
            }
        }
    }

    // `acc` をここで消費する（`into_iter`）ことで、候補構築時のclone/copyを
    // 追加で発生させない——phraseもsamplesもそのままフィールドへ移動する。
    let scored: Vec<ConceptCandidate> = acc
        .into_iter()
        .filter(|(_, a)| a.doc_freq >= min_df)
        .map(|(phrase, a)| {
            let word_count = phrase.split(' ').count();
            let msc_code = ground_in_msc(&phrase);
            let field_concentration = prior.concentration(&a.field_counts, a.labeled_papers);
            ConceptCandidate {
                word_count,
                doc_freq: a.doc_freq,
                mean_score: a.score_sum / a.doc_freq as f64,
                msc_code,
                sample_arxiv_ids: a.samples,
                field_concentration,
                phrase,
            }
        })
        .collect();

    let (mut dropped_as_boilerplate, mut candidates): (Vec<_>, Vec<_>) =
        scored.into_iter().partition(ConceptCandidate::is_boilerplate);

    candidates.sort_by(|a, b| {
        b.doc_freq
            .cmp(&a.doc_freq)
            .then(b.mean_score.partial_cmp(&a.mean_score).unwrap())
    });
    // 定型句らしい（集中度が低い）順。`REGISTER_PHRASES` 由来の候補は
    // 集中度が高いか、証拠不足で `None`——どちらも末尾に置く。
    dropped_as_boilerplate.sort_by(|a, b| {
        let key = |c: &ConceptCandidate| c.field_concentration.unwrap_or(f32::INFINITY);
        key(a).partial_cmp(&key(b)).unwrap_or(std::cmp::Ordering::Equal)
    });

    // borrowのみ（clone不要）でフレーズの生存判定を行う。
    let surviving: HashSet<&str> = candidates.iter().map(|c| c.phrase.as_str()).collect();
    let paper_links = raw_links
        .into_iter()
        .filter(|(_, phrase)| surviving.contains(phrase.as_str()))
        .collect();

    ExtractionResult {
        candidates,
        paper_links,
        dropped_as_boilerplate,
    }
}

/// MSC2020の全コード名（`name`列）に現れる1〜4語のn-gramから、それが
/// 最初に見つかったコードへのマップを、一度だけ構築して保持する
/// （`mathesis-msc` 自体がLazyLockで同じことをしているのにならった作り）。
///
/// 最適化の記録: 当初はn-gramの集合（HashSet）だけを持ち、grounding判定が
/// 成立するたびに全6,603件のMSC名を毎回re-tokenizeしながら線形走査して
/// 所属コードを探す `find_msc_code_for_ngram` を呼んでいた。実データ
/// （5,000論文→3,196候補）でプロファイリングしたところ、抽出処理全体
/// 10.6秒のうち9.97秒（94%）がこの「grounding込みの候補構築」段に
/// 集中しており、そのほぼ全てがこの重複re-tokenizeだった。候補ごとに
/// 毎回線形走査するのではなく、フレーズ→コードのマップをLazyLockで
/// 一度だけ構築してO(1)引きにしたことで、同じ実データに対して
/// 10.6秒 → 0.6秒（約18倍）に短縮した（後段のベンチマーク参照）。
static MSC_NGRAM_INDEX: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    let mut index: HashMap<String, String> = HashMap::new();
    for c in mathesis_msc::all() {
        let words: Vec<String> = rake_words_only(&c.name);
        for n in 1..=4.min(words.len().max(1)) {
            for window in words.windows(n) {
                // 複数のMSCコードが同じフレーズを含む場合（"kähler manifolds"
                // は32Q15と32J27の両方に現れる等）、最初に見つかったものを
                // 採用する。一意な選び方自体は本格的なEntity Resolutionの仕事。
                index.entry(window.join(" ")).or_insert_with(|| c.code.clone());
            }
        }
    }
    index
});

/// MSC名から、句読点で区切っただけの単語列を作る（RAKEのストップワード
/// 分割はせず、n-gram索引を作るための単純な単語抽出）。
fn rake_words_only(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut words = Vec::new();
    let mut cur = String::new();
    for c in lower.chars() {
        if c.is_alphanumeric() || c == '-' {
            cur.push(c);
        } else if !cur.is_empty() {
            words.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words
}

/// フレーズがMSC2020のいずれかのコード名にそのまま（あるいは末尾の
/// 単数/複数ゆれのみ吸収して）現れるかを調べる。O(1)（HashMap引き2回まで）。
fn ground_in_msc(phrase: &str) -> Option<String> {
    if let Some(code) = MSC_NGRAM_INDEX.get(phrase) {
        return Some(code.clone());
    }
    // 単数 <-> 複数の簡易吸収（本格的なEntity Resolutionはまだ行わない）
    let toggled = toggle_trailing_s(phrase);
    MSC_NGRAM_INDEX.get(&toggled).cloned()
}

fn toggle_trailing_s(phrase: &str) -> String {
    if let Some(stripped) = phrase.strip_suffix('s') {
        stripped.to_string()
    } else {
        format!("{phrase}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paper(id: &str, title: &str, abstract_text: &str) -> Paper {
        categorized(id, title, abstract_text, &[])
    }

    fn categorized(id: &str, title: &str, abstract_text: &str, categories: &[&str]) -> Paper {
        Paper {
            arxiv_id: id.to_string(),
            title: title.to_string(),
            abstract_text: abstract_text.to_string(),
            authors: vec![],
            categories: categories.iter().map(|c| c.to_string()).collect(),
            msc_codes: vec![],
            submitted: String::new(),
        }
    }

    /// 2分野が同数のコーパス。両分野の全論文が "earlier work"（執筆定型句）を
    /// 含み、各分野の論文だけがその分野固有の概念を含む。
    /// `MIN_LABELED_PAPERS`(=50)を超えるよう分野あたり60本にしてある。
    fn two_field_corpus() -> Vec<Paper> {
        let mut papers = Vec::new();
        for i in 0..60 {
            papers.push(categorized(
                &format!("nt{i}"),
                "On elliptic curves",
                "Our earlier work on elliptic curves is extended.",
                &["math.NT"],
            ));
        }
        for i in 0..60 {
            papers.push(categorized(
                &format!("ag{i}"),
                "On moduli spaces",
                "Our earlier work on moduli spaces is extended.",
                &["math.AG"],
            ));
        }
        papers
    }

    #[test]
    fn drops_a_phrase_whose_field_distribution_matches_the_corpus_as_boilerplate() {
        // "earlier work" は全120論文に出るので分野分布がコーパスそのもの
        // ＝どの分野の論文でも同じ割合で書かれる＝数学的内容を運ばない。
        // 手書きのブロックリストではなく、この統計だけで落ちること。
        let result = extract_candidates(&two_field_corpus(), 3);
        assert!(
            result.candidates.iter().all(|c| c.phrase != "earlier work"),
            "a phrase distributed exactly like the corpus must be dropped as boilerplate"
        );
        let dropped = result
            .dropped_as_boilerplate
            .iter()
            .find(|c| c.phrase == "earlier work")
            .expect("the dropped phrase must be reported, not silently discarded");
        assert!(dropped.field_concentration.unwrap() < concentration::GENERIC_MAX_CONCENTRATION);
    }

    #[test]
    fn keeps_a_phrase_that_concentrates_in_one_field_even_at_the_same_document_frequency() {
        // "elliptic curves" は math.NT の60論文にしか出ない。"earlier work" と
        // 文書頻度の桁は同じでも、分野に偏っている＝本物の概念。
        let result = extract_candidates(&two_field_corpus(), 3);
        let hit = result
            .candidates
            .iter()
            .find(|c| c.phrase == "elliptic curves")
            .expect("a field-specific phrase must survive");
        assert!(
            hit.field_concentration.unwrap() > concentration::GENERIC_MAX_CONCENTRATION,
            "got {:?}",
            hit.field_concentration
        );
        assert!(result.candidates.iter().any(|c| c.phrase == "moduli spaces"));
    }

    #[test]
    fn leaves_phrases_unscored_and_unfiltered_when_there_are_too_few_labeled_papers() {
        // 小さなコーパス（分野ラベル付きの論文が50件に満たない）では、
        // 集中度の推定が信用できない。ここで定型句を推測で落とすと本物の
        // 概念まで巻き込むので、スコアを付けず何も落とさないのが正しい。
        let papers: Vec<Paper> = (0..5)
            .map(|i| {
                categorized(
                    &i.to_string(),
                    "Large deviations for a simple group",
                    "Our earlier work on finite groups.",
                    &["math.PR"],
                )
            })
            .collect();
        let result = extract_candidates(&papers, 3);
        assert!(result.dropped_as_boilerplate.is_empty());
        assert!(result.candidates.iter().all(|c| c.field_concentration.is_none()));
        let phrases: HashSet<&str> = result.candidates.iter().map(|c| c.phrase.as_str()).collect();
        assert!(phrases.contains("earlier work"), "no evidence yet — keep it");
    }

    #[test]
    fn keeps_dual_use_words_when_they_form_a_real_concept() {
        // "large"/"simple"/"finite"/"number" は "large class"「simple proof"の
        // ような定型句にも、実在の数学概念にも現れる。単語単位のストップ
        // ワードで止められない（止めると概念名まで壊れる）ことの確認。
        let papers: Vec<Paper> = (0..5)
            .map(|i| {
                paper(
                    &i.to_string(),
                    "Large deviations for a simple group",
                    "We study finite groups and number theory.",
                )
            })
            .collect();
        let result = extract_candidates(&papers, 3);
        let phrases: HashSet<&str> = result.candidates.iter().map(|c| c.phrase.as_str()).collect();
        assert!(phrases.contains("large deviations"));
        assert!(phrases.contains("simple group"));
        assert!(phrases.contains("finite groups"));
        assert!(phrases.contains("number theory"));
    }

    #[test]
    fn drops_the_known_register_phrases_the_statistic_cannot_see() {
        // "mild assumptions" 等は、その言い回しを使う分野が偏っているせいで
        // 集中度が高く出てしまう（実データで0.688）。統計では落ちないと
        // 分かっているので、明示リストで補う——ここではその補いが実際に
        // 効いていることだけを確認する（分野ラベルすら無くても落ちる）。
        let papers: Vec<Paper> = (0..5)
            .map(|i| {
                paper(
                    &i.to_string(),
                    "A short proof of the main theorem",
                    "Under mild assumptions we obtain the result.",
                )
            })
            .collect();
        let result = extract_candidates(&papers, 3);
        for phrase in ["short proof", "main theorem", "mild assumptions"] {
            assert!(
                result.candidates.iter().all(|c| c.phrase != phrase),
                "{phrase} must be dropped by the explicit register list"
            );
            assert!(result.dropped_as_boilerplate.iter().any(|c| c.phrase == phrase));
        }
    }

    #[test]
    fn paper_links_drop_along_with_the_phrases_filtered_as_boilerplate() {
        let result = extract_candidates(&two_field_corpus(), 3);
        assert!(
            result.paper_links.iter().all(|(_, phrase)| phrase != "earlier work"),
            "links to a dropped phrase must not survive into paper_concepts"
        );
        assert!(result.paper_links.iter().any(|(_, phrase)| phrase == "elliptic curves"));
    }

    #[test]
    fn drops_phrases_below_min_doc_frequency() {
        let papers = vec![
            paper("1", "Kähler manifolds and rigidity", "We study Kähler manifolds."),
            paper("2", "A completely unrelated paper", "About something else entirely."),
        ];
        let result = extract_candidates(&papers, 2);
        assert!(
            result.candidates.iter().all(|c| c.phrase != "kähler manifolds"),
            "a phrase appearing in only 1 of 2 papers must be dropped at min_df=2"
        );
    }

    #[test]
    fn keeps_and_links_phrases_meeting_min_doc_frequency() {
        let papers = vec![
            paper("1", "Kähler manifolds and rigidity", "We study Kähler manifolds."),
            paper("2", "Kähler manifolds in complex geometry", "Kähler manifolds appear here too."),
        ];
        let result = extract_candidates(&papers, 2);
        let hit = result
            .candidates
            .iter()
            .find(|c| c.phrase == "kähler manifolds")
            .expect("kähler manifolds should survive min_df=2");
        assert_eq!(hit.doc_freq, 2);
        assert_eq!(hit.sample_arxiv_ids.len(), 2);

        let links: Vec<&str> = result
            .paper_links
            .iter()
            .filter(|(_, p)| p == "kähler manifolds")
            .map(|(id, _)| id.as_str())
            .collect();
        assert_eq!(links.len(), 2);
    }

    #[test]
    fn grounds_a_phrase_that_appears_verbatim_in_msc2020() {
        // "kähler manifolds" は "32Q15"（"Kähler manifolds"）と "32J27"
        // （"Compact Kähler manifolds: ..."）の両方に現れる（実データで確認
        // 済み）。どちらか一致すればよい——複数コードにまたがる場合の一意な
        // 選び方は本格的なEntity Resolution（Phase 3以降）の仕事。
        let code = ground_in_msc("kähler manifolds");
        assert!(
            matches!(code.as_deref(), Some("32Q15") | Some("32J27")),
            "expected 32Q15 or 32J27, got {code:?}"
        );
    }

    #[test]
    fn grounds_across_a_simple_singular_plural_mismatch() {
        // 抄録側は単数形で出現することが多い。MSC側は複数形("kähler manifolds")
        // なので、末尾の s の有無だけを吸収して同じ集合に解決できることを確認する。
        let code = ground_in_msc("kähler manifold");
        assert!(
            matches!(code.as_deref(), Some("32Q15") | Some("32J27")),
            "expected 32Q15 or 32J27, got {code:?}"
        );
    }

    #[test]
    fn leaves_genuinely_novel_terminology_ungrounded() {
        assert_eq!(ground_in_msc("a phrase nobody in msc2020 ever wrote"), None);
    }
}

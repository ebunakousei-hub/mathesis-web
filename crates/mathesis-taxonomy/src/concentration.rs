//! フレーズの「分野集中度」(field concentration)。
//!
//! `concepts.rs` の候補抽出は、RAKEが拾った語のうち何が本物の数学概念で
//! 何が単なる執筆上の定型句かを、それまで**フレーズの手書きリスト**
//! （"simple proof"・"large class" 等の完全一致ブロックリスト）で
//! 判定していた。100,000論文スケールテストで判明した通り、これは
//! いたちごっこであり、コーパスを増やすたびに新しい定型句を人手で
//! 見つけて足す必要があった（アーキテクチャ.txt「100,000論文スケール
//! テスト」①の「残る限界」）。
//!
//! ここではその代わりに、論文が自己申告している分野ラベルを使った
//! 統計量でそれを判定する:
//!
//!   そのフレーズを含む論文の分野分布が、コーパス全体の分野分布と
//!   ほぼ同じ  = どの分野の論文でも同じ割合で書かれる = 執筆の定型句
//!   特定の分野に偏っている                          = 本物の概念
//!
//! 具体的には、フレーズ出現論文の分野分布 p と、コーパス全体の分野分布
//! （事前分布）q の Kullback-Leibler ダイバージェンス D(p‖q) を取る。
//! 「一様さ」ではなく「コーパスの偏りからのズレ」を測るのが要点——
//! arXivのmathコーパスは元々 math.AG や math-ph に偏っているので、
//! エントロピーをそのまま見ると偏ったコーパスの偏りを概念の性質と
//! 取り違える。
//!
//! # 分野ラベルに何を使うか
//!
//! 設計メモ（アーキテクチャ.txt）では「自己申告MSCコードの分野分布」と
//! していたが、実データ（100,000論文）で両方を実装して比べたところ:
//!
//!   - 自己申告MSCコード:  60,330/100,000論文 (60.3%) にしか付いていない
//!   - arXivカテゴリ:     100,000/100,000論文 (100%)
//!
//! で、かつ両者の D(p‖q) は同じ判定を与えた（"moduli space" 0.873 vs
//! 0.860、"lie algebra" 0.635 vs 0.874、"lower bound" 0.264 vs 0.155 の
//! ように、絶対値は多少ずれても「本物か定型句か」の順序は一致した）。
//! 同じ判定が得られるなら被覆率の高い方が統計的に有利なので、
//! ラベルには arXiv カテゴリを使う。
//!
//! # 小標本バイアスの扱い
//!
//! KLダイバージェンスは標本数が少ないと過大評価される（3論文しか無い
//! フレーズは、たまたま同じ分野に落ちるだけで「集中している」ように
//! 見える）。そこで q へ向けた加法平滑化を掛けた上で、ラベル付き論文が
//! `MIN_LABELED_PAPERS` 件に満たないフレーズには**スコアを付けない**
//! （`None`）。平滑化は推定値を q 側へ引き寄せる＝集中度を過小評価する
//! 方向にしか効かないので、「スコアが閾値未満なら定型句」という判定に
//! 使うと、標本が少ないフレーズを誤って定型句と断じてしまう。それを
//! 避けるため、証拠が足りないものは判定対象から外すという設計にする。

use mathesis_ingest::model::Paper;
use std::collections::HashMap;

/// arXivのカテゴリには完全な別名（同じ論文に必ず両方付く）が存在する。
/// 実データ（100,000論文）で出現数が1件も違わないことを確認したペアだけを
/// 挙げる: math-ph=math.MP が17,659件、cs.IT=math.IT が3,828件、
/// stat.TH=math.ST が2,836件、cs.NA=math.NA が47件。正規化しないと、
/// これらの分野の論文だけが分野質量を2ラベルに分散させることになる。
const CATEGORY_ALIASES: &[(&str, &str)] = &[
    ("math-ph", "math.MP"),
    ("cs.IT", "math.IT"),
    ("stat.TH", "math.ST"),
    ("cs.NA", "math.NA"),
];

/// 分野ラベルの付いた論文がこれ未満のフレーズには集中度を付けない。
/// 実データでの平滑化の効き（`SMOOTHING`）から決めた値——50件あれば
/// 観測分布の重みが 50/(50+10) = 83% となり、平滑化による過小評価が
/// 判定を歪めない範囲に収まる。
pub const MIN_LABELED_PAPERS: usize = 50;

/// 事前分布 q へ向けた加法平滑化の強さ（Dirichlet事前分布の疑似カウント）。
const SMOOTHING: f64 = 10.0;

/// この値未満の集中度＝「コーパス全体とほぼ同じ分野分布」＝定型句として
/// 候補から落とす閾値。
///
/// 実データ（100,000論文、`--min-df 3`）で全候補のスコアを並べて決めた:
/// 0.24以下は目視した限り全て執筆定型句（"earlier work" 0.086 /
/// "necessary condition" 0.128 / "basic properties" 0.130 / "natural way"
/// 0.140 / "explicit formula" 0.152 / "key ingredient" 0.168 / "main goal"
/// 0.169 / "central role" 0.179 …）で、そこに本物の数学概念は現れなかった。
/// 一方、複数分野にまたがって使われる本物の概念のうち最も低かったのは
/// "formal power series" 0.247 で、次いで "laurent polynomials" 0.274、
/// "group action" 0.270、"fixed points" 0.290、"orthogonal group" 0.295。
///
/// つまり「全部定型句の帯」と「本物が現れ始める点」の間には 0.19〜0.247 の
/// 空きがあり、閾値はその中に置ける。**本物の概念を誤って落とす方が、
/// 定型句を残すよりコストが高い**（落とした概念は下流のクラスタリング・
/// 検索から完全に消えるが、残った定型句は集中度スコア自体で見分けられる）
/// ため、境界の中でも安全側の 0.22 を採る——"formal power series" までの
/// 余裕を 0.027 残す。
pub const GENERIC_MAX_CONCENTRATION: f32 = 0.22;

/// 別名を代表ラベルへ寄せる。
fn normalize_label(label: &str) -> &str {
    CATEGORY_ALIASES
        .iter()
        .find(|(alias, _)| *alias == label)
        .map(|(_, canonical)| *canonical)
        .unwrap_or(label)
}

/// 1論文に付く分野ラベル（正規化・重複除去済み）。
fn field_labels(paper: &Paper) -> Vec<&str> {
    let mut labels: Vec<&str> = paper.categories.iter().map(|c| normalize_label(c)).collect();
    labels.sort_unstable();
    labels.dedup();
    labels
}

/// コーパス全体の分野分布 q。ラベルは内部で0始まりのidに詰め替える
/// （フレーズごとの集計で文字列をハッシュし直さずに済ませるため）。
pub struct FieldPrior {
    ids: HashMap<String, u32>,
    prob: Vec<f64>,
}

impl FieldPrior {
    /// 各論文が合計1の質量を持ち、それを自分のラベル数で等分して各分野へ
    /// 配る（3分野にクロスリストされた論文は各分野へ1/3ずつ）。こうすると
    /// 「クロスリストの多い論文が分野分布を余計に押し上げる」ことがなく、
    /// フレーズ側の集計とも同じ数え方になるので比較できる。
    pub fn from_papers(papers: &[Paper]) -> Self {
        let mut mass: HashMap<&str, f64> = HashMap::new();
        let mut total = 0.0;
        for p in papers {
            let labels = field_labels(p);
            if labels.is_empty() {
                continue;
            }
            let w = 1.0 / labels.len() as f64;
            for l in labels {
                *mass.entry(l).or_default() += w;
                total += w;
            }
        }
        if total <= 0.0 {
            return Self { ids: HashMap::new(), prob: Vec::new() };
        }

        // ラベル名でソートしてからidを振る——同じコーパスなら常に同じidに
        // なり、集中度の値が実行ごとにぶれない。
        let mut labels: Vec<(&str, f64)> = mass.into_iter().collect();
        labels.sort_unstable_by_key(|(l, _)| *l);

        let mut ids = HashMap::with_capacity(labels.len());
        let mut prob = Vec::with_capacity(labels.len());
        for (i, (label, m)) in labels.into_iter().enumerate() {
            ids.insert(label.to_string(), i as u32);
            prob.push(m / total);
        }
        Self { ids, prob }
    }

    pub fn label_count(&self) -> usize {
        self.prob.len()
    }

    /// 論文の分野ラベルidと、1ラベルあたりの質量（1/ラベル数）。
    /// ラベルが1つも無ければ空を返す。
    pub fn label_ids(&self, paper: &Paper) -> (Vec<u32>, f64) {
        let ids: Vec<u32> = field_labels(paper)
            .into_iter()
            .filter_map(|l| self.ids.get(l).copied())
            .collect();
        let weight = if ids.is_empty() { 0.0 } else { 1.0 / ids.len() as f64 };
        (ids, weight)
    }

    /// 分野ごとの観測質量 `counts` から D(p‖q) を求める。
    /// `labeled_papers` は、そのフレーズを含む論文のうち分野ラベルを
    /// 持っていたものの件数——`MIN_LABELED_PAPERS` 未満なら `None`。
    pub fn concentration(&self, counts: &[(u32, f64)], labeled_papers: usize) -> Option<f32> {
        if self.prob.is_empty() || labeled_papers < MIN_LABELED_PAPERS {
            return None;
        }
        let observed_total: f64 = counts.iter().map(|(_, w)| w).sum();
        if observed_total <= 0.0 {
            return None;
        }

        let mut observed = vec![0.0_f64; self.prob.len()];
        for &(id, w) in counts {
            if let Some(slot) = observed.get_mut(id as usize) {
                *slot += w;
            }
        }

        let denom = observed_total + SMOOTHING;
        let mut kl = 0.0;
        for (i, &q) in self.prob.iter().enumerate() {
            if q <= 0.0 {
                continue;
            }
            // 平滑化後の p は、観測が0の分野でも SMOOTHING*q/denom > 0 に
            // なるので、log(p/q) が -inf になることはない。
            let p = (observed[i] + SMOOTHING * q) / denom;
            kl += p * (p / q).ln();
        }
        Some(kl as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paper(id: &str, categories: &[&str]) -> Paper {
        Paper {
            arxiv_id: id.to_string(),
            title: String::new(),
            abstract_text: String::new(),
            authors: vec![],
            categories: categories.iter().map(|c| c.to_string()).collect(),
            msc_codes: vec![],
            submitted: String::new(),
        }
    }

    /// 分野ラベルが `labels` の論文に `per_label` 件ずつ出現したフレーズの
    /// 観測質量（各論文はラベル1つなので質量1）。
    fn counts_for(prior: &FieldPrior, labels: &[(&str, f64)]) -> Vec<(u32, f64)> {
        labels.iter().map(|(l, w)| (prior.ids[*l], *w)).collect()
    }

    fn corpus() -> Vec<Paper> {
        // math.AG 60件 / math.PR 30件 / math.CO 10件 の偏ったコーパス。
        let mut papers = Vec::new();
        for i in 0..60 {
            papers.push(paper(&format!("ag{i}"), &["math.AG"]));
        }
        for i in 0..30 {
            papers.push(paper(&format!("pr{i}"), &["math.PR"]));
        }
        for i in 0..10 {
            papers.push(paper(&format!("co{i}"), &["math.CO"]));
        }
        papers
    }

    #[test]
    fn prior_reflects_the_corpus_field_distribution() {
        let prior = FieldPrior::from_papers(&corpus());
        assert_eq!(prior.label_count(), 3);
        assert!((prior.prob[prior.ids["math.AG"] as usize] - 0.6).abs() < 1e-9);
        assert!((prior.prob[prior.ids["math.PR"] as usize] - 0.3).abs() < 1e-9);
        assert!((prior.prob[prior.ids["math.CO"] as usize] - 0.1).abs() < 1e-9);
    }

    #[test]
    fn a_phrase_distributed_exactly_like_the_corpus_scores_near_zero() {
        // これが「定型句」の定義そのもの——偏ったコーパスでも、その偏りを
        // そのままなぞるフレーズは何の分野情報も持たない。
        let prior = FieldPrior::from_papers(&corpus());
        let counts = counts_for(&prior, &[("math.AG", 120.0), ("math.PR", 60.0), ("math.CO", 20.0)]);
        let score = prior.concentration(&counts, 200).unwrap();
        assert!(score < 0.01, "expected ~0 for a corpus-shaped phrase, got {score}");
    }

    #[test]
    fn a_phrase_concentrated_in_one_field_scores_high() {
        let prior = FieldPrior::from_papers(&corpus());
        let counts = counts_for(&prior, &[("math.CO", 200.0)]);
        let score = prior.concentration(&counts, 200).unwrap();
        assert!(score > 1.5, "expected a strongly concentrated phrase to score high, got {score}");
    }

    #[test]
    fn concentration_is_measured_against_the_corpus_bias_not_against_uniformity() {
        // math.AG だけに出るフレーズは、コーパスの60%がmath.AGなので
        // 「1分野に集中」していても情報量は小さい。一方 math.CO
        // （コーパスの10%）だけに出るフレーズは同じ「1分野に集中」でも
        // ずっと強い分野シグナルになる。エントロピーではこの差は出ない。
        let prior = FieldPrior::from_papers(&corpus());
        let dominant_field = prior.concentration(&counts_for(&prior, &[("math.AG", 200.0)]), 200).unwrap();
        let rare_field = prior.concentration(&counts_for(&prior, &[("math.CO", 200.0)]), 200).unwrap();
        assert!(
            rare_field > dominant_field,
            "math.CO-only ({rare_field}) should carry more field information than math.AG-only ({dominant_field})"
        );
    }

    #[test]
    fn phrases_with_too_few_labeled_papers_get_no_score_rather_than_a_noisy_one() {
        // 平滑化は集中度を過小評価する方向にしか効かないので、標本が
        // 少ないフレーズにスコアを付けると「定型句」と誤判定されうる。
        // 判定に足る証拠が無い場合は None を返すのが正しい。
        let prior = FieldPrior::from_papers(&corpus());
        let counts = counts_for(&prior, &[("math.CO", (MIN_LABELED_PAPERS - 1) as f64)]);
        assert_eq!(prior.concentration(&counts, MIN_LABELED_PAPERS - 1), None);
        assert!(prior.concentration(&counts, MIN_LABELED_PAPERS).is_some());
    }

    #[test]
    fn concentration_is_never_negative() {
        // D(p‖q) >= 0 は KL の基本性質。平滑化を挟んでも壊れないことを、
        // 事前分布から少しだけずれた分布で確認する。
        let prior = FieldPrior::from_papers(&corpus());
        let counts = counts_for(&prior, &[("math.AG", 119.0), ("math.PR", 61.0), ("math.CO", 20.0)]);
        let score = prior.concentration(&counts, 200).unwrap();
        assert!(score >= 0.0, "KL divergence must be non-negative, got {score}");
    }

    #[test]
    fn aliased_arxiv_categories_are_folded_into_one_field() {
        // math-ph と math.MP は同じ分野の別名で、実データでは必ず両方付く。
        // 正規化しないと、この分野の論文だけ質量が2ラベルに割れてしまう。
        let papers = vec![paper("1", &["math-ph", "math.MP"]), paper("2", &["math.AG"])];
        let prior = FieldPrior::from_papers(&papers);
        assert_eq!(prior.label_count(), 2, "math-ph/math.MP must collapse into one label");
        assert!((prior.prob[prior.ids["math.MP"] as usize] - 0.5).abs() < 1e-9);
    }

    #[test]
    fn cross_listed_papers_split_their_mass_instead_of_counting_twice() {
        let papers = vec![paper("1", &["math.AG", "math.PR"]), paper("2", &["math.AG"])];
        let prior = FieldPrior::from_papers(&papers);
        // math.AG は 0.5(論文1) + 1.0(論文2) = 1.5、math.PR は 0.5、合計2.0
        assert!((prior.prob[prior.ids["math.AG"] as usize] - 0.75).abs() < 1e-9);
        assert!((prior.prob[prior.ids["math.PR"] as usize] - 0.25).abs() < 1e-9);
    }

    #[test]
    fn papers_without_any_category_are_simply_not_counted() {
        let papers = vec![paper("1", &["math.AG"]), paper("2", &[])];
        let prior = FieldPrior::from_papers(&papers);
        assert_eq!(prior.label_count(), 1);
        let (ids, weight) = prior.label_ids(&papers[1]);
        assert!(ids.is_empty());
        assert_eq!(weight, 0.0);
    }

    #[test]
    fn a_corpus_with_no_categories_at_all_yields_no_scores() {
        // 分野ラベルが1つも無いコーパス（テスト用の小さなDB等）でも
        // パイプライン全体は動き続けるべき——単に全候補が None になる。
        let papers = vec![paper("1", &[]), paper("2", &[])];
        let prior = FieldPrior::from_papers(&papers);
        assert_eq!(prior.label_count(), 0);
        assert_eq!(prior.concentration(&[], 1000), None);
    }
}

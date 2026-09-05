//! 概念の**文脈ベクトル**——共起のPPMI行列をランダム化対称固有分解で
//! 低次元に落としたもの。`embed.rs`（文字列embedding）の置き換え。
//!
//! # なぜこれが要るか（実データでの測定結果）
//!
//! `embed.rs` は概念を `embed_text(model, phrase)` ——つまり**フレーズ文字列
//! そのもの**をOllamaに投げて表現していた。100,000論文コーパスの出力
//! （`taxonomy.related.json`、関連リスト74,566件）を全走査したところ:
//!
//!   語幹を共有する候補が6割以上を占める関連リスト … 64,520 / 74,566 = **86.5%**
//!
//!   riemann hypothesis → extended riemann hypothesis, riemann function（以上）
//!   brownian motion    → brownian motions, ordinary brownian motion,
//!                        driving brownian motion, brownian motion model, …
//!   elliptic curves    → elliptic curves ii, rational elliptic curves,
//!                        elliptic plane curves, modular elliptic curves, …
//!
//! zeta functionもL関数も素数定理もRiemann予想の隣に無く、Wiener過程も
//! マルチンゲールもBrownian motionの隣に無い。**文字列をembedしている限り、
//! cos類似度は文字列類似度の言い換えにしかならない。**クラスタも同じで、
//! cluster 22 は "elliptic curve" の綴り10種、cluster 28 は "quantum group"
//! の綴り12種だった——タクソノミー構築のつもりのパイプラインが、実際には
//! Entity Resolution（アーキテクチャ.txt 5.4 が*クラスタリングの前段*に
//! 置いた工程）を実行していた。
//!
//! ここでは概念を「名前」ではなく「**どの概念と同じ論文に現れるか**」で
//! 表す。分布仮説そのもので、名前が似ていることに一切依存しない——だから
//! 上の失敗モードが構造的に起こり得ない。
//!
//! # 手順
//!
//! 1. 同一論文内の概念ペアの共起回数を数える（`paper_concepts` 由来）。
//! 2. PPMI（正の相互情報量）で重み付ける。生の共起回数をそのまま使うと
//!    「どちらも高頻度だから一緒に出る」だけのペアが最大になるので、
//!    周辺確率で割って「偶然より何倍一緒に出るか」に直す必要がある。
//!    文脈側の周辺分布に指数 α を掛ける平滑化（Levy–Goldberg 2015）は
//!    低頻度の概念のPMIが過大評価されるのを抑える。α=1 で教科書どおりの
//!    PPMIに一致する（`ppmi_matrix` のテストで確認）。
//! 3. ランダム化対称固有分解で d 次元へ落とす。PPMI行列は n×n
//!    （n=80,727なら65億要素）だが、非ゼロは共起した組だけなので疎行列
//!    のまま扱える。乱数はここでも固定シードの自前PRNG——`ann.rs` と
//!    同じ理由で、クラスタリング結果の再現性を保つため `std` の乱数は
//!    使わない。
//! 4. 各行をL2正規化する（cos類似度がそのまま内積になる）。
//!
//! # 外部サービスへの依存が消えること
//!
//! この経路は Ollama を必要としない。純Rust・決定的・同じ入力からは
//! いつでも同じベクトルが出る。`embed.rs` は比較・回帰確認のために残す
//! （同じグラフ構築関数に両方を通し、NMI・純度・関連リストの中身で
//! 優劣を数字で比べられる）。

use std::collections::HashMap;

/// 文脈ベクトル構築のパラメータ。
#[derive(Debug, Clone)]
pub struct ContextParams {
    /// 出力する次元数。
    pub dim: usize,
    /// この回数未満しか共起していないペアは捨てる。1回だけの共起は
    /// PMIが最大級に大きく出る（分母の周辺確率が小さいので）一方、
    /// 統計的な裏付けが無い——ノイズを増幅するだけなので落とす。
    pub min_cooccur: u32,
    /// 文脈側周辺分布の平滑化指数。1.0 で教科書どおりのPPMI。
    pub context_alpha: f64,
    /// ランダム化固有分解のべき乗反復回数。多いほど上位固有空間の
    /// 近似が良くなる（が、その分行列積を繰り返す）。
    pub power_iterations: usize,
    /// 固有値の重み付け指数。埋め込みを U·|Λ|^p とするときの p。
    /// 0.0 なら固有ベクトルそのまま、1.0 なら固有値をそのまま掛ける。
    /// 0.5 が word embedding で標準的（Levy–Goldberg 2015）。
    pub eigenvalue_power: f32,
    /// 乱数シード（決定性のため固定）。
    pub seed: u64,
}

impl Default for ContextParams {
    fn default() -> Self {
        Self {
            dim: 192,
            min_cooccur: 2,
            context_alpha: 0.75,
            power_iterations: 2,
            eigenvalue_power: 0.5,
            seed: 0x5EED_C0FF_EE12_3456,
        }
    }
}

/// 構築の統計。「何を使って何を落としたか」を呼び出し側が表示できるように
/// 返す——`extract` が除外内容を必ず表示するのと同じ方針。
#[derive(Debug, Clone, Default)]
pub struct ContextStats {
    /// 共起を1件でも持っていた概念の数。
    pub covered: usize,
    /// 共起が無く零ベクトルになった概念の数。
    pub isolated: usize,
    /// `min_cooccur` を通ったペア数（PPMI行列の非ゼロ数の半分）。
    pub kept_pairs: usize,
    /// `min_cooccur` で落としたペア数。
    pub dropped_pairs: usize,
    /// PPMIが0以下になって落ちたペア数（`kept_pairs` のうち）。
    pub zero_ppmi_pairs: usize,
}

/// 対称疎行列（上三角のみ保持し、行列積のときに両方向へ効かせる）。
#[derive(Debug, Clone, Default)]
pub struct SparseSym {
    pub n: usize,
    /// (i, j, value)、i < j。
    pub entries: Vec<(u32, u32, f32)>,
}

impl SparseSym {
    /// y = A x（xは n×cols の行優先密行列、yも同形）。
    fn multiply(&self, x: &[f32], cols: usize, out: &mut [f32]) {
        out.iter_mut().for_each(|v| *v = 0.0);
        for &(i, j, w) in &self.entries {
            let (i, j) = (i as usize, j as usize);
            let (xi, xj) = (i * cols, j * cols);
            for c in 0..cols {
                out[xi + c] += w * x[xj + c];
                out[xj + c] += w * x[xi + c];
            }
        }
    }
}

/// `paper_concept_nodes` は (arxiv_id, 概念のノード添字) のペア列。
/// `n` は概念の総数（ノード添字の上限）。
pub fn build_context_vectors(
    n: usize,
    paper_concept_nodes: &[(String, usize)],
    params: &ContextParams,
) -> (Vec<Vec<f32>>, ContextStats) {
    let (matrix, mut stats) = ppmi_matrix(n, paper_concept_nodes, params);
    let vectors = spectral_embedding(&matrix, params);
    stats.covered = vectors.iter().filter(|v| v.iter().any(|&x| x != 0.0)).count();
    stats.isolated = n - stats.covered;
    (vectors, stats)
}

/// 共起カウント → PPMI行列。
pub fn ppmi_matrix(
    n: usize,
    paper_concept_nodes: &[(String, usize)],
    params: &ContextParams,
) -> (SparseSym, ContextStats) {
    let mut by_paper: HashMap<&str, Vec<u32>> = HashMap::new();
    for (arxiv_id, node) in paper_concept_nodes {
        by_paper.entry(arxiv_id.as_str()).or_default().push(*node as u32);
    }

    let mut counts: HashMap<(u32, u32), u32> = HashMap::new();
    for nodes in by_paper.values_mut() {
        nodes.sort_unstable();
        nodes.dedup();
        for a in 0..nodes.len() {
            for b in (a + 1)..nodes.len() {
                *counts.entry((nodes[a], nodes[b])).or_default() += 1;
            }
        }
    }

    let mut stats = ContextStats::default();
    let mut kept: Vec<((u32, u32), u32)> = Vec::new();
    for (pair, count) in counts {
        if count >= params.min_cooccur {
            kept.push((pair, count));
        } else {
            stats.dropped_pairs += 1;
        }
    }
    // HashMapの反復順は実行ごとに変わりうるので、以降の計算（特に浮動小数
    // の加算順）を決定的にするために必ず並べ替える。
    kept.sort_unstable();
    stats.kept_pairs = kept.len();

    // 周辺質量。ペア(i,j)はiとjの両方に count ぶん寄与する。
    let mut row_sum = vec![0.0f64; n];
    for &((i, j), c) in &kept {
        row_sum[i as usize] += c as f64;
        row_sum[j as usize] += c as f64;
    }
    let total: f64 = row_sum.iter().sum();
    if total <= 0.0 {
        return (SparseSym { n, entries: Vec::new() }, stats);
    }

    // 平滑化した周辺分布 p_α(k) = row_k^α / Σ row^α。α=1 なら row_k/total に
    // 一致し、PMI が教科書どおりの ln(c·total/(row_i·row_j)) に戻る。
    let alpha = params.context_alpha;
    let smoothed: Vec<f64> = row_sum.iter().map(|&r| if r > 0.0 { r.powf(alpha) } else { 0.0 }).collect();
    let smoothed_total: f64 = smoothed.iter().sum();

    let mut entries = Vec::with_capacity(kept.len());
    for &((i, j), c) in &kept {
        let p_ij = c as f64 / total;
        let p_i = smoothed[i as usize] / smoothed_total;
        let p_j = smoothed[j as usize] / smoothed_total;
        if p_i <= 0.0 || p_j <= 0.0 {
            continue;
        }
        let pmi = (p_ij / (p_i * p_j)).ln();
        if pmi > 0.0 {
            entries.push((i, j, pmi as f32));
        } else {
            stats.zero_ppmi_pairs += 1;
        }
    }

    (SparseSym { n, entries }, stats)
}

/// ランダム化対称固有分解 → 行L2正規化された d 次元ベクトル。
pub fn spectral_embedding(matrix: &SparseSym, params: &ContextParams) -> Vec<Vec<f32>> {
    let n = matrix.n;
    let dim = params.dim.min(n);
    if n == 0 || dim == 0 || matrix.entries.is_empty() {
        return vec![vec![0.0; dim]; n];
    }
    // オーバーサンプリング（Halko et al. 2011）。上位dim個の固有空間を
    // 安定して捉えるために少し余分に取ってから切り詰める。
    let l = (dim + 10).min(n);

    let mut rng = DeterministicRng::new(params.seed);
    let mut y: Vec<f32> = (0..n * l).map(|_| rng.next_gaussian()).collect();
    let mut buf = vec![0.0f32; n * l];

    matrix.multiply(&y, l, &mut buf);
    std::mem::swap(&mut y, &mut buf);
    for _ in 0..params.power_iterations {
        orthonormalize(&mut y, n, l);
        matrix.multiply(&y, l, &mut buf);
        std::mem::swap(&mut y, &mut buf);
    }
    orthonormalize(&mut y, n, l);
    let q = y;

    // T = Qᵀ A Q （l×l、対称）
    let mut aq = vec![0.0f32; n * l];
    matrix.multiply(&q, l, &mut aq);
    let mut t = vec![0.0f64; l * l];
    for row in 0..n {
        let qr = &q[row * l..row * l + l];
        let ar = &aq[row * l..row * l + l];
        for a in 0..l {
            let qa = qr[a] as f64;
            if qa == 0.0 {
                continue;
            }
            for b in 0..l {
                t[a * l + b] += qa * ar[b] as f64;
            }
        }
    }
    // 丸め誤差で崩れた対称性を戻す（Jacobiは厳密に対称な行列を要求する）。
    for a in 0..l {
        for b in (a + 1)..l {
            let m = 0.5 * (t[a * l + b] + t[b * l + a]);
            t[a * l + b] = m;
            t[b * l + a] = m;
        }
    }

    let (eigenvalues, eigenvectors) = jacobi_eigen(&mut t, l);
    let mut order: Vec<usize> = (0..l).collect();
    order.sort_by(|&a, &b| eigenvalues[b].abs().partial_cmp(&eigenvalues[a].abs()).unwrap());
    order.truncate(dim);

    // U = Q V、そのあと |λ|^p で重み付け。
    let scale: Vec<f32> = order
        .iter()
        .map(|&k| (eigenvalues[k].abs() as f32).powf(params.eigenvalue_power))
        .collect();

    let mut out = Vec::with_capacity(n);
    for row in 0..n {
        let qr = &q[row * l..row * l + l];
        let mut v = Vec::with_capacity(dim);
        for (slot, &k) in order.iter().enumerate() {
            let mut acc = 0.0f32;
            for (a, &qa) in qr.iter().enumerate() {
                acc += qa * eigenvectors[a * l + k] as f32;
            }
            v.push(acc * scale[slot]);
        }
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 1e-12 {
            v.iter_mut().for_each(|x| *x /= norm);
        } else {
            v.iter_mut().for_each(|x| *x = 0.0);
        }
        out.push(v);
    }
    out
}

/// 修正グラム・シュミット（列を正規直交化する。`m` は n×cols の行優先）。
fn orthonormalize(m: &mut [f32], n: usize, cols: usize) {
    for c in 0..cols {
        for prev in 0..c {
            let mut dot = 0.0f64;
            for row in 0..n {
                dot += m[row * cols + c] as f64 * m[row * cols + prev] as f64;
            }
            let dot = dot as f32;
            if dot != 0.0 {
                for row in 0..n {
                    m[row * cols + c] -= dot * m[row * cols + prev];
                }
            }
        }
        let mut norm = 0.0f64;
        for row in 0..n {
            let v = m[row * cols + c] as f64;
            norm += v * v;
        }
        let norm = norm.sqrt() as f32;
        if norm > 1e-12 {
            for row in 0..n {
                m[row * cols + c] /= norm;
            }
        } else {
            // 数値的に潰れた列は0にする（後段で固有値が0になり自然に落ちる）。
            for row in 0..n {
                m[row * cols + c] = 0.0;
            }
        }
    }
}

/// 巡回Jacobi法による実対称行列の固有分解。`a` は l×l 行優先（破壊的）。
/// 戻り値は (固有値, 固有ベクトル行列 l×l 行優先——列kが第k固有ベクトル)。
fn jacobi_eigen(a: &mut [f64], l: usize) -> (Vec<f64>, Vec<f64>) {
    let mut v = vec![0.0f64; l * l];
    for i in 0..l {
        v[i * l + i] = 1.0;
    }
    for _sweep in 0..100 {
        let mut off = 0.0f64;
        for p in 0..l {
            for q in (p + 1)..l {
                off += a[p * l + q] * a[p * l + q];
            }
        }
        if off.sqrt() < 1e-12 {
            break;
        }
        for p in 0..l {
            for q in (p + 1)..l {
                let apq = a[p * l + q];
                if apq.abs() < 1e-18 {
                    continue;
                }
                let theta = (a[q * l + q] - a[p * l + p]) / (2.0 * apq);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for k in 0..l {
                    let akp = a[k * l + p];
                    let akq = a[k * l + q];
                    a[k * l + p] = c * akp - s * akq;
                    a[k * l + q] = s * akp + c * akq;
                }
                for k in 0..l {
                    let apk = a[p * l + k];
                    let aqk = a[q * l + k];
                    a[p * l + k] = c * apk - s * aqk;
                    a[q * l + k] = s * apk + c * aqk;
                }
                for k in 0..l {
                    let vkp = v[k * l + p];
                    let vkq = v[k * l + q];
                    v[k * l + p] = c * vkp - s * vkq;
                    v[k * l + q] = s * vkp + c * vkq;
                }
            }
        }
    }
    let eigenvalues = (0..l).map(|i| a[i * l + i]).collect();
    (eigenvalues, v)
}

/// 固定シードのPRNG（`ann.rs` と同じ理由——結果の再現性のため `std` の
/// 乱数は使わない）。SplitMix64。
struct DeterministicRng(u64);

impl DeterministicRng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn next_unit(&mut self) -> f64 {
        // [0,1) の一様乱数。
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Box–Muller法による標準正規乱数。
    fn next_gaussian(&mut self) -> f32 {
        let u1 = self.next_unit().max(1e-12);
        let u2 = self.next_unit();
        ((-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn links(pairs: &[(&str, usize)]) -> Vec<(String, usize)> {
        pairs.iter().map(|(a, n)| (a.to_string(), *n)).collect()
    }

    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b).map(|(x, y)| x * y).sum()
    }

    #[test]
    fn ppmi_reduces_to_the_textbook_formula_when_alpha_is_one() {
        // 2概念が2論文で共起するだけの最小構成。α=1 なら
        // PMI = ln(c·total/(row_i·row_j)) がそのまま出るはず。
        let data = links(&[("p1", 0), ("p1", 1), ("p2", 0), ("p2", 1)]);
        let params = ContextParams { min_cooccur: 1, context_alpha: 1.0, ..Default::default() };
        let (m, _) = ppmi_matrix(2, &data, &params);
        // c=2, row_0=row_1=2, total=4 → ln(2*4/(2*2)) = ln(2)
        assert_eq!(m.entries.len(), 1);
        let (i, j, w) = m.entries[0];
        assert_eq!((i, j), (0, 1));
        assert!((w - std::f32::consts::LN_2).abs() < 1e-5, "PMI={w}, 期待 ln2={}", std::f32::consts::LN_2);
    }

    #[test]
    fn min_cooccur_drops_single_occurrence_pairs() {
        let data = links(&[("p1", 0), ("p1", 1)]);
        let params = ContextParams { min_cooccur: 2, ..Default::default() };
        let (m, stats) = ppmi_matrix(2, &data, &params);
        assert!(m.entries.is_empty());
        assert_eq!(stats.dropped_pairs, 1);
    }

    #[test]
    fn repeated_concepts_within_one_paper_count_once() {
        // 同じ論文に同じ概念が2回リンクされていても共起は1回。
        // （`paper_concepts` は主キーで重複しないが、呼び出し側が
        //  重複を渡してきても結果が変わらないことを保証する。）
        let a = links(&[("p1", 0), ("p1", 0), ("p1", 1)]);
        let b = links(&[("p1", 0), ("p1", 1)]);
        let params = ContextParams { min_cooccur: 1, ..Default::default() };
        assert_eq!(ppmi_matrix(2, &a, &params).0.entries, ppmi_matrix(2, &b, &params).0.entries);
    }

    #[test]
    fn concepts_sharing_papers_end_up_closer_than_concepts_that_never_co_occur() {
        // これがこのモジュールの存在理由そのもの。名前は一切見ずに
        // 「同じ論文に出るか」だけで距離が決まることを確認する。
        // 0,1,2 は同じ論文群に、3,4,5 は別の論文群に現れる。
        let mut data = Vec::new();
        for p in 0..12 {
            let paper = format!("A{p}");
            for c in 0..3 {
                data.push((paper.clone(), c));
            }
        }
        for p in 0..12 {
            let paper = format!("B{p}");
            for c in 3..6 {
                data.push((paper.clone(), c));
            }
        }
        let params = ContextParams { dim: 4, min_cooccur: 2, ..Default::default() };
        let (vectors, stats) = build_context_vectors(6, &data, &params);
        assert_eq!(stats.isolated, 0, "全概念が共起を持つはず");
        let within = cosine(&vectors[0], &vectors[1]);
        let across = cosine(&vectors[0], &vectors[4]);
        assert!(
            within > across,
            "同じ論文群に出る概念(0,1)={within} が、出ない概念(0,4)={across} より近いはず"
        );
    }

    #[test]
    fn a_bridging_concept_sits_between_two_communities() {
        // 両方のコミュニティに跨がる概念5は、どちらの内部ペアよりも
        // 反対側に近くなる——文字列類似度では絶対に出せない性質。
        let mut data = Vec::new();
        for p in 0..15 {
            let paper = format!("A{p}");
            for c in [0usize, 1, 2, 5] {
                data.push((paper.clone(), c));
            }
        }
        for p in 0..15 {
            let paper = format!("B{p}");
            for c in [3usize, 4, 5] {
                data.push((paper.clone(), c));
            }
        }
        let params = ContextParams { dim: 4, min_cooccur: 2, ..Default::default() };
        let (vectors, _) = build_context_vectors(6, &data, &params);
        assert!(
            cosine(&vectors[5], &vectors[3]) > cosine(&vectors[0], &vectors[3]),
            "橋渡し概念5はコミュニティBに、コミュニティAの内部概念0より近いはず"
        );
    }

    #[test]
    fn isolated_concepts_get_zero_vectors_and_are_reported() {
        let mut data = Vec::new();
        for p in 0..8 {
            let paper = format!("A{p}");
            data.push((paper.clone(), 0));
            data.push((paper, 1));
        }
        // 概念2はどの論文にも現れない。
        let params = ContextParams { dim: 4, min_cooccur: 2, ..Default::default() };
        let (vectors, stats) = build_context_vectors(3, &data, &params);
        assert_eq!(stats.isolated, 1);
        assert!(vectors[2].iter().all(|&x| x == 0.0), "共起証拠の無い概念は零ベクトル");
    }

    #[test]
    fn output_is_deterministic_across_runs() {
        // クラスタリングの再現性はこのプロジェクトの前提（ann.rs と同じ理由）。
        let mut data = Vec::new();
        for p in 0..10 {
            let paper = format!("A{p}");
            for c in 0..4 {
                data.push((paper.clone(), c));
            }
        }
        let params = ContextParams { dim: 4, min_cooccur: 2, ..Default::default() };
        let first = build_context_vectors(4, &data, &params).0;
        let second = build_context_vectors(4, &data, &params).0;
        assert_eq!(first, second);
    }

    #[test]
    fn jacobi_recovers_a_known_symmetric_spectrum() {
        // diag(3,1) を45度回した行列の固有値は 3 と 1。
        let mut a = vec![2.0, 1.0, 1.0, 2.0];
        let (values, _) = jacobi_eigen(&mut a, 2);
        let mut sorted = values.clone();
        sorted.sort_by(|x, y| y.partial_cmp(x).unwrap());
        assert!((sorted[0] - 3.0).abs() < 1e-9, "{sorted:?}");
        assert!((sorted[1] - 1.0).abs() < 1e-9, "{sorted:?}");
    }
}

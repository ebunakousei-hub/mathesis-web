//! 近似最近傍探索（Approximate Nearest Neighbor, ANN）。
//!
//! `graph.rs::build_concept_graph`（クラスタリング用のk近傍グラフ構築）と
//! `search.rs::top_k_by_embedding`（hybrid searchのrelated段階の事前計算）は
//! どちらも「embeddingの全ペアを総当たりでコサイン類似度計算する」O(n²)を
//! 使っていた。両者のコメントが明記していたとおり、候補数nが数千のうちは
//! これで十分速かった（実測: n=2,247で0.5秒）。しかしarXiv 10万〜100万論文
//! 規模になると複合語候補数nも数万〜数十万に達し、n²は非現実的になる
//! （n=10万でn²=100億ペア、n=100万でn²=1兆ペア）。
//!
//! Qdrant等の専用ベクトルDBはこの環境に導入できない（`embed.rs`のコメント
//! 参照——Docker/GPU不在）ため、外部依存を増やさず純粋Rustで実装する。
//! 手法はランダム超平面によるSimHash（コサイン類似度の局所性を保つ標準的な
//! LSH）: 複数の独立なハッシュテーブルでベクトルをバケット化し、
//! いずれか1つのテーブルで同じバケットに落ちたペアだけを候補として厳密な
//! コサイン類似度で採点する。全ペアではなく「同じバケットのペア」だけを
//! 見るため、実質的な計算量はO(n × 平均バケットサイズ × テーブル数)に
//! 収まり、nに対してほぼ線形に近い挙動になる。
//!
//! ハッシュ用の超平面は固定シードの決定的PRNGで生成する——同じ入力からは
//! 常に同じクラスタリング/検索結果が再現されるべきという方針
//! （`quotient.rs`の代表元選択・`alignment.rs`のタイブレークが決定的なのと
//! 同じ理由）で、`std`の乱数やシステム時刻には依存しない。
//!
//! nが小さいとき（`EXACT_FALLBACK_THRESHOLD`未満）はLSHのオーバーヘッドが
//! 総当たりより高くつくうえ、既存の小規模テスト（数件のembeddingで厳密な
//! 近傍を期待する）の挙動を変えないため、素朴な総当たりにフォールバックする。

use crate::embed::cosine_similarity;
use std::collections::{HashMap, HashSet};

/// この件数未満なら総当たり（LSHのバケット化コストが割に合わない規模、かつ
/// 既存の小規模テストが厳密一致を期待しているため挙動を変えない）。
const EXACT_FALLBACK_THRESHOLD: usize = 1500;

/// 独立なハッシュテーブル（バンド）の数。複数持つことで、1つのテーブルで
/// たまたま近傍が別バケットに分かれてしまっても、他のテーブルで拾える
/// 確率を上げる（再現率対策）。
const NUM_TABLES: usize = 4;

/// 1バケットあたりの目標件数。小さすぎると再現率が落ち、大きすぎると
/// バケット内総当たりのコストが増える。
const TARGET_BUCKET_SIZE: usize = 64;

const MIN_BITS: u32 = 6;
/// u32の符号ビット列に収める都合上の上限（2^22バケット、n=100万でも
/// 平均バケットサイズが目標値を下回らない程度の余裕を持たせてある）。
const MAX_BITS: u32 = 22;

/// クラスタリング/検索結果の再現性のための固定シード（意味は無く、
/// 「毎回同じ」であることだけが重要）。
const LSH_SEED: u64 = 0xA5F3_1C9E_4B77_D02D;

/// `n`件のベクトル（`get(i)`で参照を取る、呼び出し側の所有形式に合わせて
/// コピーを強要しないためのアクセサ形式）の全ペアのうち、コサイン類似度が
/// `min_sim`以上のものを`(i, j, sim)`（i<j）として返す。nが小さければ
/// 総当たり、大きければLSHで候補を絞ってから厳密採点する。
pub fn candidate_pairs_above_threshold<'a>(
    n: usize,
    get: impl Fn(usize) -> &'a [f32] + Copy,
    min_sim: f32,
) -> Vec<(usize, usize, f32)> {
    if n < EXACT_FALLBACK_THRESHOLD {
        return brute_force_pairs(n, get, min_sim);
    }
    lsh_candidate_pairs(n, get, min_sim)
}

fn brute_force_pairs<'a>(n: usize, get: impl Fn(usize) -> &'a [f32] + Copy, min_sim: f32) -> Vec<(usize, usize, f32)> {
    let mut out = Vec::new();
    for i in 0..n {
        for j in (i + 1)..n {
            let sim = cosine_similarity(get(i), get(j));
            if sim >= min_sim {
                out.push((i, j, sim));
            }
        }
    }
    out
}

fn bits_for(n: usize) -> u32 {
    if n <= TARGET_BUCKET_SIZE {
        return MIN_BITS;
    }
    let ideal = (n as f64 / TARGET_BUCKET_SIZE as f64).log2().ceil() as i64;
    ideal.clamp(i64::from(MIN_BITS), i64::from(MAX_BITS)) as u32
}

/// splitmix64: 依存を増やさないための最小限の決定的PRNG。ハッシュ用の
/// 超平面という「毎回同じでさえあればよい」用途には十分。
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

    /// おおよそ[-1, 1)の一様乱数。真のガウス分布ではないが、SimHashの
    /// 超平面法線としては実用上十分（多くの実装が一様分布で代用する）。
    fn next_signed_unit(&mut self) -> f32 {
        let bits = self.next_u64();
        (((bits >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0) as f32
    }
}

/// `[num_tables][bits][dim]` の超平面法線ベクトル一式を、固定シードから
/// 決定的に生成する。
fn build_hyperplanes(num_tables: usize, bits: u32, dim: usize) -> Vec<Vec<Vec<f32>>> {
    let mut rng = DeterministicRng::new(LSH_SEED);
    (0..num_tables)
        .map(|_| {
            (0..bits)
                .map(|_| (0..dim).map(|_| rng.next_signed_unit()).collect())
                .collect()
        })
        .collect()
}

fn signature(v: &[f32], hyperplanes: &[Vec<f32>]) -> u32 {
    let mut sig: u32 = 0;
    for (bit, plane) in hyperplanes.iter().enumerate() {
        let dot: f32 = v.iter().zip(plane).map(|(a, b)| a * b).sum();
        if dot >= 0.0 {
            sig |= 1 << bit;
        }
    }
    sig
}

fn lsh_candidate_pairs<'a>(n: usize, get: impl Fn(usize) -> &'a [f32] + Copy, min_sim: f32) -> Vec<(usize, usize, f32)> {
    let dim = (0..n).map(get).find(|v| !v.is_empty()).map_or(0, <[f32]>::len);
    if dim == 0 {
        return Vec::new();
    }
    let bits = bits_for(n);
    let tables = build_hyperplanes(NUM_TABLES, bits, dim);

    let mut candidate_pairs: HashSet<(usize, usize)> = HashSet::new();
    for hyperplanes in &tables {
        let mut buckets: HashMap<u32, Vec<usize>> = HashMap::new();
        for i in 0..n {
            let v = get(i);
            if v.len() != dim {
                continue; // 次元不一致（呼び出し側のバグ検出用、embed.rs::cosine_similarityと同じ扱い）
            }
            buckets.entry(signature(v, hyperplanes)).or_default().push(i);
        }
        for members in buckets.values() {
            for a in 0..members.len() {
                for b in (a + 1)..members.len() {
                    let (i, j) = (members[a], members[b]);
                    candidate_pairs.insert(if i < j { (i, j) } else { (j, i) });
                }
            }
        }
    }

    candidate_pairs
        .into_iter()
        .filter_map(|(i, j)| {
            let sim = cosine_similarity(get(i), get(j));
            (sim >= min_sim).then_some((i, j, sim))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn random_unit_vectors(count: usize, dim: usize, seed: u64) -> Vec<Vec<f32>> {
        let mut rng = DeterministicRng::new(seed);
        (0..count)
            .map(|_| {
                let v: Vec<f32> = (0..dim).map(|_| rng.next_signed_unit()).collect();
                let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
                v.iter().map(|x| x / norm.max(1e-9)).collect()
            })
            .collect()
    }

    /// 既知のクラスタ（同一方向±小さな摂動）+ 大量のノイズ点、という合成
    /// データで、真の近傍ペア（ブルートフォースで求めた正解）のうちLSHが
    /// 何割を再現できるかを検証する。100%の再現は保証しない設計
    /// （それがLSHの本質）だが、実用に足る再現率（90%以上）は必須。
    #[test]
    fn lsh_recovers_most_true_neighbors_in_a_clustered_synthetic_dataset() {
        let dim = 32;
        let mut embeddings = Vec::new();

        // 5つの「概念クラスタ」、各20件が中心方向の近くに密集。
        let centers = random_unit_vectors(5, dim, 1);
        for center in &centers {
            let mut rng = DeterministicRng::new(center.iter().map(|x| x.to_bits() as u64).sum());
            for _ in 0..20 {
                let noisy: Vec<f32> = center.iter().map(|c| c + 0.02 * rng.next_signed_unit()).collect();
                let norm: f32 = noisy.iter().map(|x| x * x).sum::<f32>().sqrt();
                embeddings.push(noisy.iter().map(|x| x / norm.max(1e-9)).collect());
            }
        }
        // ノイズ点（互いにも密集クラスタにも無関係な方向）を足して、
        // ブルートフォースに頼らない規模（EXACT_FALLBACK_THRESHOLD超）にする。
        embeddings.extend(random_unit_vectors(2000, dim, 42));
        let n = embeddings.len();
        let get = |i: usize| embeddings[i].as_slice();

        let min_sim = 0.9;
        let ground_truth: HashSet<(usize, usize)> =
            brute_force_pairs(n, get, min_sim).into_iter().map(|(i, j, _)| (i, j)).collect();
        assert!(ground_truth.len() > 50, "synthetic setup should produce a healthy number of true near-duplicate pairs");

        let found: HashSet<(usize, usize)> =
            lsh_candidate_pairs(n, get, min_sim).into_iter().map(|(i, j, _)| (i, j)).collect();

        let recovered = ground_truth.intersection(&found).count();
        let recall = recovered as f64 / ground_truth.len() as f64;
        assert!(recall >= 0.9, "LSH recall too low: {recall:.3} ({recovered}/{})", ground_truth.len());

        // LSHが返すペアはすべて本物（誤検出はブルートフォースのしきい値
        // フィルタでも消えるはずなので、falseは混じらない）。
        assert!(found.is_subset(&ground_truth));
    }

    #[test]
    fn candidate_pairs_above_threshold_uses_exact_brute_force_below_the_cutoff() {
        let embeddings = [vec![1.0, 0.0], vec![0.99, 0.01], vec![0.0, 1.0]];
        let get = |i: usize| embeddings[i].as_slice();
        let pairs = candidate_pairs_above_threshold(embeddings.len(), get, 0.5);
        assert_eq!(pairs.len(), 1, "only (0,1) exceeds min_sim=0.5 among 3 small vectors");
        assert_eq!((pairs[0].0, pairs[0].1), (0, 1));
    }

    #[test]
    fn bits_for_scales_with_n_but_stays_within_bounds() {
        assert_eq!(bits_for(10), MIN_BITS);
        assert!(bits_for(100_000) > MIN_BITS);
        assert!(bits_for(10_000_000) <= MAX_BITS);
    }

    #[test]
    fn same_seed_always_produces_the_same_hyperplanes() {
        let a = build_hyperplanes(2, 8, 16);
        let b = build_hyperplanes(2, 8, 16);
        assert_eq!(a, b, "hyperplane generation must be fully deterministic for reproducible clustering runs");
    }
}

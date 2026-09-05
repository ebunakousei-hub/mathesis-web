//! クラスタ単位でのMSC2020とのalignment（アーキテクチャ.txt 5.8 Phase 5）。
//!
//! Phase 2の`ground_in_msc`は候補フレーズ1件ずつをMSC名の語彙と文字列
//! 照合するだけだった（10,000論文規模で候補の41%しか一致しなかった）。
//! Phase 4で作ったクラスタは実際に意味的にまとまっていることを確認済み
//! （"schrödinger operators"と"schroedinger equation"が同じクラスタに
//! 入る等）ので、その構造を使って:
//!   1. クラスタ全体としてどのMSC分野に属すると言えるかを、grounded
//!      メンバーの多数決で決める（cluster-level alignment）
//!   2. 確信度が高いクラスタなら、MSC未申告のメンバーにもそのコードを
//!      「推定」として伝播する（Phase 1で自己申告コードが無かった候補を、
//!      MSCの実質的な被覆に含める——5.1の「MSCを拡張・補正する」）
//!   3. grounded メンバーが1件も無いクラスタは、MSC2020に対応物が無い
//!      候補taxonomyノードとして報告する（新規terminologyのクラスタ版）

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct ClusterAlignment {
    pub cluster_id: usize,
    pub size: usize,
    /// MSCコードを持つメンバー数（自己申告 or Phase2のgrounding）
    pub grounded_count: usize,
    pub dominant_code: Option<String>,
    pub dominant_name: Option<String>,
    /// dominant_codeの祖先に一致したgroundedメンバーの割合（grounded_count分の一致数）
    pub confidence: f32,
}

impl ClusterAlignment {
    pub fn is_novel(&self) -> bool {
        self.grounded_count == 0
    }

    /// 伝播や「整合済み」表示に使ってよいと判断する閾値。1件だけの
    /// groundingは偶然の一致でも成立してしまうため2件以上を要求し、
    /// 単純な過半数（ちょうど半々のときは「割れている」とみなす）を求める。
    pub fn is_confident(&self) -> bool {
        self.grounded_count >= 2 && self.confidence > 0.5
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct InferredAlignment {
    pub phrase: String,
    pub cluster_id: usize,
    pub inferred_code: String,
    pub inferred_name: String,
    pub confidence: f32,
}

/// クラスタ1つぶんのalignmentを計算する。`member_codes` はそのクラスタの
/// 各メンバーの自己申告/grounding済みMSCコード（無いメンバーは `None`）。
pub fn align_cluster(cluster_id: usize, size: usize, member_codes: &[Option<String>]) -> ClusterAlignment {
    let grounded: Vec<&str> = member_codes.iter().filter_map(|c| c.as_deref()).collect();
    let grounded_count = grounded.len();

    if grounded_count == 0 {
        return ClusterAlignment {
            cluster_id,
            size,
            grounded_count: 0,
            dominant_code: None,
            dominant_name: None,
            confidence: 0.0,
        };
    }

    // まずセクション単位（3文字、例"18Axx"）の多数決を試す。過半数が
    // 取れなければ、粒度を落としてトップレベル（2桁）で試す——
    // セクションレベルで割れていても、同じ分野の中で割れているだけ
    // ということは多い。
    if let Some((code, name, votes)) = vote_at_level(&grounded, mathesis_msc::MscLevel::Section) {
        let confidence = votes as f32 / grounded_count as f32;
        if confidence > 0.5 {
            return ClusterAlignment { cluster_id, size, grounded_count, dominant_code: Some(code), dominant_name: Some(name), confidence };
        }
    }

    let (code, name, votes) = vote_at_level(&grounded, mathesis_msc::MscLevel::TopLevel)
        .expect("grounded_count > 0 の場合、全MSCコードはトップレベルの祖先を必ず持つ");
    let confidence = votes as f32 / grounded_count as f32;
    ClusterAlignment { cluster_id, size, grounded_count, dominant_code: Some(code), dominant_name: Some(name), confidence }
}

/// `codes` それぞれの `level` における祖先コードを集計し、最多得票の
/// (コード, 名前, 得票数) を返す。同数のときはコード文字列が辞書順で
/// 小さい方を選ぶ（HashMapの走査順に依存しない、決定的な結果にするため）。
fn vote_at_level(codes: &[&str], level: mathesis_msc::MscLevel) -> Option<(String, String, usize)> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    let mut names: HashMap<&str, &str> = HashMap::new();
    for &code in codes {
        if let Some(ancestor) = mathesis_msc::ancestor_chain(code).into_iter().find(|c| c.level == level) {
            *counts.entry(ancestor.code.as_str()).or_default() += 1;
            names.insert(ancestor.code.as_str(), ancestor.name.as_str());
        }
    }
    counts
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(a.0)))
        .map(|(code, votes)| (code.to_string(), names[code].to_string(), votes))
}

/// 確信度の高いクラスタについて、MSC未申告のメンバーへ推定コードを
/// 伝播する。`members` はそのクラスタの (フレーズ, 既存のMSCコード) の一覧。
pub fn propagate(alignment: &ClusterAlignment, members: &[(String, Option<String>)]) -> Vec<InferredAlignment> {
    if !alignment.is_confident() {
        return Vec::new();
    }
    let (Some(code), Some(name)) = (&alignment.dominant_code, &alignment.dominant_name) else {
        return Vec::new();
    };
    members
        .iter()
        .filter(|(_, existing)| existing.is_none())
        .map(|(phrase, _)| InferredAlignment {
            phrase: phrase.clone(),
            cluster_id: alignment.cluster_id,
            inferred_code: code.clone(),
            inferred_name: name.clone(),
            confidence: alignment.confidence,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // 実データで存在を確認済みのMSC2020コード（crates/mathesis-msc参照）:
    // 18A05/18A25 はどちらもセクション18Axx、18B05はセクション18Bxx、
    // いずれもトップレベル18-XX。11A05はトップレベル11-XXでセクション11Axx。

    #[test]
    fn cluster_with_no_grounded_members_is_novel() {
        let a = align_cluster(0, 3, &[None, None, None]);
        assert!(a.is_novel());
        assert!(!a.is_confident());
        assert_eq!(a.dominant_code, None);
    }

    #[test]
    fn cluster_with_agreeing_section_level_codes_aligns_at_section_granularity() {
        let codes = vec![Some("18A05".to_string()), Some("18A25".to_string())];
        let a = align_cluster(1, 2, &codes);
        assert_eq!(a.dominant_code.as_deref(), Some("18Axx"));
        assert_eq!(a.confidence, 1.0);
        assert!(a.is_confident());
        assert!(!a.is_novel());
    }

    #[test]
    fn cluster_split_across_sections_falls_back_to_shared_top_level() {
        // 18A05と18B05はセクションが違う（18Axx vs 18Bxx、1票ずつで多数決不成立）
        // が、トップレベルはどちらも18-XXで一致する。
        let codes = vec![Some("18A05".to_string()), Some("18B05".to_string())];
        let a = align_cluster(2, 2, &codes);
        assert_eq!(a.dominant_code.as_deref(), Some("18-XX"));
        assert_eq!(a.confidence, 1.0);
        assert!(a.is_confident());
    }

    #[test]
    fn cluster_split_across_unrelated_top_level_fields_is_not_confident() {
        let codes = vec![Some("18A05".to_string()), Some("11A05".to_string())];
        let a = align_cluster(3, 2, &codes);
        assert_eq!(a.grounded_count, 2);
        assert_eq!(a.confidence, 0.5, "a coin-flip split must not count as a majority");
        assert!(!a.is_confident(), "0.5 confidence with only 2 grounded members must not be treated as confident");
    }

    #[test]
    fn a_single_grounded_member_is_not_enough_to_be_confident() {
        // 偶然の一致1件だけでクラスタ全体の分野を決め打ちしない。
        let codes = vec![Some("18A05".to_string()), None, None];
        let a = align_cluster(4, 3, &codes);
        assert_eq!(a.grounded_count, 1);
        assert_eq!(a.confidence, 1.0, "the single grounded member trivially agrees with itself");
        assert!(!a.is_confident(), "confidence alone isn't enough — need at least 2 grounded members");
    }

    #[test]
    fn propagate_assigns_the_dominant_code_only_to_ungrounded_members() {
        let alignment = align_cluster(5, 3, &[Some("18A05".to_string()), Some("18A25".to_string()), None]);
        let members = vec![
            ("definitions and generalizations".to_string(), Some("18A05".to_string())),
            ("functor categories".to_string(), Some("18A25".to_string())),
            ("some ungrounded term".to_string(), None),
        ];
        let inferred = propagate(&alignment, &members);
        assert_eq!(inferred.len(), 1, "only the ungrounded member should receive an inferred code");
        assert_eq!(inferred[0].phrase, "some ungrounded term");
        assert_eq!(inferred[0].inferred_code, "18Axx");
        assert_eq!(inferred[0].confidence, 1.0);
    }

    #[test]
    fn propagate_does_nothing_for_a_non_confident_cluster() {
        let alignment = align_cluster(6, 3, &[Some("18A05".to_string()), Some("11A05".to_string()), None]);
        let members = vec![
            ("x".to_string(), Some("18A05".to_string())),
            ("y".to_string(), Some("11A05".to_string())),
            ("z".to_string(), None),
        ];
        assert!(propagate(&alignment, &members).is_empty());
    }

    #[test]
    fn propagate_does_nothing_for_a_novel_cluster() {
        let alignment = align_cluster(7, 2, &[None, None]);
        let members = vec![("x".to_string(), None), ("y".to_string(), None)];
        assert!(propagate(&alignment, &members).is_empty());
    }
}

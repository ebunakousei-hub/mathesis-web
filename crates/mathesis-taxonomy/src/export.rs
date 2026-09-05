//! Phase 1〜5のパイプライン出力を、Web版Explorer（アーキテクチャ.txt 5.7
//! Phase 6）が読み込める静的JSONへ書き出す。Phase 7では、ブラウザ側で
//! exact / same concept / specialization / related のhybrid searchが
//! 動くよう `searchIndex`（全候補のフラットな索引）と `relatedEdges`
//! （embedding近傍の事前計算済み辺リスト）も同梱する。
//!
//! `web/src/fields.ts` は手書きの5分野・学際領域という静的taxonomyだった。
//! ここではその代わりに、実際にMSC2020へconfidentに整合したクラスタを
//! MSCトップレベル分野ごとにまとめた「fields」と、MSC2020に対応物が無い
//! クラスタ（新語彙候補）を「novelClusters」として書き出す。MSC名は
//! 公式データが英語のみのため、動的セクションのラベルは常に英語になる
//! （日本語訳データを持たないため、誤訳をでっち上げるより素直にそうする）。
//!
//! `searchIndex` は fields/novelClusters に載らなかった候補（確信度不足の
//! ambiguousクラスタや孤立候補）も含む全件——検索は「表示に足る」条件とは
//! 別物なので、絞り込まずに索引化する。`relatedEdges` はembeddingベクトル
//! 本体ではなく近傍上位k件のフレーズ+スコアだけを載せる。ブラウザに
//! 5千件超の384次元ベクトルを丸ごと持たせるとJSONが数十MBに膨らむため
//! （実測前だが1件あたり384*4バイト×候補数で見積もれば明らかに過大）、
//! Rust側で`search::top_k_by_embedding`により事前計算した辺だけを渡す。

use crate::alignment::ClusterAlignment;
use crate::concepts::ConceptCandidate;
use crate::resolve;
use serde::Serialize;
use std::collections::HashMap;

/// 書き出す小数の桁数。
///
/// 類似度スコアも確信度も分野集中度も、表示は小数2桁、比較に使うのも
/// せいぜい3桁で足りる。にもかかわらず`f32`をそのまま直列化すると
/// `0.24858053` のような8〜9桁が出る。この差は100k論文規模のJSONで
/// 実測1.5MB分あり、`fetch`とブラウザの`JSON.parse`の両方に効く
/// （どちらも非圧縮のバイト数に比例する）。
fn round3(value: f32) -> f32 {
    (value * 1000.0).round() / 1000.0
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedMember {
    pub phrase: String,
    pub doc_freq: usize,
    pub msc_code: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedCluster {
    pub id: usize,
    pub size: usize,
    pub confidence: f32,
    pub dominant_code: Option<String>,
    pub dominant_name: Option<String>,
    pub members: Vec<ExportedMember>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedField {
    pub code: String,
    pub name: String,
    pub cluster_count: usize,
    pub concept_count: usize,
    pub clusters: Vec<ExportedCluster>,
}

/// 検索索引。1件ごとのオブジェクトではなく**列**で持つ。
///
/// `[{"phrase":..,"docFreq":..,"mscCode":..,"clusterId":..,
/// "fieldConcentration":..}, …]` という素直な形だと、5つのキー名が
/// 候補の数だけ繰り返される。100k論文規模の実データ（80,727候補）では
/// **キー名の繰り返しだけで4.9MB**——この区画7.81MBの63%がフィールド名の
/// 文字列だった。列に転置するだけで 7.81MB → 2.88MB になる。
///
/// これは人が読むための構造ではなくブラウザへの配信物なので、可読性より
/// 転送量とパース時間を採る（読みたいときは `jq` を通せばよい）。
/// 各Vecは同じ長さで、同じ添字が同じ候補を指す。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchIndexColumns {
    pub phrase: Vec<String>,
    pub doc_freq: Vec<usize>,
    pub msc_code: Vec<Option<String>>,
    pub cluster_id: Vec<Option<usize>>,
    /// 分野集中度（`concentration.rs`）。ブラウザ側で「この語がどれだけ
    /// 特定分野に固有か」を出すために載せる。クラスタのメンバー一覧
    /// （`ExportedMember`）には載せない——同じ数字が概念1件につき複数回
    /// 出ることになり、JSONを無駄に太らせるだけなので、候補1件につき
    /// 1回だけ現れるこの索引側に置く。
    pub field_concentration: Vec<Option<f32>>,
}

/// `searchIndex`のうち、文書頻度上位`HEAD_SHARD_SIZE`件だけを抜き出した
/// 「ヘッドシャード」（診断④「配信の不可分性」への対応）。
///
/// `searchIndex`本体（実データ80,727件・9.0MB）はJSON.parseと索引構築
/// （転置索引・IDF計算、`web/src/queryIndex.ts::buildConceptSearchIndex`）
/// が合わせて実測632ms かかり、これがメインスレッドを塞いでいた——問題は
/// バイト数ではなく**不可分性**（全件が終わるまで1件も検索に答えられない
/// こと）。ブラウザ側はこの小さなヘッドシャードを最初に同期的に読み込み、
/// **フルの索引がWorker上で出来上がるまでの間の即答**に使う
/// （`web/src/searchWorker.ts`参照）。文書頻度上位の概念は検索されやすい
/// 語でもあるはずなので、この小さな部分集合だけでも多くのクエリに
/// その場で答えられる。
pub const HEAD_SHARD_SIZE: usize = 800;

/// `columns`と全く同じ生成規則（`build_export`の`search_index`）で作った
/// 索引の**先頭部分の複製**であり、別のデータではない。文書頻度で降順に
/// 並べ替えてから`HEAD_SHARD_SIZE`件に切るだけ。同点は元の並び順を保つ
/// （`sort_by_key`は安定ソート）。
pub fn build_head_shard(columns: &SearchIndexColumns) -> SearchIndexColumns {
    let mut order: Vec<usize> = (0..columns.phrase.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(columns.doc_freq[i]));
    order.truncate(HEAD_SHARD_SIZE);
    SearchIndexColumns {
        phrase: order.iter().map(|&i| columns.phrase[i].clone()).collect(),
        doc_freq: order.iter().map(|&i| columns.doc_freq[i]).collect(),
        msc_code: order.iter().map(|&i| columns.msc_code[i].clone()).collect(),
        cluster_id: order.iter().map(|&i| columns.cluster_id[i]).collect(),
        field_concentration: order.iter().map(|&i| columns.field_concentration[i]).collect(),
    }
}

/// embedding近傍の辺リスト。**別ファイル**に分けて書き出す。
///
/// 実データではこれだけで16.95MB——`taxonomy.json`全体27.18MBの62%を
/// 占めていた。しかもこれが必要になるのは、利用者が実際に検索して
/// 「関連概念」段階を見るときだけで、分野カードを眺めるだけの利用者や
/// ページを開いただけの利用者には一切要らない。同じファイルに入れて
/// いる限り、全利用者が毎回この62%をダウンロードし終えるまで
/// 概念エクスプローラーが一切動かない。
///
/// こちらも列形式（10.57MB）。`source[i]` の近傍が
/// `targets[i]` / `scores[i]`。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedEdgesExport {
    pub source: Vec<String>,
    pub targets: Vec<Vec<String>>,
    pub scores: Vec<Vec<f32>>,
}

/// 概念の出典論文。**タイトル付きの文書オブジェクトとして**出す。
///
/// 以前はここが `arxivIds: Vec<Vec<String>>`——概念ごとに生のarXiv ID
/// 最大3件だけ——だった。つまりWeb版のバンドルには**論文オブジェクトが
/// 1件も入っていなかった**: タイトルも投稿年も分類も無く、利用者は
/// "0905.3137" という文字列を渡されてサイトの外へ出るしかなかった。
/// 検索結果の単位が「文書」ではなく「フレーズ」だったということで、
/// これが検索エンジンとしての体裁との最大の距離だった
/// （`concepts.rs::ConceptCandidate::sample_arxiv_ids` は元々
///  「目視確認用」の3件サンプルで、UIの土台に使える設計ではない）。
///
/// 論文本体は列形式で1回だけ持ち、概念からは**添字**で参照する
/// ——同じ論文が多数の概念に出るので、IDや題名を概念ごとに繰り返すと
/// 配信量が跳ね上がる。`source[i]` の論文が `papers[i]`（`arxivId` 等の
/// 配列への添字）。
///
/// `RelatedEdgesExport`と同じく、利用者が実際に検索するまで要らない
/// データなので別ファイルへ分け、ブラウザは遅延読み込みする。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PapersExport {
    pub arxiv_id: Vec<String>,
    pub title: Vec<String>,
    pub year: Vec<Option<u16>>,
    pub primary_category: Vec<Option<String>>,
    pub source: Vec<String>,
    pub papers: Vec<Vec<u32>>,
}

/// 論文1件の、この書き出しに必要な情報（`store::PaperMeta` と同じ内容を
/// export側の型として受ける——`export` が `store` に依存しないようにする
/// ため、既存の他の `build_*` と同じ方針）。
#[derive(Debug, Clone)]
pub struct PaperRow {
    pub arxiv_id: String,
    pub title: String,
    pub year: Option<u16>,
    pub primary_category: Option<String>,
}

/// 概念ごとに載せる論文の上限。
///
/// `paper_concepts` は実データで186万本あり、全部載せると配信量が
/// 桁違いになる。一方で利用者が1つの概念について見るのはせいぜい
/// 十数件なので、概念ごとに上位N件へ切る。
pub const MAX_PAPERS_PER_CONCEPT: usize = 12;

/// 概念→論文の並び順を決める。
///
/// 引用数を持っていないので「重要な論文」は判定できない。代わりに、
/// **確実に言えること**だけを使う:
///
///   1. その概念が論文の**題名**に現れるか。現れるなら、その論文が
///      その概念について書かれていることは疑いようがない。
///   2. 同点なら、抽出概念数の少ない論文を先に。概念が少ない論文ほど、
///      その1件が論文の主題に占める割合が大きい。
///   3. それも同点ならarXiv ID順（実行ごとに順序が変わらないように）。
///
/// これは重要度の推定ではなく**関連の強さの推定**であり、そう名乗る。
/// 引用グラフに基づく順位付けは、引用データを持ってから。
fn paper_rank_key(title_matches: bool, concept_count: usize, arxiv_id: &str) -> (bool, usize, String) {
    (!title_matches, concept_count, arxiv_id.to_string())
}

/// `links` は (arxiv_id, phrase) の全ペア。`papers` は論文メタデータ。
/// `resolved[g].members`は`candidates`への添字（`build_export`と同じ
/// 制約——呼び出し側`run_export`参照）。
pub fn build_papers_export(
    resolved: &[resolve::ResolvedConcept],
    candidates: &[ConceptCandidate],
    papers: &[PaperRow],
    links: &[(String, String)],
) -> PapersExport {
    let paper_index: HashMap<&str, usize> =
        papers.iter().enumerate().map(|(i, p)| (p.arxiv_id.as_str(), i)).collect();
    let title_lower: Vec<String> = papers.iter().map(|p| p.title.to_lowercase()).collect();

    let mut concept_count: HashMap<&str, usize> = HashMap::new();
    for (arxiv_id, _) in links {
        *concept_count.entry(arxiv_id.as_str()).or_default() += 1;
    }

    let mut by_phrase: HashMap<&str, Vec<usize>> = HashMap::new();
    for (arxiv_id, phrase) in links {
        if let Some(&pi) = paper_index.get(arxiv_id.as_str()) {
            by_phrase.entry(phrase.as_str()).or_default().push(pi);
        }
    }

    // 実際に参照された論文だけを書き出す（参照されない論文の題名を
    // 配信物に含める理由が無い）。添字は詰め直す。
    //
    // 表記ゆれのどのメンバーで論文が来ても、代表表記1件へ集約する——
    // 以前は`candidates`を単位にしていたため、"kahler manifold"で
    // 言及した論文と"kahler manifolds"で言及した論文が別々のエントリに
    // 分かれていた。`search_index`側を代表表記1行に畳んだ以上、その行の
    // 「出典論文」がどちらか片方の綴りだけに限られると、別表記で言及した
    // 論文が画面から消えてしまう。
    let mut used: Vec<bool> = vec![false; papers.len()];
    let mut source = Vec::new();
    let mut selected: Vec<Vec<usize>> = Vec::new();
    for group in resolved {
        let mut paper_indices: Vec<usize> = group
            .members
            .iter()
            .filter_map(|&m| by_phrase.get(candidates[m].phrase.as_str()))
            .flatten()
            .copied()
            .collect();
        if paper_indices.is_empty() {
            continue;
        }
        paper_indices.sort_unstable();
        paper_indices.dedup();
        let needle = group.representative.to_lowercase();
        let mut ranked = paper_indices;
        ranked.sort_by_key(|&pi| {
            paper_rank_key(
                title_lower[pi].contains(&needle),
                concept_count.get(papers[pi].arxiv_id.as_str()).copied().unwrap_or(usize::MAX),
                &papers[pi].arxiv_id,
            )
        });
        ranked.truncate(MAX_PAPERS_PER_CONCEPT);
        for &pi in &ranked {
            used[pi] = true;
        }
        source.push(group.representative.clone());
        selected.push(ranked);
    }

    let mut remap = vec![u32::MAX; papers.len()];
    let mut arxiv_id = Vec::new();
    let mut title = Vec::new();
    let mut year = Vec::new();
    let mut primary_category = Vec::new();
    for (i, keep) in used.iter().enumerate() {
        if *keep {
            remap[i] = arxiv_id.len() as u32;
            arxiv_id.push(papers[i].arxiv_id.clone());
            title.push(papers[i].title.clone());
            year.push(papers[i].year);
            primary_category.push(papers[i].primary_category.clone());
        }
    }

    let papers_out = selected
        .into_iter()
        .map(|list| list.into_iter().map(|pi| remap[pi]).collect())
        .collect();

    PapersExport { arxiv_id, title, year, primary_category, source, papers: papers_out }
}

/// 表記ゆれの一覧（`resolve.rs` の出力）。代表表記 → その別表記。
///
/// クラスタリングは解決済みの概念に対して行うので、別表記は同じ
/// クラスタIDを持つ。それでも一覧を別に出すのは、Web側が
/// 「elliptic curves（他3つの表記）」のように**畳んだ事実そのものを
/// 見せられる**ようにするため——黙って畳むと、利用者は自分が打った
/// 表記が結果に出てこない理由が分からない（同じ理由で綴り訂正も
/// 画面に出している）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AliasExport {
    pub representative: Vec<String>,
    pub aliases: Vec<Vec<String>>,
}

/// 型付き関係（`relations.rs`、アーキテクチャ.txt 5.3の`Relation`）。
///
/// **根拠文を持つものだけを書き出す**——`Confirmed`（分布的統計と本文の
/// 一文が一致）と`Grounded`（本文の一文のみ）。`Proposed`（統計のみ、
/// 根拠文なし）は実データで103,096件と桁違いに多く、しかも読者が
/// 自分で真偽を確かめる手段（根拠文）を持たない。実際にConfirmedの
/// 質を人手でサンプル確認したところ、正しかったのは当初約36%
/// （84件中30件）。的を絞った表層パターン修正を3回重ねた後の実測では
/// 約50%（38件中19件、2026-09-03、`relations.rs`冒頭の既知の限界を
/// 参照）——2倍にはなったが依然として半数近くが誤りであることに変わりは
/// なく、`status`は「両経路が一致した」以上の確実性を主張していない。
/// この精度で
/// 根拠文の無い10万件超を検索結果に混ぜるのは、読者が真偽を検証する
/// 手立てを持たないまま統計的な当て推量を事実として見せることになり、
/// 「実データ・作り話ゼロ」の原則に反する。根拠文があれば、読者は
/// 提示された一文を読んでその場で判断できる——これが線引きの理由。
///
/// `Proposed`は`concept_candidates.db`側には保存されており、将来
/// より精度の高い判定手法（例えば根拠文の統語解析）が実装された際に
/// 再利用できる。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationsExport {
    pub subject: Vec<String>,
    pub object: Vec<String>,
    /// "specialization_of" | "equivalent_to"
    pub kind: Vec<String>,
    /// "confirmed" | "grounded"（`proposed`はここには出さない、上記参照）。
    pub status: Vec<String>,
    pub confidence: Vec<f32>,
    pub evidence_sentence: Vec<String>,
    pub evidence_arxiv_id: Vec<String>,
}

pub fn build_relations_export(edges: &[crate::relations::RelationEdge]) -> RelationsExport {
    use crate::relations::{RelationKind, RelationStatus};

    let mut with_evidence: Vec<&crate::relations::RelationEdge> = edges
        .iter()
        .filter(|e| matches!(e.status, RelationStatus::Confirmed | RelationStatus::Grounded))
        .collect();
    // Confirmedを先に、同じstatus内は確信度の降順——利用者が最初に見る
    // ものが最も裏付けの強いものになるように（`extract`の除外リストが
    // 集中度の低い順に並ぶのと同じ「見せる順序にも意味を持たせる」方針）。
    with_evidence.sort_by(|a, b| {
        let rank = |s: RelationStatus| if s == RelationStatus::Confirmed { 0 } else { 1 };
        rank(a.status)
            .cmp(&rank(b.status))
            .then_with(|| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| a.subject.cmp(&b.subject))
    });

    let mut out = RelationsExport {
        subject: Vec::with_capacity(with_evidence.len()),
        object: Vec::with_capacity(with_evidence.len()),
        kind: Vec::with_capacity(with_evidence.len()),
        status: Vec::with_capacity(with_evidence.len()),
        confidence: Vec::with_capacity(with_evidence.len()),
        evidence_sentence: Vec::with_capacity(with_evidence.len()),
        evidence_arxiv_id: Vec::with_capacity(with_evidence.len()),
    };
    for e in with_evidence {
        out.subject.push(e.subject.clone());
        out.object.push(e.object.clone());
        out.kind.push(match e.kind {
            RelationKind::SpecializationOf => "specialization_of".to_string(),
            RelationKind::EquivalentTo => "equivalent_to".to_string(),
        });
        out.status.push(match e.status {
            RelationStatus::Confirmed => "confirmed".to_string(),
            RelationStatus::Grounded => "grounded".to_string(),
            RelationStatus::Proposed => unreachable!("Proposedはフィルタ済み"),
        });
        out.confidence.push(round3(e.confidence));
        out.evidence_sentence.push(e.evidence_sentence.clone().unwrap_or_default());
        out.evidence_arxiv_id.push(e.evidence_arxiv_id.clone().unwrap_or_default());
    }
    out
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaxonomyExport {
    /// このJSONを`export`コマンドが書き出した時刻（UNIX秒）。外部レビュー
    /// （2026-09-05）で「データの生成日時が画面に見えない」と指摘されたため
    /// 追加した——日付への変換はUI側（JSの`Date`）に任せ、Rust側は新規の
    /// 日時クレートを増やさずに済ませる。
    pub generated_at_unix: u64,
    pub paper_count: usize,
    pub candidate_count: usize,
    /// Entity Resolution（`resolve.rs`）で表記ゆれを畳んだ後の、実際に
    /// 区別できる概念の数。`candidate_count`との差が「畳んだ表記ゆれの
    /// 延べ件数」——`search_index`の行数は`candidate_count`ではなく
    /// こちらに一致する。
    pub resolved_concept_count: usize,
    pub cluster_count: usize,
    pub ambiguous_cluster_count: usize,
    pub fields: Vec<ExportedField>,
    pub novel_clusters: Vec<ExportedCluster>,
    pub search_index: SearchIndexColumns,
}

/// `resolved[g].members`は`candidates`への添字なので、`resolved`は
/// 必ず`resolve::resolve(candidatesのphrase, candidatesのdoc_freq)`を
/// **候補の並び順そのままで**呼んだ結果を渡すこと（呼び出し側
/// `run_export`参照）。
pub fn build_export(
    paper_count: usize,
    candidates: &[ConceptCandidate],
    resolved: &[resolve::ResolvedConcept],
    alignments: &[ClusterAlignment],
    members_by_cluster: &HashMap<usize, Vec<String>>,
) -> TaxonomyExport {
    let doc_freq_by_phrase: HashMap<&str, usize> =
        candidates.iter().map(|c| (c.phrase.as_str(), c.doc_freq)).collect();
    let msc_by_phrase: HashMap<&str, Option<&str>> =
        candidates.iter().map(|c| (c.phrase.as_str(), c.msc_code.as_deref())).collect();

    let mut cluster_of: HashMap<&str, usize> = HashMap::new();
    for (&cluster_id, phrases) in members_by_cluster {
        for phrase in phrases {
            cluster_of.insert(phrase.as_str(), cluster_id);
        }
    }

    let to_cluster = |a: &ClusterAlignment| -> ExportedCluster {
        let mut members: Vec<ExportedMember> = members_by_cluster
            .get(&a.cluster_id)
            .into_iter()
            .flatten()
            .map(|phrase| ExportedMember {
                doc_freq: doc_freq_by_phrase.get(phrase.as_str()).copied().unwrap_or(0),
                msc_code: msc_by_phrase.get(phrase.as_str()).copied().flatten().map(str::to_string),
                phrase: phrase.clone(),
            })
            .collect();
        members.sort_by_key(|m| std::cmp::Reverse(m.doc_freq));
        ExportedCluster {
            id: a.cluster_id,
            size: a.size,
            confidence: round3(a.confidence),
            dominant_code: a.dominant_code.clone(),
            dominant_name: a.dominant_name.clone(),
            members,
        }
    };

    // confidentなクラスタを、支配的コードのMSCトップレベル祖先ごとにまとめる。
    let mut by_field: HashMap<String, Vec<&ClusterAlignment>> = HashMap::new();
    for a in alignments.iter().filter(|a| a.is_confident()) {
        if let Some(code) = &a.dominant_code {
            if let Some(top) = mathesis_msc::ancestor_chain(code).first() {
                by_field.entry(top.code.clone()).or_default().push(a);
            }
        }
    }

    let mut fields: Vec<ExportedField> = by_field
        .into_iter()
        .map(|(top_code, aligns)| {
            let name = mathesis_msc::by_code(&top_code).map(|c| c.name.clone()).unwrap_or_default();
            let mut clusters: Vec<ExportedCluster> = aligns.iter().map(|&a| to_cluster(a)).collect();
            clusters.sort_by_key(|c| std::cmp::Reverse(c.size));
            let concept_count: usize = clusters.iter().map(|c| c.size).sum();
            ExportedField {
                code: top_code,
                name,
                cluster_count: clusters.len(),
                concept_count,
                clusters,
            }
        })
        .collect();
    fields.sort_by_key(|f| std::cmp::Reverse(f.concept_count));

    let mut novel_clusters: Vec<ExportedCluster> =
        alignments.iter().filter(|a| a.is_novel() && a.size > 1).map(to_cluster).collect();
    novel_clusters.sort_by_key(|c| std::cmp::Reverse(c.size));

    let ambiguous_cluster_count = alignments.iter().filter(|a| !a.is_confident() && !a.is_novel()).count();

    // 表記ゆれを1件の解決済み概念として畳んだ行を単位にする。以前は
    // ここが`candidates`（解決前の生候補）をそのまま列にしていたため、
    // "kahler manifold"/"kahler manifolds"のような表記ゆれが検索結果に
    // 別々の行として残り続けていた——クラスタリング・関係抽出はとっくに
    // 解決済み概念を単位にしていた（`resolve_and_pool`）のに、利用者が
    // 実際に触る検索索引だけが取り残されていた。
    let search_index = {
        let mut phrase = Vec::with_capacity(resolved.len());
        let mut doc_freq = Vec::with_capacity(resolved.len());
        let mut msc_code = Vec::with_capacity(resolved.len());
        let mut cluster_id = Vec::with_capacity(resolved.len());
        let mut field_concentration = Vec::with_capacity(resolved.len());
        for group in resolved {
            phrase.push(group.representative.clone());
            doc_freq.push(group.doc_freq_sum);
            // MSC・分野集中度は代表表記を優先し、無ければ他のメンバーから
            // 拾う（`resolve_and_pool`の同じ方針——`members`は代表が
            // 先頭の決定的な順序）。
            msc_code.push(group.members.iter().find_map(|&m| candidates[m].msc_code.clone()));
            field_concentration
                .push(group.members.iter().find_map(|&m| candidates[m].field_concentration).map(round3));
            cluster_id.push(cluster_of.get(group.representative.as_str()).copied());
        }
        SearchIndexColumns { phrase, doc_freq, msc_code, cluster_id, field_concentration }
    };

    TaxonomyExport {
        generated_at_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        paper_count,
        candidate_count: candidates.len(),
        resolved_concept_count: resolved.len(),
        cluster_count: alignments.len(),
        ambiguous_cluster_count,
        fields,
        novel_clusters,
        search_index,
    }
}

/// 近傍の辺リストを、`taxonomy.json` とは別のファイルへ書き出す形にまとめる。
/// 出力順はフレーズ名でソートする——同じDBからは常に同じバイト列が出る方が、
/// 差分の確認やキャッシュの扱いで扱いやすい（`HashMap` の反復順は不定）。
pub fn build_related_export(neighbors: &HashMap<String, Vec<(String, f32)>>) -> RelatedEdgesExport {
    let mut sources: Vec<&String> = neighbors
        .iter()
        .filter(|(_, edges)| !edges.is_empty())
        .map(|(phrase, _)| phrase)
        .collect();
    sources.sort_unstable();

    let mut source = Vec::with_capacity(sources.len());
    let mut targets = Vec::with_capacity(sources.len());
    let mut scores = Vec::with_capacity(sources.len());
    for phrase in sources {
        let edges = &neighbors[phrase];
        source.push(phrase.clone());
        targets.push(edges.iter().map(|(n, _)| n.clone()).collect());
        scores.push(edges.iter().map(|(_, s)| round3(*s)).collect());
    }
    RelatedEdgesExport { source, targets, scores }
}

/// 概念ごとの出典arXiv IDを、別ファイル用にまとめる。出力順はフレーズ名で
/// ソートする——`build_related_export`と同じ理由（同じDBからは常に同じ
/// バイト列が出る方が差分確認・キャッシュの扱いで有利）。
/// `sample_arxiv_ids` が空の候補（理論上あり得るが実データでは0件——
/// `extract_candidates`は出現した論文のうち最大3件を必ず記録する）は
/// スキップする。
pub fn build_alias_export(groups: &[(String, Vec<String>)]) -> AliasExport {
    let mut with_aliases: Vec<&(String, Vec<String>)> = groups.iter().filter(|(_, a)| !a.is_empty()).collect();
    with_aliases.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    AliasExport {
        representative: with_aliases.iter().map(|(r, _)| r.clone()).collect(),
        aliases: with_aliases.iter().map(|(_, a)| a.clone()).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(phrase: &str, doc_freq: usize, msc_code: Option<&str>) -> ConceptCandidate {
        ConceptCandidate {
            phrase: phrase.to_string(),
            word_count: phrase.split(' ').count(),
            doc_freq,
            mean_score: 0.0,
            msc_code: msc_code.map(str::to_string),
            sample_arxiv_ids: vec![],
            field_concentration: None,
        }
    }

    /// `run_export`と同じ規約（`candidates`と同じ並び順・フィルタ無し）で
    /// Entity Resolutionを掛ける、テスト専用のヘルパー。
    fn resolve_all(candidates: &[ConceptCandidate]) -> Vec<resolve::ResolvedConcept> {
        let phrases: Vec<String> = candidates.iter().map(|c| c.phrase.clone()).collect();
        let doc_freqs: Vec<usize> = candidates.iter().map(|c| c.doc_freq).collect();
        resolve::resolve(&phrases, &doc_freqs)
    }

    #[test]
    fn groups_confident_clusters_by_msc_top_level_field() {
        // 18A05/18A25 は実データで確認済み: どちらもトップレベル18-XX。
        let candidates = vec![
            candidate("definitions and generalizations", 10, Some("18A05")),
            candidate("functor categories", 8, Some("18A25")),
        ];
        let alignment = crate::alignment::align_cluster(0, 2, &[Some("18A05".to_string()), Some("18A25".to_string())]);
        let mut members_by_cluster = HashMap::new();
        members_by_cluster.insert(0, vec!["definitions and generalizations".to_string(), "functor categories".to_string()]);

        let resolved = resolve_all(&candidates);
        let export = build_export(100, &candidates, &resolved, std::slice::from_ref(&alignment), &members_by_cluster);

        assert_eq!(export.fields.len(), 1);
        assert_eq!(export.fields[0].code, "18-XX");
        assert_eq!(export.fields[0].clusters.len(), 1);
        assert_eq!(export.fields[0].clusters[0].members.len(), 2);
        // doc_freq降順（10が先）
        assert_eq!(export.fields[0].clusters[0].members[0].phrase, "definitions and generalizations");
        assert!(export.novel_clusters.is_empty());
    }

    #[test]
    fn novel_clusters_with_no_grounding_are_listed_separately_from_fields() {
        let candidates = vec![candidate("some new term", 5, None), candidate("another new term", 4, None)];
        let alignment = crate::alignment::align_cluster(1, 2, &[None, None]);
        let mut members_by_cluster = HashMap::new();
        members_by_cluster.insert(1, vec!["some new term".to_string(), "another new term".to_string()]);

        let resolved = resolve_all(&candidates);
        let export = build_export(100, &candidates, &resolved, std::slice::from_ref(&alignment), &members_by_cluster);

        assert!(export.fields.is_empty());
        assert_eq!(export.novel_clusters.len(), 1);
        assert_eq!(export.novel_clusters[0].members.len(), 2);
    }

    #[test]
    fn singleton_novel_clusters_are_excluded_as_too_thin_to_be_interesting() {
        let candidates = vec![candidate("lonely term", 5, None)];
        let alignment = crate::alignment::align_cluster(2, 1, &[None]);
        let mut members_by_cluster = HashMap::new();
        members_by_cluster.insert(2, vec!["lonely term".to_string()]);

        let resolved = resolve_all(&candidates);
        let export = build_export(100, &candidates, &resolved, std::slice::from_ref(&alignment), &members_by_cluster);
        assert!(export.novel_clusters.is_empty());
    }

    #[test]
    fn search_index_includes_every_candidate_even_ambiguous_or_singleton_ones() {
        // 18A05はfields、"lonely term"は novel_clusters にすら載らない孤立候補——
        // それでも search_index には両方出てくるべき（検索は表示条件と独立）。
        let candidates = vec![candidate("definitions and generalizations", 10, Some("18A05")), candidate("lonely term", 5, None)];
        let alignment_a = crate::alignment::align_cluster(0, 1, &[Some("18A05".to_string())]);
        let alignment_b = crate::alignment::align_cluster(1, 1, &[None]);
        let mut members_by_cluster = HashMap::new();
        members_by_cluster.insert(0, vec!["definitions and generalizations".to_string()]);
        members_by_cluster.insert(1, vec!["lonely term".to_string()]);

        let resolved = resolve_all(&candidates);
        let export = build_export(100, &candidates, &resolved, &[alignment_a, alignment_b], &members_by_cluster);

        let index = &export.search_index;
        assert_eq!(index.phrase.len(), 2);
        // 列形式なので、同じ添字が同じ候補を指すことも合わせて確認する。
        let lonely = index.phrase.iter().position(|p| p == "lonely term").unwrap();
        assert_eq!(index.cluster_id[lonely], Some(1));
        assert_eq!(index.msc_code[lonely], None);
        assert_eq!(index.doc_freq[lonely], 5);
        assert_eq!(index.doc_freq.len(), 2);
        assert_eq!(index.field_concentration.len(), 2);
    }

    #[test]
    fn search_index_folds_spelling_variants_into_a_single_row() {
        // 実データで確認された欠陥の再現: "kahler manifold"と
        // "kahler manifolds"のような表記ゆれが、Entity Resolution
        // （`resolve.rs`）を通したのに検索索引には別々の行として
        // 残っていた——クラスタリング等は既に解決済み概念を単位に
        // していたのに、検索索引だけが生候補のままだった。
        let candidates = vec![
            candidate("kahler manifolds", 30, Some("32Q15")),
            candidate("kahler manifold", 70, Some("32Q15")),
            candidate("ricci flow", 15, None),
        ];
        let resolved = resolve_all(&candidates);
        let alignment = crate::alignment::align_cluster(0, 2, &[Some("32Q15".to_string()), None]);
        let mut members_by_cluster = HashMap::new();
        // クラスタリングは代表表記だけを単位にする（既存の挙動）。
        members_by_cluster.insert(0, vec!["kahler manifold".to_string(), "ricci flow".to_string()]);

        let export = build_export(100, &candidates, &resolved, std::slice::from_ref(&alignment), &members_by_cluster);
        let index = &export.search_index;

        assert_eq!(index.phrase.len(), 2, "表記ゆれ2件は1行に畳まれるべき");
        let kahler = index.phrase.iter().position(|p| p == "kahler manifold").unwrap();
        assert!(!index.phrase.contains(&"kahler manifolds".to_string()), "別表記が独立した行として残ってはいけない");
        assert_eq!(index.doc_freq[kahler], 100, "畳んだメンバーのdoc_freqの合計であるべき（30+70）");
        assert_eq!(index.msc_code[kahler], Some("32Q15".to_string()));
        assert_eq!(index.cluster_id[kahler], Some(0));
        assert_eq!(export.resolved_concept_count, 2);
        assert_eq!(export.candidate_count, 3, "candidate_countは畳む前の生候補数のまま");
    }

    #[test]
    fn related_edges_go_to_their_own_export_and_empty_neighbor_lists_are_dropped() {
        let mut neighbors = HashMap::new();
        neighbors.insert("kähler manifold".to_string(), vec![("ricci flow".to_string(), 0.62_f32)]);
        neighbors.insert("ricci flow".to_string(), vec![]); // 近傍が閾値未満で0件だったケース

        let related = build_related_export(&neighbors);

        assert_eq!(related.source, vec!["kähler manifold".to_string()], "空の近傍リストは書き出さない");
        assert_eq!(related.targets, vec![vec!["ricci flow".to_string()]]);
        assert!((related.scores[0][0] - 0.62).abs() < 1e-3);
    }

    #[test]
    fn related_export_is_ordered_so_the_same_db_always_produces_the_same_bytes() {
        // `HashMap` の反復順は不定なので、出力側でソートしていないと
        // 実行のたびにファイルのバイト列が変わり、差分確認もキャッシュも
        // 当てにならなくなる。
        let mut neighbors = HashMap::new();
        for phrase in ["zeta function", "abelian variety", "moduli space"] {
            neighbors.insert(phrase.to_string(), vec![("x".to_string(), 0.5_f32)]);
        }
        let related = build_related_export(&neighbors);
        assert_eq!(
            related.source,
            vec!["abelian variety".to_string(), "moduli space".to_string(), "zeta function".to_string()]
        );
    }

    fn paper(id: &str, title: &str, year: u16) -> PaperRow {
        PaperRow {
            arxiv_id: id.to_string(),
            title: title.to_string(),
            year: Some(year),
            primary_category: Some("math.AG".to_string()),
        }
    }

    fn link(id: &str, phrase: &str) -> (String, String) {
        (id.to_string(), phrase.to_string())
    }

    #[test]
    fn papers_export_carries_titles_not_just_ids() {
        // これが `PapersExport` の存在理由。以前の出力にはIDしか無く、
        // 利用者は題名すら見られなかった。
        let candidates = vec![candidate("moduli space", 10, None)];
        let resolved = resolve_all(&candidates);
        let papers = vec![paper("alg-geom/9710015", "The moduli space of curves", 1997)];
        let links = vec![link("alg-geom/9710015", "moduli space")];
        let export = build_papers_export(&resolved, &candidates, &papers, &links);
        assert_eq!(export.source, vec!["moduli space".to_string()]);
        assert_eq!(export.title, vec!["The moduli space of curves".to_string()]);
        assert_eq!(export.year, vec![Some(1997)]);
        assert_eq!(export.papers, vec![vec![0u32]]);
    }

    #[test]
    fn papers_whose_title_contains_the_concept_rank_first() {
        // 引用数を持たない以上、これが「この論文はこの概念についてだ」と
        // 確実に言える唯一の信号。
        let candidates = vec![candidate("moduli space", 10, None)];
        let resolved = resolve_all(&candidates);
        let papers = vec![
            paper("math/0002", "A remark on stability conditions", 2000),
            paper("math/0001", "Moduli space of stable maps", 2000),
        ];
        let links = vec![link("math/0002", "moduli space"), link("math/0001", "moduli space")];
        let export = build_papers_export(&resolved, &candidates, &papers, &links);
        let first = export.papers[0][0] as usize;
        assert_eq!(export.arxiv_id[first], "math/0001", "題名に概念を含む論文が先頭");
    }

    #[test]
    fn papers_export_only_includes_papers_actually_referenced() {
        let candidates = vec![candidate("moduli space", 10, None)];
        let resolved = resolve_all(&candidates);
        let papers = vec![paper("math/0001", "Moduli space", 2000), paper("math/9999", "Unrelated", 1999)];
        let links = vec![link("math/0001", "moduli space")];
        let export = build_papers_export(&resolved, &candidates, &papers, &links);
        assert_eq!(export.arxiv_id, vec!["math/0001".to_string()], "参照されない論文は書き出さない");
    }

    #[test]
    fn papers_per_concept_are_capped() {
        let candidates = vec![candidate("moduli space", 40, None)];
        let resolved = resolve_all(&candidates);
        let papers: Vec<PaperRow> = (0..40).map(|i| paper(&format!("math/{i:04}"), "Untitled", 2000)).collect();
        let links: Vec<(String, String)> =
            (0..40).map(|i| link(&format!("math/{i:04}"), "moduli space")).collect();
        let export = build_papers_export(&resolved, &candidates, &papers, &links);
        assert_eq!(export.papers[0].len(), MAX_PAPERS_PER_CONCEPT);
    }

    #[test]
    fn papers_export_aggregates_papers_across_spelling_variants_under_the_representative() {
        // search_indexを代表表記1行に畳んだ以上、その行の出典論文が
        // 代表表記だけで言及した論文に限られてはいけない——"kahler
        // manifolds"（別表記）だけで言及した論文が消えてしまう。
        let candidates = vec![candidate("kahler manifolds", 1, None), candidate("kahler manifold", 2, None)];
        let resolved = resolve_all(&candidates);
        let papers = vec![
            paper("math/0001", "A paper using the plural form", 2000),
            paper("math/0002", "A paper using the singular form", 2001),
        ];
        let links = vec![link("math/0001", "kahler manifolds"), link("math/0002", "kahler manifold")];
        let export = build_papers_export(&resolved, &candidates, &papers, &links);

        assert_eq!(export.source, vec!["kahler manifold".to_string()], "1行に畳まれるべき");
        assert_eq!(export.arxiv_id.len(), 2, "どちらの表記で言及した論文も残るべき");
        let ids: Vec<&str> = export.arxiv_id.iter().map(String::as_str).collect();
        assert!(ids.contains(&"math/0001"), "別表記(plural)だけで言及した論文が消えてはいけない");
        assert!(ids.contains(&"math/0002"));
    }

    #[test]
    fn alias_export_only_lists_groups_that_actually_have_variants() {
        let groups = vec![
            ("zeta function".to_string(), vec!["zeta-function".to_string(), "zeta functions".to_string()]),
            ("l function".to_string(), vec![]),
            ("abelian variety".to_string(), vec!["abelian varieties".to_string()]),
        ];
        let export = build_alias_export(&groups);
        assert_eq!(export.representative, vec!["abelian variety".to_string(), "zeta function".to_string()]);
        assert_eq!(export.aliases[0], vec!["abelian varieties".to_string()]);
    }

    fn relation(
        subject: &str,
        object: &str,
        status: crate::relations::RelationStatus,
        confidence: f32,
        evidence: Option<&str>,
    ) -> crate::relations::RelationEdge {
        crate::relations::RelationEdge {
            subject: subject.to_string(),
            object: object.to_string(),
            kind: crate::relations::RelationKind::SpecializationOf,
            status,
            confidence,
            evidence_sentence: evidence.map(str::to_string),
            evidence_arxiv_id: evidence.map(|_| "math/0001".to_string()),
        }
    }

    #[test]
    fn relations_export_drops_proposed_edges_without_evidence() {
        use crate::relations::RelationStatus::{Confirmed, Grounded, Proposed};
        let edges = vec![
            relation("elliptic curves", "abelian varieties", Confirmed, 0.9, Some("An elliptic curve is a special case of an abelian variety.")),
            relation("quantum groups", "hopf algebras", Grounded, 1.0, Some("Quantum groups are a special case of Hopf algebras.")),
            relation("kähler manifold", "symplectic manifold", Proposed, 0.6, None),
        ];
        let export = build_relations_export(&edges);
        assert_eq!(export.subject.len(), 2, "根拠文の無いProposedは出さない");
        assert!(!export.status.contains(&"proposed".to_string()));
    }

    #[test]
    fn relations_export_orders_confirmed_before_grounded() {
        use crate::relations::RelationStatus::{Confirmed, Grounded};
        let edges = vec![
            relation("b concept", "b broader", Grounded, 1.0, Some("sentence b")),
            relation("a concept", "a broader", Confirmed, 0.5, Some("sentence a")),
        ];
        let export = build_relations_export(&edges);
        assert_eq!(export.status[0], "confirmed", "Confirmedを先に出す");
        assert_eq!(export.subject[0], "a concept");
    }

    fn columns_from(phrases_and_freqs: &[(&str, usize)]) -> SearchIndexColumns {
        SearchIndexColumns {
            phrase: phrases_and_freqs.iter().map(|(p, _)| p.to_string()).collect(),
            doc_freq: phrases_and_freqs.iter().map(|(_, f)| *f).collect(),
            msc_code: phrases_and_freqs.iter().map(|_| None).collect(),
            cluster_id: phrases_and_freqs.iter().map(|_| None).collect(),
            field_concentration: phrases_and_freqs.iter().map(|_| None).collect(),
        }
    }

    #[test]
    fn head_shard_keeps_only_the_most_frequent_entries_in_descending_order() {
        let columns = columns_from(&[("rare term", 2), ("popular term", 50), ("mid term", 10)]);
        let shard = build_head_shard(&columns);
        assert_eq!(shard.phrase, vec!["popular term", "mid term", "rare term"]);
        assert_eq!(shard.doc_freq, vec![50, 10, 2]);
    }

    #[test]
    fn head_shard_truncates_to_head_shard_size() {
        let entries: Vec<(String, usize)> = (0..(HEAD_SHARD_SIZE + 50)).map(|i| (format!("term{i}"), i)).collect();
        let refs: Vec<(&str, usize)> = entries.iter().map(|(p, f)| (p.as_str(), *f)).collect();
        let columns = columns_from(&refs);
        let shard = build_head_shard(&columns);
        assert_eq!(shard.phrase.len(), HEAD_SHARD_SIZE, "上位HEAD_SHARD_SIZE件だけを残すべき");
        // 降順の先頭は最大のdoc_freq（末尾に近いインデックスほどfが大きい）
        assert_eq!(shard.doc_freq[0], HEAD_SHARD_SIZE + 49);
    }

    #[test]
    fn head_shard_of_a_small_corpus_is_not_padded() {
        let columns = columns_from(&[("only term", 1)]);
        let shard = build_head_shard(&columns);
        assert_eq!(shard.phrase.len(), 1, "HEAD_SHARD_SIZEより少ない件数を水増ししない");
    }
}

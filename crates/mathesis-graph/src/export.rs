//! Phase 10: 判断グラフ（Lean/Coqの形式証明、Phase 9で`mathesis-importer`が
//! 取り込んだ実データ）をWeb版Explorerが読める静的JSONへ書き出す。
//!
//! `crates/mathesis-taxonomy::export`（Phase 6、arXiv概念タクソノミー側）と
//! 対になる、判断グラフ側の静的エクスポート。判断ノード（種別・名前・
//! ステートメント・コンテキストΓ・由来ファイル）、`judgment_dependencies`
//! （Phase 9、証明が参照している他の判断）、`papers`（arXiv論文への
//! リンク）をまとめてJSONへ落とす——ブラウザは`mathesis-graph`のSQLiteに
//! 直接触れられない（wasm32-unknown-unknownにはrusqliteのCコンパイラ依存が
//! ビルドできないため、`web/README.md`参照）ので、この静的スナップショット
//! 経由が現状唯一の橋渡し。

use crate::model::JudgmentRecord;
use crate::paper::PaperId;
use crate::store::{GraphStore, Result};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedJudgment {
    pub id: i64,
    pub kind: String,
    pub name: Option<String>,
    pub statement: String,
    pub context: Vec<String>,
    pub parse_status: String,
    pub source_file: String,
    pub source_line: u32,
    pub paper_arxiv_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedPaper {
    pub arxiv_id: String,
    pub title: Option<String>,
    pub judgment_count: usize,
    /// 診断⑥拡張（`paper_citation.rs`）: この論文が引用している他の論文の
    /// arXiv id。著者自身が明記した"arXiv:"記載から解決できた分だけで、
    /// かつ引用先がこのグラフに既に橋渡し済み（＝判断を1件以上持つ）
    /// 論文の場合のみ——著者名・タイトルの一致には頼らない。
    pub cites: Vec<String>,
}

// `dependencies`/`morphisms`（辺そのもの）はここには**もう無い**
// （P2、`docs/P2_STATUS.md`）。Web版は`mathesis-provenance web-export`が
// 証拠層から直接生成する`dependencies.json`/`morphisms.json`を読む——
// `kind`/`origin`/`status`/`rationale`は`morphisms`テーブルの生の値ではなく
// `RelationAssertion`+`Evidence`+`ReviewDecision`から再構成されたものになり、
// 二重の読み方が生まれないようにするため、この構造体からは辺の配列そのものを
// 削除した。件数（構造ノード側の統計として引き続き有用）だけはここに残す。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphExport {
    /// このJSONを`--export`が書き出した時刻（UNIX秒）。外部レビュー
    /// （2026-09-05）で「データの生成日時が画面に見えない」と指摘されたため
    /// 追加した——`mathesis-taxonomy::export::TaxonomyExport`と同型の対応。
    pub generated_at_unix: u64,
    pub judgment_count: usize,
    pub dependency_count: usize,
    pub morphism_count: usize,
    pub papers: Vec<ExportedPaper>,
    pub judgments: Vec<ExportedJudgment>,
}

fn context_line(name: &str, ty: &mathesis_ast::Expr) -> String {
    format!("{name} : {ty}")
}

fn to_exported(j: &JudgmentRecord, paper_arxiv_id: Option<String>) -> ExportedJudgment {
    ExportedJudgment {
        id: j.id.0,
        kind: j.kind.as_str().to_string(),
        name: j.name.clone(),
        statement: j.statement.to_string(),
        context: j.context.iter().map(|(n, t)| context_line(n, t)).collect(),
        parse_status: j.parse_status.as_str().to_string(),
        source_file: j.source_file.clone(),
        source_line: j.source_line,
        paper_arxiv_id,
    }
}

/// グラフストア全体を`GraphExport`へまとめる。判断1件ごとに
/// `dependencies_of`を呼ぶ素朴な実装——今の規模（実データで1,431件）では
/// 十分速い（実測は呼び出し側のCLI出力参照）。論文idの解決はプロセス内で
/// キャッシュし、同じ論文への問い合わせを繰り返さない。
pub fn build_export(store: &GraphStore) -> Result<GraphExport> {
    let judgments = store.list_judgments()?;

    let mut paper_arxiv_id_cache: HashMap<PaperId, String> = HashMap::new();
    let mut paper_judgment_counts: HashMap<PaperId, usize> = HashMap::new();

    let mut exported_judgments = Vec::with_capacity(judgments.len());
    let mut dependency_count = 0usize;

    for j in &judgments {
        let paper_arxiv_id = match j.source_paper {
            Some(pid) => {
                *paper_judgment_counts.entry(pid).or_default() += 1;
                if let Some(cached) = paper_arxiv_id_cache.get(&pid) {
                    Some(cached.clone())
                } else {
                    let resolved = store.get_paper(pid)?.map(|r| r.arxiv_id);
                    if let Some(a) = &resolved {
                        paper_arxiv_id_cache.insert(pid, a.clone());
                    }
                    resolved
                }
            }
            None => None,
        };

        exported_judgments.push(to_exported(j, paper_arxiv_id));
        dependency_count += store.dependencies_of(j.id)?.len();
    }

    let paper_records = store.list_papers()?;
    // 診断⑥拡張の引用先解決に使う——`citations_of`が返すPaperIdは、
    // judgment数が0の論文（理論上ありうる）も含めて`papers`テーブル全体の
    // どれかを指すので、`paper_arxiv_id_cache`（judgmentから辿れた分だけ）
    // ではなく`list_papers`の全件から作る。
    let arxiv_id_by_paper_id: HashMap<PaperId, String> =
        paper_records.iter().map(|p| (p.id, p.arxiv_id.clone())).collect();

    let mut papers = Vec::with_capacity(paper_records.len());
    for p in paper_records {
        let cites: Vec<String> = store
            .citations_of(p.id)?
            .into_iter()
            .filter_map(|target| arxiv_id_by_paper_id.get(&target).cloned())
            .collect();
        papers.push(ExportedPaper {
            judgment_count: paper_judgment_counts.get(&p.id).copied().unwrap_or(0),
            arxiv_id: p.arxiv_id,
            title: p.title,
            cites,
        });
    }

    let morphism_count = store.list_morphisms()?.len();

    Ok(GraphExport {
        generated_at_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        judgment_count: exported_judgments.len(),
        dependency_count,
        morphism_count,
        papers,
        judgments: exported_judgments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{JudgmentKind, NewJudgment, ParseStatus, SourceRef};
    use mathesis_ast::parse_expr;

    fn insert_named(store: &GraphStore, name: &str, stmt: &str, paper: Option<PaperId>) -> crate::model::JudgmentId {
        let expr = parse_expr(stmt).unwrap().expr;
        let statement = store.intern_expr(&expr).unwrap();
        store
            .insert_judgment(&NewJudgment {
                kind: JudgmentKind::Theorem,
                name: Some(name.into()),
                context: vec![],
                statement,
                definition_body_raw: None,
                source: SourceRef { file: "export_test.lean".into(), line: 1 },
                raw_text: format!("theorem {name} : {stmt}"),
                parse_status: ParseStatus::Full,
                source_paper: paper,
            })
            .unwrap()
    }

    #[test]
    fn build_export_includes_judgments_dependencies_and_paper_linkage() {
        let store = GraphStore::open_in_memory().unwrap();
        let paper = store.intern_paper("2604.05984", Some("DeGiorgi")).unwrap();
        let base = insert_named(&store, "base_lemma", "True", Some(paper));
        let derived = insert_named(&store, "derived_thm", "True", Some(paper));
        let standalone = insert_named(&store, "standalone", "True", None);
        store.record_judgment_dependency(derived, base).unwrap();

        let export = build_export(&store).unwrap();

        assert_eq!(export.judgment_count, 3);
        assert_eq!(export.dependency_count, 1);
        assert_eq!(export.papers.len(), 1);
        assert_eq!(export.papers[0].arxiv_id, "2604.05984");
        assert_eq!(export.papers[0].judgment_count, 2);

        let derived_export = export.judgments.iter().find(|j| j.id == derived.0).unwrap();
        assert_eq!(derived_export.paper_arxiv_id.as_deref(), Some("2604.05984"));
        let standalone_export = export.judgments.iter().find(|j| j.id == standalone.0).unwrap();
        assert_eq!(standalone_export.paper_arxiv_id, None);
    }

    #[test]
    fn build_export_includes_paper_level_citations() {
        let store = GraphStore::open_in_memory().unwrap();
        let citing = store.intern_paper("math/0002", None).unwrap();
        let cited = store.intern_paper("math/0001", None).unwrap();
        insert_named(&store, "thm_citing", "True", Some(citing));
        insert_named(&store, "thm_cited", "True", Some(cited));
        store.record_paper_citation(citing, cited).unwrap();

        let export = build_export(&store).unwrap();

        let citing_export = export.papers.iter().find(|p| p.arxiv_id == "math/0002").unwrap();
        assert_eq!(citing_export.cites, vec!["math/0001".to_string()]);
        let cited_export = export.papers.iter().find(|p| p.arxiv_id == "math/0001").unwrap();
        assert!(cited_export.cites.is_empty(), "引用される側は何も引用していない");
    }

    /// 射そのもの（kind/origin/status/rationale）のexportは
    /// `mathesis-provenance web_export`側（P2、`docs/P2_STATUS.md`）が担う
    /// ようになったため、ここでは件数だけを確かめる——per-morphismの詳細は
    /// `mathesis-provenance`の`web_export`/`legacy_adapter_test.rs`が検証する。
    #[test]
    fn build_export_counts_morphisms_without_exporting_them() {
        use crate::morphism::MorphismKind;

        let store = GraphStore::open_in_memory().unwrap();
        let group_hom = insert_named(&store, "group_hom", "True", None);
        let abelian_group_hom = insert_named(&store, "abelian_group_hom", "True", None);
        store
            .annotate(group_hom, abelian_group_hom, MorphismKind::Specialization, Some("test rationale".into()))
            .unwrap();

        let export = build_export(&store).unwrap();

        assert_eq!(export.morphism_count, 1);
    }
}

//! 診断⑥拡張: `\cite`が指す論文単位の引用関係。
//!
//! `judgment_dependencies`（判断ノード間の「証明が参照している」依存、
//! `\ref`由来）とは意味が異なるため別テーブルにする——`\ref`はラベル経由で
//! 「同じ論文内の特定の1判断」を指すのに対し、`\cite`が指すのは「引用文献
//! という1本の論文全体」であって、その論文の**どの判断**を参照している
//! かという情報を`\cite`自体は持たない。片方をもう片方に無理に押し込める
//! と、実データに無い辺（存在しない特定の判断への依存）を作り話すことに
//! なる（`mathesis-fulltext::bridge`のdoc comment参照）。
//!
//! 解決は`crates/mathesis-fulltext::citation`が行う——引用文献の生テキスト
//! （`\bibitem`本文）に著者自身が明記した"arXiv:1234.56789"のような
//! 具体的なIDだけを抽出し、その論文が既にこのグラフに`intern_paper`済み
//! （＝橋渡し済みの判断を1件以上持つ）ときだけ辺を張る。著者名・タイトルの
//! 文字列一致には一切頼らない——曖昧な場合は見送る。

use crate::paper::PaperId;
use crate::store::{GraphStore, Result};
use rusqlite::params;

impl GraphStore {
    /// `from`論文が`to`論文を引用していることを記録する。同じ組を重ねて
    /// 呼んでも冪等（`record_judgment_dependency`と同じ`INSERT OR IGNORE`）。
    pub fn record_paper_citation(&self, from: PaperId, to: PaperId) -> Result<()> {
        self.conn
            .prepare_cached("INSERT OR IGNORE INTO paper_citations (from_paper, to_paper) VALUES (?1, ?2)")?
            .execute(params![from.0, to.0])?;
        Ok(())
    }

    /// `paper`が引用している論文（＝`paper`から見た引用先）。
    pub fn citations_of(&self, paper: PaperId) -> Result<Vec<PaperId>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT to_paper FROM paper_citations WHERE from_paper = ?1 ORDER BY to_paper")?;
        let rows =
            stmt.query_map(params![paper.0], |r| r.get::<_, i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows.into_iter().map(PaperId).collect())
    }

    /// `paper`を引用している論文（＝逆方向、"これを引用しているのはどれか"）。
    pub fn cited_by(&self, paper: PaperId) -> Result<Vec<PaperId>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT from_paper FROM paper_citations WHERE to_paper = ?1 ORDER BY from_paper")?;
        let rows =
            stmt.query_map(params![paper.0], |r| r.get::<_, i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows.into_iter().map(PaperId).collect())
    }

    pub fn paper_citation_count(&self) -> Result<i64> {
        self.conn.query_row("SELECT COUNT(*) FROM paper_citations", [], |r| r.get(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{JudgmentKind, NewJudgment, ParseStatus, SourceRef};
    use mathesis_ast::Expr;

    fn paper_with_a_judgment(store: &GraphStore, arxiv_id: &str) -> PaperId {
        let paper_id = store.intern_paper(arxiv_id, None).unwrap();
        let statement = store.intern_expr(&Expr::Unparsed("placeholder".into())).unwrap();
        store
            .insert_judgment(&NewJudgment {
                kind: JudgmentKind::Theorem,
                name: None,
                context: vec![],
                statement,
                definition_body_raw: None,
                source: SourceRef { file: format!("arxiv:{arxiv_id}"), line: 1 },
                raw_text: "placeholder".into(),
                parse_status: ParseStatus::Informal,
                source_paper: Some(paper_id),
            })
            .unwrap();
        paper_id
    }

    #[test]
    fn records_and_queries_a_citation_in_both_directions() {
        let store = GraphStore::open_in_memory().unwrap();
        let a = paper_with_a_judgment(&store, "math/0001");
        let b = paper_with_a_judgment(&store, "math/0002");

        store.record_paper_citation(a, b).unwrap();

        assert_eq!(store.citations_of(a).unwrap(), vec![b]);
        assert_eq!(store.cited_by(b).unwrap(), vec![a]);
        assert!(store.citations_of(b).unwrap().is_empty(), "bはaを引用していない");
        assert_eq!(store.paper_citation_count().unwrap(), 1);
    }

    #[test]
    fn recording_the_same_citation_twice_is_idempotent() {
        let store = GraphStore::open_in_memory().unwrap();
        let a = paper_with_a_judgment(&store, "math/0001");
        let b = paper_with_a_judgment(&store, "math/0002");

        store.record_paper_citation(a, b).unwrap();
        store.record_paper_citation(a, b).unwrap();

        assert_eq!(store.paper_citation_count().unwrap(), 1);
    }

    #[test]
    fn a_paper_can_cite_multiple_papers() {
        let store = GraphStore::open_in_memory().unwrap();
        let a = paper_with_a_judgment(&store, "math/0001");
        let b = paper_with_a_judgment(&store, "math/0002");
        let c = paper_with_a_judgment(&store, "math/0003");

        store.record_paper_citation(a, b).unwrap();
        store.record_paper_citation(a, c).unwrap();

        assert_eq!(store.citations_of(a).unwrap(), vec![b, c]);
    }
}

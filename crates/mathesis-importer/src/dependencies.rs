//! Phase 9: 1回のインポートで読み込んだ判断ノード群から、証明・定義本体が
//! 既存の判断名を参照している箇所を検出し、`GraphStore::judgment_dependencies`
//! （論理的な射である`morphisms`とは別物、`crates/mathesis-fulltext`の
//! Phase 8 `theorem_dependencies`のLean版）へ記録する。
//!
//! 名前解決は同一インポートバッチ内（＝この実行で読み込んだファイル群）に
//! 閉じる——他の実行・他のDBの判断へは結びつけない、既知の制約。

use mathesis_graph::{GraphStore, JudgmentId};
use mathesis_lean_parse::{find_referenced_names, strip_lean_comments};
use std::collections::{HashMap, HashSet};

/// 依存関係抽出の入力として、1件の判断について必要な最小限の情報。
pub struct InsertedJudgment {
    pub id: JudgmentId,
    pub name: Option<String>,
    /// シグネチャ＋証明/定義本体を含む生テキスト（`raw_text`）。
    pub raw_text: String,
}

/// このインポートバッチで挿入された判断ノード群を突き合わせ、`judgment_dependencies`
/// へ記録する。記録した辺の総数を返す。
pub fn record_dependencies(
    store: &GraphStore,
    inserted: &[InsertedJudgment],
) -> mathesis_graph::store::Result<usize> {
    let name_to_id: HashMap<&str, JudgmentId> =
        inserted.iter().filter_map(|j| j.name.as_deref().map(|n| (n, j.id))).collect();
    let known_names: HashSet<&str> = name_to_id.keys().copied().collect();

    let mut count = 0usize;
    for j in inserted {
        let cleaned = strip_lean_comments(&j.raw_text);
        for referenced in find_referenced_names(&cleaned, &known_names) {
            if Some(referenced.as_str()) == j.name.as_deref() {
                continue; // 自己参照（再帰定義等）は依存として数えない
            }
            if let Some(&target) = name_to_id.get(referenced.as_str()) {
                store.record_judgment_dependency(j.id, target)?;
                count += 1;
            }
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    // `strip_lean_comments`・`find_referenced_names` そのものの単体テストは
    // `mathesis-lean-parse`（この2関数が実際に定義されている場所）に移した。
    // ここに残すのは、`GraphStore`（実際のDB挿入・外部キー制約）と組み
    // 合わさったときの`record_dependencies`のふるまいだけ。
    use super::*;

    /// `judgment_dependencies`は`judgments`への外部キー制約を持つため、テストでも
    /// 実在するJudgmentIdが要る——名前だけの`InsertedJudgment`をでっち上げても
    /// 挿入時にFOREIGN KEY違反になる。ダミーの中身で実際に1件insertし、
    /// 採番されたIDを使う。
    fn insert_dummy_judgment(store: &GraphStore, name: &str) -> JudgmentId {
        use mathesis_ast::parse_expr;
        use mathesis_graph::{JudgmentKind, NewJudgment, ParseStatus, SourceRef};

        let stmt = parse_expr("True").unwrap().expr;
        let stmt_id = store.intern_expr(&stmt).unwrap();
        store
            .insert_judgment(&NewJudgment {
                kind: JudgmentKind::Theorem,
                name: Some(name.to_string()),
                context: vec![],
                statement: stmt_id,
                definition_body_raw: None,
                source: SourceRef { file: "test.lean".to_string(), line: 1 },
                raw_text: String::new(),
                parse_status: ParseStatus::Full,
                source_paper: None,
            })
            .unwrap()
    }

    #[test]
    fn record_dependencies_ignores_names_mentioned_only_in_a_doc_comment() {
        // 実データ(DeGiorgiコーパス`Supersolutions/TestFunctions.lean`)で
        // 見つかった実際のパターンの再現: モジュールdocコメントが
        // 「Main results: weak_harnack_stage_one_inverse」のように他の
        // 判断名を地の文で挙げている。これを本物の証明上の依存として
        // 誤検出してはいけない。
        let store = GraphStore::open_in_memory().unwrap();
        let main_id = insert_dummy_judgment(&store, "weak_harnack_stage_one_inverse");
        let unrelated_id = insert_dummy_judgment(&store, "unrelated_lemma");
        let inserted = vec![
            InsertedJudgment {
                id: main_id,
                name: Some("weak_harnack_stage_one_inverse".to_string()),
                raw_text: "theorem weak_harnack_stage_one_inverse : True := trivial".to_string(),
            },
            InsertedJudgment {
                id: unrelated_id,
                name: Some("unrelated_lemma".to_string()),
                raw_text: "/-!\nSee `weak_harnack_stage_one_inverse` for the main estimate.\n-/\ntheorem unrelated_lemma : True := trivial".to_string(),
            },
        ];
        let count = record_dependencies(&store, &inserted).unwrap();
        assert_eq!(count, 0, "docコメント内の言及は依存として記録してはいけない");
    }

    #[test]
    fn record_dependencies_finds_a_real_reference_in_a_proof_body() {
        let store = GraphStore::open_in_memory().unwrap();
        let base_id = insert_dummy_judgment(&store, "base_lemma");
        let derived_id = insert_dummy_judgment(&store, "derived_thm");
        let inserted = vec![
            InsertedJudgment {
                id: base_id,
                name: Some("base_lemma".to_string()),
                raw_text: "theorem base_lemma : True := trivial".to_string(),
            },
            InsertedJudgment {
                id: derived_id,
                name: Some("derived_thm".to_string()),
                raw_text: "theorem derived_thm : True := by exact base_lemma".to_string(),
            },
        ];
        let count = record_dependencies(&store, &inserted).unwrap();
        assert_eq!(count, 1);
        let deps = store.dependencies_of(derived_id).unwrap();
        assert_eq!(deps, vec![base_id]);
    }

    #[test]
    fn record_dependencies_does_not_record_a_self_reference() {
        let store = GraphStore::open_in_memory().unwrap();
        let rec_id = insert_dummy_judgment(&store, "rec_def");
        let inserted = vec![InsertedJudgment {
            id: rec_id,
            name: Some("rec_def".to_string()),
            raw_text: "def rec_def : Nat := rec_def".to_string(),
        }];
        let count = record_dependencies(&store, &inserted).unwrap();
        assert_eq!(count, 0, "再帰的な自己参照を依存として数えてはいけない");
    }
}

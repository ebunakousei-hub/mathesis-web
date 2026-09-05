#[cfg(test)]
mod integration_tests {
    use mathesis_ast::parse_expr;
    use mathesis_graph::GraphStore;

    #[test]
    fn test_import_integration() {
        // Create an in-memory store and test basic import workflow
        let store = GraphStore::open_in_memory().unwrap();

        // Simulate what the importer does
        let theorem_name = "add_assoc";
        let statement_str = "a + b + c = a + (b + c)";

        // Parse context
        let context_expr = parse_expr("ℕ").unwrap().expr;
        let context_id = store.intern_expr(&context_expr).unwrap();

        // Parse statement
        let stmt_expr = parse_expr(statement_str).unwrap().expr;
        let stmt_id = store.intern_expr(&stmt_expr).unwrap();

        // Create hypothesis
        let hypotheses = vec![
            mathesis_graph::Hypothesis {
                name: "a".to_string(),
                ty: context_id,
            },
            mathesis_graph::Hypothesis {
                name: "b".to_string(),
                ty: context_id,
            },
            mathesis_graph::Hypothesis {
                name: "c".to_string(),
                ty: context_id,
            },
        ];

        // Insert judgment
        let judgment = mathesis_graph::NewJudgment {
            kind: mathesis_graph::JudgmentKind::Theorem,
            name: Some(theorem_name.to_string()),
            context: hypotheses,
            statement: stmt_id,
            definition_body_raw: None,
            source: mathesis_graph::SourceRef {
                file: "sample_algebra.lean".to_string(),
                line: 1,
            },
            raw_text: format!("theorem {} : {} := by ...", theorem_name, statement_str),
            parse_status: mathesis_graph::ParseStatus::Full,
            source_paper: None,
        };

        let judgment_id = store.insert_judgment(&judgment).unwrap();

        // Verify stored data
        let retrieved = store.get_judgment(judgment_id).unwrap();
        assert_eq!(retrieved.name.as_deref(), Some(theorem_name));
        assert_eq!(retrieved.kind, mathesis_graph::JudgmentKind::Theorem);
        assert_eq!(retrieved.context.len(), 3);
        assert_eq!(retrieved.context[0].0, "a");
        assert_eq!(retrieved.context[1].0, "b");
        assert_eq!(retrieved.context[2].0, "c");

        // List all judgments
        let all_judgments = store.list_judgments().unwrap();
        assert!(all_judgments.len() >= 1);
        assert!(all_judgments.iter().any(|j| j.name.as_deref() == Some(theorem_name)));
    }

    #[test]
    fn test_alpha_equivalent_parsing() {
        let store = GraphStore::open_in_memory().unwrap();

        // Two theorems with the same statement but different variable names
        let stmt1 = parse_expr("∀ x, x + 0 = x").unwrap().expr;
        let stmt2 = parse_expr("∀ y, y + 0 = y").unwrap().expr;

        let id1 = store.intern_expr(&stmt1).unwrap();
        let id2 = store.intern_expr(&stmt2).unwrap();

        // They should be interned to the same expression node
        assert_eq!(id1, id2, "α-equivalent expressions should share nodes");
    }

    #[test]
    fn test_storage_validation() {
        let store = GraphStore::open_in_memory().unwrap();

        // Add some valid data
        let context_expr = parse_expr("ℕ").unwrap().expr;
        let context_id = store.intern_expr(&context_expr).unwrap();

        let stmt_expr = parse_expr("a + b = b + a").unwrap().expr;
        let stmt_id = store.intern_expr(&stmt_expr).unwrap();

        let hypotheses = vec![mathesis_graph::Hypothesis {
            name: "a".to_string(),
            ty: context_id,
        }];

        store
            .insert_judgment(&mathesis_graph::NewJudgment {
                kind: mathesis_graph::JudgmentKind::Theorem,
                name: Some("comm".to_string()),
                context: hypotheses,
                statement: stmt_id,
                definition_body_raw: None,
                source: mathesis_graph::SourceRef {
                    file: "test.lean".to_string(),
                    line: 1,
                },
                raw_text: "theorem comm (a : ℕ) : a + b = b + a".to_string(),
                parse_status: mathesis_graph::ParseStatus::Full,
                source_paper: None,
            })
            .unwrap();

        // Validate the store
        let report = store.validate().unwrap();
        assert!(
            report.is_valid(),
            "Store should be valid. Issues: {:?}",
            report.broken_references
        );
        assert!(report.total_expressions > 0);
        assert_eq!(report.total_judgments, 1);
        println!("Validation report: {}", report.summary());
    }

}

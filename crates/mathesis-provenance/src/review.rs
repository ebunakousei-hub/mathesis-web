use crate::model::{AssertionId, NewReviewDecision, ReviewDecision, ReviewId, ReviewOutcome};
use crate::store::{ProvenanceStore, Result};
use rusqlite::params;

impl ProvenanceStore {
    pub fn insert_review_decision(&self, new: &NewReviewDecision) -> Result<ReviewId> {
        self.conn
            .prepare_cached(
                "INSERT INTO review_decisions
                    (assertion_id, decision, reviewer_id, authorization_level, scope, rationale,
                     decided_at_unix, dataset_version, expires_at_unix, supersedes_review_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?
            .execute(params![
                new.assertion_id.0,
                new.decision.as_str(),
                new.reviewer_id,
                new.authorization_level,
                new.scope,
                new.rationale,
                new.decided_at_unix,
                new.dataset_version,
                new.expires_at_unix,
                new.supersedes_review_id.map(|id| id.0),
            ])?;
        Ok(ReviewId(self.conn.last_insert_rowid()))
    }

    /// 時系列順(古い→新しい、id は decided_at_unix の同点タイブレーク)。
    pub fn review_decisions_for(&self, assertion: AssertionId) -> Result<Vec<ReviewDecision>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, assertion_id, decision, reviewer_id, authorization_level, scope, rationale,
                    decided_at_unix, dataset_version, expires_at_unix, supersedes_review_id
             FROM review_decisions WHERE assertion_id = ?1 ORDER BY decided_at_unix, id",
        )?;
        let rows = stmt
            .query_map(params![assertion.0], Self::review_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// P6.3（`docs/P6_3_STATUS.md`）: レビューは追記専用ログなので「今この
    /// assertionをどう扱うべきか」は個々の行ではなく最新の行で決まる——
    /// accept の後に revoke/reject が積まれれば、古い accept は残るが
    /// 効力を失う。`supersedes_review_id`は監査用の相互参照で、この解決
    /// 自体はチェーンを辿らない(時刻順の最後の行を採用するだけで、
    /// 「誰が何を置き換えたか」の記録と「今の実効判断は何か」の判定を
    /// 分離している)。
    pub fn effective_review_decision(&self, assertion: AssertionId) -> Result<Option<ReviewDecision>> {
        Ok(self.review_decisions_for(assertion)?.into_iter().last())
    }

    pub fn review_decision_count(&self) -> Result<i64> {
        self.conn.query_row("SELECT COUNT(*) FROM review_decisions", [], |r| r.get(0))
    }

    fn review_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReviewDecision> {
        let decision_str: String = row.get(2)?;
        Ok(ReviewDecision {
            id: ReviewId(row.get(0)?),
            assertion_id: AssertionId(row.get(1)?),
            decision: ReviewOutcome::from_str(&decision_str).expect("保存済み decision は常に既知の値"),
            reviewer_id: row.get(3)?,
            authorization_level: row.get(4)?,
            scope: row.get(5)?,
            rationale: row.get(6)?,
            decided_at_unix: row.get(7)?,
            dataset_version: row.get(8)?,
            expires_at_unix: row.get(9)?,
            supersedes_review_id: row.get::<_, Option<i64>>(10)?.map(ReviewId),
        })
    }
}

/// P6.3: 「本人確認済みのaccept」の定義そのもの——`ARCHITECTURE_NEXT.md`の
/// 精神を踏まえ、単なる`decision == Accept`より厳密にする:
///
///   1. 直近の実効判断が信頼付与側(`Accept`/`Supersede`)であること
///      (Reject/Revoke/Split/Merge/NeedsExpertはすべて非信頼)
///   2. `reviewer_id`が空でないこと(誰が承認したか分からないレビューは
///      信頼しない——レガシー移行行が`reviewer_id: None`のまま残っている
///      のはこのため)
///   3. `authorization_level`が空でないこと(承認者が「どんな資格で」
///      承認したかを表明していること——身元だけでは足りない)
///   4. 期限切れでないこと(`expires_at_unix`が過去なら無効)
///   5. `dataset_version`が検証対象のリリースタグと一致すること
///      ("no drift" — 別リリースに対して行われたレビューを、確認なしに
///      今のリリースへ横流ししない)
pub fn is_authenticated_accept(decision: &ReviewDecision, now_unix: i64, expected_release_tag: &str) -> bool {
    let trust_affirming = matches!(decision.decision, ReviewOutcome::Accept | ReviewOutcome::Supersede);
    let has_reviewer = decision.reviewer_id.as_deref().is_some_and(|s| !s.trim().is_empty());
    let has_authorization = decision.authorization_level.as_deref().is_some_and(|s| !s.trim().is_empty());
    let not_expired = decision.expires_at_unix.is_none_or(|exp| exp > now_unix);
    let release_matches = decision.dataset_version.as_deref() == Some(expected_release_tag);
    trust_affirming && has_reviewer && has_authorization && not_expired && release_matches
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        EpistemicState, EvidenceKind, NewEvidence, NewRelationAssertion, NewRelease, NewSourceRecord, RelationKind,
    };

    fn setup_assertion(prov: &ProvenanceStore) -> AssertionId {
        let release_id = prov
            .get_or_insert_release(&NewRelease {
                tag: "test-release".into(),
                git_commit: None,
                generated_at_unix: 1000,
                notes: None,
            })
            .unwrap();
        let source = prov
            .get_or_insert_source_record(&NewSourceRecord {
                provider: "test".into(),
                provider_id: "src-1".into(),
                provider_revision: None,
                retrieved_at_unix: None,
                content_hash: None,
                licence: None,
                attribution: None,
                raw_payload_uri: None,
                adapter_name: "test".into(),
                adapter_version: "1".into(),
                parser_version: None,
                reproducibility_json: None,
            })
            .unwrap();
        let assertion = prov
            .insert_assertion(&NewRelationAssertion {
                subject_ref: "concept:a".into(),
                predicate: RelationKind::EquivalentTo,
                object_ref: "concept:b".into(),
                epistemic_state: EpistemicState::Reviewed,
                score: None,
                policy_version: None,
                created_by_run_id: None,
                supersedes_id: None,
                release_id,
                legacy_ref: Some("test:1".into()),
            })
            .unwrap();
        prov.insert_evidence(&NewEvidence {
            assertion_id: assertion,
            source_record_id: source,
            locator: None,
            evidence_kind: EvidenceKind::ReviewerNote,
            extractor_or_model: None,
            version: None,
            input_hash: None,
            output_hash: None,
            metric_name: None,
            metric_value: None,
            dependency_origin: None,
            external_classification: None,
        })
        .unwrap();
        assertion
    }

    #[test]
    fn accept_with_reviewer_and_authorization_and_matching_release_is_authenticated() {
        let prov = ProvenanceStore::open_in_memory().unwrap();
        let assertion = setup_assertion(&prov);
        prov.insert_review_decision(&NewReviewDecision {
            assertion_id: assertion,
            decision: ReviewOutcome::Accept,
            reviewer_id: Some("alice".into()),
            authorization_level: Some("maintainer".into()),
            scope: None,
            rationale: Some("checked by hand".into()),
            decided_at_unix: 1000,
            dataset_version: Some("test-release".into()),
            expires_at_unix: None,
            supersedes_review_id: None,
        })
        .unwrap();
        let effective = prov.effective_review_decision(assertion).unwrap().unwrap();
        assert!(is_authenticated_accept(&effective, 2000, "test-release"));
    }

    #[test]
    fn anonymous_accept_is_not_authenticated() {
        let prov = ProvenanceStore::open_in_memory().unwrap();
        let assertion = setup_assertion(&prov);
        prov.insert_review_decision(&NewReviewDecision {
            assertion_id: assertion,
            decision: ReviewOutcome::Accept,
            reviewer_id: None,
            authorization_level: None,
            scope: None,
            rationale: None,
            decided_at_unix: 1000,
            dataset_version: Some("test-release".into()),
            expires_at_unix: None,
            supersedes_review_id: None,
        })
        .unwrap();
        let effective = prov.effective_review_decision(assertion).unwrap().unwrap();
        assert!(!is_authenticated_accept(&effective, 2000, "test-release"));
    }

    #[test]
    fn revoke_after_accept_withdraws_trust() {
        let prov = ProvenanceStore::open_in_memory().unwrap();
        let assertion = setup_assertion(&prov);
        let accept_id = prov
            .insert_review_decision(&NewReviewDecision {
                assertion_id: assertion,
                decision: ReviewOutcome::Accept,
                reviewer_id: Some("alice".into()),
                authorization_level: Some("maintainer".into()),
                scope: None,
                rationale: None,
                decided_at_unix: 1000,
                dataset_version: Some("test-release".into()),
                expires_at_unix: None,
                supersedes_review_id: None,
            })
            .unwrap();
        prov.insert_review_decision(&NewReviewDecision {
            assertion_id: assertion,
            decision: ReviewOutcome::Revoke,
            reviewer_id: Some("bob".into()),
            authorization_level: Some("maintainer".into()),
            scope: None,
            rationale: Some("found a counterexample".into()),
            decided_at_unix: 2000,
            dataset_version: Some("test-release".into()),
            expires_at_unix: None,
            supersedes_review_id: Some(accept_id),
        })
        .unwrap();
        let effective = prov.effective_review_decision(assertion).unwrap().unwrap();
        assert_eq!(effective.decision, ReviewOutcome::Revoke);
        assert!(!is_authenticated_accept(&effective, 3000, "test-release"));
    }

    #[test]
    fn expired_accept_is_not_authenticated() {
        let prov = ProvenanceStore::open_in_memory().unwrap();
        let assertion = setup_assertion(&prov);
        prov.insert_review_decision(&NewReviewDecision {
            assertion_id: assertion,
            decision: ReviewOutcome::Accept,
            reviewer_id: Some("alice".into()),
            authorization_level: Some("maintainer".into()),
            scope: None,
            rationale: None,
            decided_at_unix: 1000,
            dataset_version: Some("test-release".into()),
            expires_at_unix: Some(1500),
            supersedes_review_id: None,
        })
        .unwrap();
        let effective = prov.effective_review_decision(assertion).unwrap().unwrap();
        assert!(!is_authenticated_accept(&effective, 2000, "test-release"));
        assert!(is_authenticated_accept(&effective, 1200, "test-release"));
    }

    #[test]
    fn accept_from_a_different_release_does_not_carry_over() {
        let prov = ProvenanceStore::open_in_memory().unwrap();
        let assertion = setup_assertion(&prov);
        prov.insert_review_decision(&NewReviewDecision {
            assertion_id: assertion,
            decision: ReviewOutcome::Accept,
            reviewer_id: Some("alice".into()),
            authorization_level: Some("maintainer".into()),
            scope: None,
            rationale: None,
            decided_at_unix: 1000,
            dataset_version: Some("old-release".into()),
            expires_at_unix: None,
            supersedes_review_id: None,
        })
        .unwrap();
        let effective = prov.effective_review_decision(assertion).unwrap().unwrap();
        assert!(!is_authenticated_accept(&effective, 2000, "test-release"));
    }
}

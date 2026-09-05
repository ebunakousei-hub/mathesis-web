use crate::model::{EntityKind, EpistemicState, RelationKind};

pub const SOURCE_MAPPING_POLICY_VERSION: &str = "mathesis-source-mapping-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraversalPolicy {
    Excluded,
    VisibleOnly,
    DefaultTraversal,
    FormalOnly,
}

impl TraversalPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Excluded => "excluded",
            Self::VisibleOnly => "visible_only",
            Self::DefaultTraversal => "default_traversal",
            Self::FormalOnly => "formal_only",
        }
    }
}

/// Publication/traversal policy is deliberately separate from epistemic state.
/// A reviewed semantic claim is not automatically a formal dependency.
pub fn traversal_policy(predicate: RelationKind, state: EpistemicState) -> TraversalPolicy {
    if state == EpistemicState::Rejected {
        return TraversalPolicy::Excluded;
    }
    match predicate {
        RelationKind::DependsOn | RelationKind::Cites | RelationKind::Imports => {
            if matches!(state, EpistemicState::Observed | EpistemicState::Verified) {
                TraversalPolicy::DefaultTraversal
            } else {
                TraversalPolicy::VisibleOnly
            }
        }
        RelationKind::Implies => {
            if matches!(state, EpistemicState::Verified) {
                TraversalPolicy::DefaultTraversal
            } else if state == EpistemicState::Reviewed {
                TraversalPolicy::FormalOnly
            } else {
                TraversalPolicy::VisibleOnly
            }
        }
        RelationKind::Specializes
        | RelationKind::Generalizes
        | RelationKind::EquivalentTo
        | RelationKind::RelatedTo
        | RelationKind::UsesConcept => {
            if matches!(state, EpistemicState::Reviewed | EpistemicState::Verified) {
                TraversalPolicy::DefaultTraversal
            } else {
                TraversalPolicy::VisibleOnly
            }
        }
    }
}

/// Declarative relation policy used by inserts and release verification.
pub fn valid_entity_kinds(predicate: RelationKind, subject: EntityKind, object: EntityKind) -> bool {
    match predicate {
        RelationKind::DependsOn => subject == EntityKind::Judgment && object == EntityKind::Judgment,
        RelationKind::Cites => subject == EntityKind::Paper && object == EntityKind::Paper,
        RelationKind::Implies
        | RelationKind::Specializes
        | RelationKind::Generalizes
        | RelationKind::EquivalentTo => {
            (subject == EntityKind::Judgment && object == EntityKind::Judgment)
                || (subject == EntityKind::Concept && object == EntityKind::Concept)
        }
        RelationKind::UsesConcept => {
            object == EntityKind::Concept
                && matches!(subject, EntityKind::Judgment | EntityKind::Paper)
        }
        RelationKind::Imports | RelationKind::RelatedTo => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_relations_are_not_default_traversal_before_review() {
        assert_eq!(
            traversal_policy(RelationKind::Implies, EpistemicState::Proposed),
            TraversalPolicy::VisibleOnly
        );
        assert_eq!(
            traversal_policy(RelationKind::Implies, EpistemicState::Reviewed),
            TraversalPolicy::FormalOnly
        );
        assert_eq!(
            traversal_policy(RelationKind::Implies, EpistemicState::Verified),
            TraversalPolicy::DefaultTraversal
        );
    }

    #[test]
    fn relation_schema_rejects_cross_domain_claims() {
        assert!(!valid_entity_kinds(RelationKind::Specializes, EntityKind::Paper, EntityKind::Paper));
        assert!(!valid_entity_kinds(RelationKind::Cites, EntityKind::Concept, EntityKind::Concept));
        assert!(valid_entity_kinds(RelationKind::UsesConcept, EntityKind::Judgment, EntityKind::Concept));
    }
}

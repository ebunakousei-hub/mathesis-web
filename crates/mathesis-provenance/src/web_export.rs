//! P2, Increment 1 (`docs/P2_STATUS.md`): the running Web app's edge-level
//! read model — judgment dependencies, morphisms (implies/specializes/
//! generalizes/equivalent_to between judgments), and typed concept
//! relations — is generated directly from this crate's `ProvenanceStore`
//! (`RelationAssertion` + `Evidence` + `ReviewDecision`), not from
//! `mathesis-graph`/`mathesis-taxonomy`'s own exporters plus a thin
//! "provenance sidecar" the client had to join by hand. Those two crates'
//! own `--export`/`export` commands still produce `judgments.json` (judgment
//! statements, papers — node data outside this crate's scope) and
//! `taxonomy.json` (concepts/clusters/search index), but no longer own the
//! *edges* the app displays.
//!
//! Enumeration reads `list_assertions_for_release` alone — nothing here opens
//! the original `mathesis-graph`/`mathesis-taxonomy` SQLite databases.
//! `subject_ref`/`object_ref`'s `"kind:id"` tag (`docs/DATA_DICTIONARY.md`
//! design decision 2) says which domain (dependency/morphism/relation) an
//! assertion belongs to. Fields the legacy exporters used to read straight
//! off `mathesis-graph`'s `morphisms` table or `mathesis-taxonomy`'s
//! `RelationStatus` (`kind`/`origin`/`status`/`rationale`/`confidence`) are
//! instead reconstructed from the assertion's own `Evidence`/
//! `ReviewDecision` rows, so the web read model can never disagree with what
//! `mathesis-provenance verify` has already checked.

use crate::assertion_export::{evidence_details_for, review_decision_details_for};
use crate::model::{EpistemicState, RelationAssertion, RelationKind};
use crate::relation_policy::traversal_policy;
use crate::store::ProvenanceStore;
use serde::Serialize;

pub const WEB_EXPORT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyEdge {
    pub assertion_id: i64,
    pub from: i64,
    pub to: i64,
    /// P5, Item 1（`docs/P5_PLAN.md`）: `relation_policy::traversal_policy`の
    /// 文字列表現をそのまま辺へ持たせる——クライアントが既定トラバース対象を
    /// 判断するのに`assertions.json`（4MB超）をまるごと読み込まずに済む。
    pub traversal_policy: String,
    /// Priority 2, step 1（ユーザー指示 2026-09-08）:
    /// `"checker-derived"`(Lean elaboratorの実行結果、
    /// `lean_manifest_adapter.rs`)か`"text-extracted"`
    /// (`mathesis-importer`の識別子名一致)かを、`MorphismEdge.origin`と
    /// 同じくEvidenceの`evidence_kind`から復元する——実データでは今のところ
    /// ほぼ全件`text-extracted`(`depends_on`は基本`extracted`)だが、
    /// Lean manifestを取り込んだ分だけ`checker-derived`(`observed`)になる。
    pub origin: String,
}

/// `mathesis-graph::export::ExportedMorphism`と同じ4フィールド
/// （`kind`/`origin`/`status`/`rationale`）を、`morphisms`テーブルからでは
/// なく`RelationAssertion`+`Evidence`+`ReviewDecision`から再構成する。
/// `id`は旧来の`morphisms.id`ではなくこのassertionのid——1射につき
/// assertionが必ず1件（`legacy_adapter::import_graph`）なので識別子として
/// 完全に代用でき、`web/src/lineage.ts`が別途持っていた
/// `morphismProvenance`という2つ目のidマップが丸ごと要らなくなる。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MorphismEdge {
    pub id: i64,
    pub src: i64,
    pub dst: i64,
    pub kind: String,
    pub origin: String,
    pub status: String,
    pub rationale: Option<String>,
    /// `DependencyEdge::traversal_policy`と同じ理由・同じ値の語彙。
    pub traversal_policy: String,
}

/// `mathesis-taxonomy::export::RelationsExport`の1行相当。`confidence`は
/// Confirmedにしか存在しない実測値——Groundedは`None`（旧
/// `taxonomy.relations.json`が出していた固定1.0のプレースホルダは、ここでは
/// 出さない。`relations.rs::merge()`のGrounded確定時に代入されていた値で
/// あって測定値ではなかった、という外部レビュー2026-09-05の指摘そのもの）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationEdge {
    pub assertion_id: i64,
    pub subject: String,
    pub object: String,
    pub kind: String,
    pub status: String,
    pub confidence: Option<f64>,
    pub evidence_sentence: String,
    pub evidence_arxiv_id: String,
    /// P6.3（`docs/P6_3_STATUS.md`）: `DependencyEdge`/`MorphismEdge`と同じ
    /// 語彙——このクレートを追加するまで`RelationEdge`だけこのフィールドを
    /// 持っていなかった(概念関係には「本人確認済みレビューで信頼を得る」
    /// 経路が無かったため、フィールド自体に意味が無かった)。今は
    /// `promote-review`で意味的関係もreviewed/verifiedへ昇格できるため、
    /// 他の2種と揃える。
    pub traversal_policy: String,
}

fn morphism_kind_str(k: RelationKind) -> Option<&'static str> {
    Some(match k {
        RelationKind::Implies => "implication",
        RelationKind::Specializes => "specialization",
        RelationKind::Generalizes => "generalization",
        RelationKind::EquivalentTo => "equivalence",
        _ => return None,
    })
}

fn relation_kind_str(k: RelationKind) -> Option<&'static str> {
    Some(match k {
        RelationKind::Specializes => "specialization_of",
        RelationKind::EquivalentTo => "equivalent_to",
        _ => return None,
    })
}

/// `judgment_dependencies` -> depends_on。構造的な機械的事実そのままで、
/// 状態は常に`extracted`（`legacy_adapter::import_graph`参照）——ここに
/// Evidence由来の追加解釈は要らない。
///
/// P5, Item 2 step 4（`docs/P5_PLAN.md`）: `from`/`to`は`subject_ref`の
/// 文字列プレフィックスを剥がすのではなく、`subject_entity_id`(FK)を
/// `judgment_id_for_entity`で逆引きして得る——FKが無い(=カタログ未解決)
/// assertionは出さない。`verify-release`が通ったDBでは
/// `subject_entity_id`と`subject_ref`の解決結果は一致することが保証
/// 済み（`entity_endpoint_drift`検査）なので実データでの結果は変わらないが、
/// 「何が出典か」の主従が入れ替わる——文字列は表示用、FKが真実。
pub fn build_dependency_edges(prov: &ProvenanceStore, assertions: &[RelationAssertion]) -> anyhow::Result<Vec<DependencyEdge>> {
    let mut out = Vec::new();
    for a in assertions {
        if a.predicate != RelationKind::DependsOn {
            continue;
        }
        let Some(subject_entity_id) = a.subject_entity_id else { continue };
        let Some(object_entity_id) = a.object_entity_id else { continue };
        let Some(from) = prov.judgment_id_for_entity(subject_entity_id)? else { continue };
        let Some(to) = prov.judgment_id_for_entity(object_entity_id)? else { continue };
        let evidence = evidence_details_for(prov, a.id)?;
        let origin = if evidence.iter().any(|e| e.evidence_kind == "formal_export") {
            "checker-derived"
        } else {
            "text-extracted"
        };
        out.push(DependencyEdge {
            assertion_id: a.id.0,
            from,
            to,
            traversal_policy: traversal_policy(a.predicate, a.epistemic_state).as_str().to_string(),
            origin: origin.to_string(),
        });
    }
    Ok(out)
}

/// 射（implies/specializes/generalizes/equivalent_to、judgment同士）。
/// `origin`は起源となったEvidence行の`evidence_kind`
/// （`reviewer_note`=manual、`model_output`=heuristic）から、`status`は
/// `epistemic_state`と`ReviewDecision`の有無から復元する
/// （`docs/DATA_DICTIONARY.md`「Resolved decisions #4」——legacy
/// `Accepted`は`epistemic_state: proposed`のまま、`ReviewDecision{accept}`
/// が別途あるかどうかで見分ける）。
pub fn build_morphism_edges(
    prov: &ProvenanceStore,
    assertions: &[RelationAssertion],
    release_tag: &str,
    now_unix: i64,
) -> anyhow::Result<Vec<MorphismEdge>> {
    let mut out = Vec::new();
    for a in assertions {
        let Some(kind) = morphism_kind_str(a.predicate) else { continue };
        // `build_dependency_edges`と同じ理由でFK経由に切り替える。
        let Some(subject_entity_id) = a.subject_entity_id else { continue };
        let Some(object_entity_id) = a.object_entity_id else { continue };
        let Some(src) = prov.judgment_id_for_entity(subject_entity_id)? else { continue };
        let Some(dst) = prov.judgment_id_for_entity(object_entity_id)? else { continue };
        let evidence = evidence_details_for(prov, a.id)?;
        // P6.3: `status`は昔ながらの「acceptがどこかに1件でもあれば
        // accepted」のまま——本人確認済み/失効/リリース一致まで見る厳密な
        // 判定(`is_current_authenticated_accept`)はリリースゲートと
        // provenanceパネルの仕事で、この表示専用フィールドの意味は変えない。
        let review_decisions = review_decision_details_for(prov, a.id, release_tag, now_unix)?;
        let origin_evidence =
            evidence.iter().find(|e| e.evidence_kind == "reviewer_note" || e.evidence_kind == "model_output");
        let origin = match origin_evidence.map(|e| e.evidence_kind.as_str()) {
            Some("reviewer_note") => "manual",
            _ => "heuristic",
        };
        let rationale = origin_evidence.and_then(|e| e.locator.clone());
        let status = if a.epistemic_state == EpistemicState::Rejected {
            "rejected"
        } else if review_decisions.iter().any(|r| r.decision == "accept") {
            "accepted"
        } else {
            "proposed"
        };
        out.push(MorphismEdge {
            id: a.id.0,
            src,
            dst,
            kind: kind.to_string(),
            origin: origin.to_string(),
            status: status.to_string(),
            rationale,
            traversal_policy: traversal_policy(a.predicate, a.epistemic_state).as_str().to_string(),
        });
    }
    Ok(out)
}

/// 型付き概念関係（specialization_of/equivalent_to）。根拠文
/// （`source_span`のEvidence）を持たないassertion（distributionalのみの
/// Proposed）は出さない——`mathesis-taxonomy::export::RelationsExport`と
/// 同じ「読者が自分の目で確かめられる根拠がある行だけを見せる」方針
/// （実測精度約50%、`relations.rs`冒頭コメント参照）。
///
/// P6.3: `reviewer_note`のEvidenceも同じ理由で受理する——
/// `promote-review`で作る本人確認済みレビュー由来のassertionは
/// (`store.rs`のSCHEMAコメントが明記する唯一の例外どおり)`source_span`を
/// 持たず、代わりにレビューの根拠(rationale)を`reviewer_note`として持つ。
/// ここで弾くと、レビューで信頼を得たはずの意味的関係がリリースゲートは
/// 通るのに`relations.json`に一切現れない、という矛盾が起きる。
///
/// P5, Item 2 step 4: `subject`/`object`は`subject_ref`の文字列（表記ゆれの
/// ままのことがある）ではなく、`subject_entity_id`が指すエンティティの
/// `display_label`(=Entity Resolutionが選んだ代表表記)を出す。実データで
/// 突き合わせて確認した実例（1051件中3件）: `subject_ref`が
/// "pull back"/"one dimensional"/"dg module"という別表記のまま記録されて
/// いたのに対し、代表表記は"pull-back"/"one-dimensional"/"dg-modules"——
/// `web/src/dynamicTaxonomy.ts`の概念詳細は検索索引の代表表記（`h.phrase`）
/// をキーに`relations.json`を引く(`expandRelations`)ため、旧来のasIs出力
/// ではこの3件が**該当の概念ページ上で一度も表示されていなかった**
/// (別表記のキーに埋もれて孤立していた) ——これは仕様ではなく実バグで、
/// この切り替えが直す。
pub fn build_relation_edges(prov: &ProvenanceStore, assertions: &[RelationAssertion]) -> anyhow::Result<Vec<RelationEdge>> {
    let mut out = Vec::new();
    for a in assertions {
        let Some(kind) = relation_kind_str(a.predicate) else { continue };
        let Some(subject_entity_id) = a.subject_entity_id else { continue };
        let Some(object_entity_id) = a.object_entity_id else { continue };
        let Some(subject_entity) = prov.try_get_entity(subject_entity_id)? else { continue };
        let Some(object_entity) = prov.try_get_entity(object_entity_id)? else { continue };
        // P6.3で判明: `Specializes`/`EquivalentTo`/`Generalizes`はjudgment同士
        // (射)とconcept同士(型付き概念関係)の両方で使われる共有語彙
        // （`relation_policy::valid_entity_kinds`）——`reviewer_note`証拠を
        // 受理するようになった結果、`build_morphism_edges`側のjudgment同士の
        // assertionが種別チェック無しにここへも紛れ込む実バグを、対抗
        // フィクスチャ(`every_generated_edge_resolves_to_a_consistent_assertion`)
        // が検出した。両端が本当にConcept種別のエンティティであることを
        // 明示的に確認する。
        if subject_entity.kind != crate::model::EntityKind::Concept || object_entity.kind != crate::model::EntityKind::Concept {
            continue;
        }
        let subject = subject_entity.display_label;
        let object = object_entity.display_label;
        let evidence = evidence_details_for(prov, a.id)?;
        let Some(grounding) = evidence
            .iter()
            .find(|e| e.evidence_kind == "source_span" || e.evidence_kind == "reviewer_note")
        else {
            continue;
        };
        let model_output = evidence.iter().find(|e| e.evidence_kind == "model_output");
        let status = if grounding.evidence_kind == "reviewer_note" {
            "reviewed".to_string()
        } else if model_output.is_some() {
            "confirmed".to_string()
        } else {
            "grounded".to_string()
        };
        out.push(RelationEdge {
            assertion_id: a.id.0,
            subject,
            object,
            kind: kind.to_string(),
            status,
            confidence: model_output.and_then(|e| e.metric_value),
            evidence_sentence: grounding.locator.clone().unwrap_or_default(),
            evidence_arxiv_id: if grounding.source_provider == "arxiv" {
                grounding.source_provider_id.clone()
            } else {
                String::new()
            },
            traversal_policy: traversal_policy(a.predicate, a.epistemic_state).as_str().to_string(),
        });
    }
    // Confirmedを先に、同じstatus内は確信度の降順——旧`RelationsExport`と
    // 同じ表示順（利用者が最初に見るものが最も裏付けの強いものになるように）。
    out.sort_by(|x, y| {
        // P6.3: 本人確認済みレビューはLLM/検出器の"confirmed"より上位——
        // 人間が実際に読んで判断した根拠のほうが強い。
        let rank = |s: &str| match s {
            "reviewed" => 0,
            "confirmed" => 1,
            _ => 2,
        };
        rank(&x.status)
            .cmp(&rank(&y.status))
            .then_with(|| y.confidence.partial_cmp(&x.confidence).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| x.subject.cmp(&y.subject))
    });
    Ok(out)
}

#[derive(Debug, Default)]
pub struct WebExport {
    pub dependencies: Vec<DependencyEdge>,
    pub morphisms: Vec<MorphismEdge>,
    pub relations: Vec<RelationEdge>,
}

/// このリリースの`ProvenanceStore`**だけ**を入口に、Web版が今表示している
/// 3種の辺すべてを組み立てる。
///
/// P5, Item 2 step 4以降、3種すべて`subject_entity_id`/`object_entity_id`
/// (FK)を出典に使う——カタログが空のDB(`build-catalog`未実行)では
/// どのassertionもFKを持たないため、3種とも0件になる。これを気づかれない
/// まま空の`web-export`を書き出してしまわないよう、ここで早期に失敗する
/// （`build-catalog`が今や`web-export`の事実上の前提工程になったことを、
/// 黙って壊れた出力を出すのではなく明示的なエラーとして伝える）。
pub fn build_web_export(prov: &ProvenanceStore, release: crate::model::ReleaseId) -> anyhow::Result<WebExport> {
    if prov.entity_count()? == 0 {
        anyhow::bail!(
            "no entity catalog present (entity_count == 0) — run `mathesis-provenance build-catalog` \
             before `web-export`; edges now resolve through subject_entity_id/object_entity_id, not string parsing"
        );
    }
    let release_tag = prov.get_release(release)?.tag;
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let assertions = prov.list_assertions_for_release(release)?;
    Ok(WebExport {
        dependencies: build_dependency_edges(prov, &assertions)?,
        morphisms: build_morphism_edges(prov, &assertions, &release_tag, now_unix)?,
        relations: build_relation_edges(prov, &assertions)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        EvidenceKind, NewEvidence, NewRelationAssertion, NewRelease, NewReviewDecision, NewSourceRecord, ReviewOutcome,
    };
    use crate::store::ProvenanceStore;

    use crate::model::{EntityKind, NewEntity};

    /// P5, Item 2 step 4: `build_dependency_edges`/`build_morphism_edges`は
    /// 今や`subject_entity_id`/`object_entity_id`(FK)経由でしか判断idを
    /// 引けない——テストのassertionを挿入する前に、判断エンティティを
    /// カタログへ先に登録しておく（実運用の`import-legacy`→
    /// `build-catalog`の順序と同じ）。
    fn ensure_judgment(prov: &ProvenanceStore, id: i64) {
        prov.get_or_insert_entity(
            &NewEntity { kind: EntityKind::Judgment, display_label: format!("j{id}"), source_record_id: None },
            &format!("judgment:{id}"),
        )
        .unwrap();
    }

    fn ensure_concept(prov: &ProvenanceStore, phrase: &str) {
        prov.get_or_insert_entity(
            &NewEntity { kind: EntityKind::Concept, display_label: phrase.into(), source_record_id: None },
            &format!("concept:{phrase}"),
        )
        .unwrap();
    }

    fn setup() -> (ProvenanceStore, crate::model::ReleaseId, crate::model::SourceRecordId) {
        let prov = ProvenanceStore::open_in_memory().unwrap();
        let release = prov
            .get_or_insert_release(&NewRelease {
                tag: "test".into(),
                git_commit: None,
                generated_at_unix: 0,
                notes: None,
            })
            .unwrap();
        let source = prov
            .get_or_insert_source_record(&NewSourceRecord {
                provider: "arxiv".into(),
                provider_id: "math/0001".into(),
                provider_revision: None,
                retrieved_at_unix: None,
                content_hash: None,
                licence: None,
                attribution: None,
                raw_payload_uri: None,
                adapter_name: "test".into(),
                adapter_version: "0".into(),
                parser_version: None,
                reproducibility_json: None,
            })
            .unwrap();
        (prov, release, source)
    }

    #[test]
    fn dependency_edges_reconstruct_from_to_from_refs() {
        let (prov, release, source) = setup();
        ensure_judgment(&prov, 5);
        ensure_judgment(&prov, 2);
        let a = prov
            .insert_assertion(&NewRelationAssertion {
                subject_ref: "judgment:5".into(),
                predicate: RelationKind::DependsOn,
                object_ref: "judgment:2".into(),
                epistemic_state: EpistemicState::Extracted,
                score: None,
                policy_version: None,
                created_by_run_id: None,
                supersedes_id: None,
                release_id: release,
                legacy_ref: Some("judgment_dependency:5:2".into()),
            })
            .unwrap();
        prov.insert_evidence(&NewEvidence {
            assertion_id: a,
            source_record_id: source,
            locator: Some("foo.lean:1".into()),
            evidence_kind: EvidenceKind::SourceSpan,
            extractor_or_model: None,
            version: None,
            input_hash: None,
            output_hash: None,
            metric_name: None,
            metric_value: None,
            dependency_origin: None,
        })
        .unwrap();

        let assertions = prov.list_assertions_for_release(release).unwrap();
        let deps = build_dependency_edges(&prov, &assertions).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].from, 5);
        assert_eq!(deps[0].to, 2);
        assert_eq!(deps[0].assertion_id, a.0);
        assert_eq!(deps[0].origin, "text-extracted", "SourceSpan由来はtext-extracted");
    }

    /// Priority 2, step 1（ユーザー指示 2026-09-08）:
    /// `lean_manifest_adapter`が作る`FormalExport`のEvidenceを持つ辺は
    /// `"checker-derived"`と区別されるべき——同じ`depends_on`述語でも、
    /// テキスト抽出とLean elaborator由来をクライアントが見分けられること。
    #[test]
    fn dependency_edge_origin_distinguishes_checker_derived_from_text_extracted() {
        let (prov, release, source) = setup();
        ensure_judgment(&prov, 1);
        ensure_judgment(&prov, 2);
        let a = prov
            .insert_assertion(&NewRelationAssertion {
                subject_ref: "judgment:1".into(),
                predicate: RelationKind::DependsOn,
                object_ref: "judgment:2".into(),
                epistemic_state: EpistemicState::Observed,
                score: None,
                policy_version: None,
                created_by_run_id: None,
                supersedes_id: None,
                release_id: release,
                legacy_ref: Some("lean-manifest:1:2".into()),
            })
            .unwrap();
        prov.insert_evidence(&NewEvidence {
            assertion_id: a,
            source_record_id: source,
            locator: Some("Test.thm_a -> thm_b".into()),
            evidence_kind: EvidenceKind::FormalExport,
            extractor_or_model: Some("mathesis-provenance-lean-manifest-adapter".into()),
            version: None,
            input_hash: None,
            output_hash: None,
            metric_name: None,
            metric_value: None,
            dependency_origin: Some("body".into()),
        })
        .unwrap();

        let assertions = prov.list_assertions_for_release(release).unwrap();
        let deps = build_dependency_edges(&prov, &assertions).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].origin, "checker-derived");
        assert_eq!(deps[0].traversal_policy, "default_traversal", "observedなdepends_onは既定トラバース対象になるべき");
    }

    /// P5, Item 1（`docs/P5_PLAN.md`）: real dataの`depends_on`は今のところ
    /// 全件`extracted`（`observed`ではない——`docs/DATA_DICTIONARY.md`の決定）
    /// なので`visible_only`になるべき。`observed`に上がれば`default_traversal`
    /// に変わることも同じテストで確認する——クライアントが読む語彙が
    /// `relation_policy::traversal_policy`とズレないことの契約テスト。
    #[test]
    fn dependency_edge_carries_the_traversal_policy_computed_from_its_epistemic_state() {
        let (prov, release, source) = setup();
        ensure_judgment(&prov, 1);
        ensure_judgment(&prov, 2);
        ensure_judgment(&prov, 3);
        ensure_judgment(&prov, 4);
        let extracted = prov
            .insert_assertion(&NewRelationAssertion {
                subject_ref: "judgment:1".into(),
                predicate: RelationKind::DependsOn,
                object_ref: "judgment:2".into(),
                epistemic_state: EpistemicState::Extracted,
                score: None,
                policy_version: None,
                created_by_run_id: None,
                supersedes_id: None,
                release_id: release,
                legacy_ref: Some("d1".into()),
            })
            .unwrap();
        let observed = prov
            .insert_assertion(&NewRelationAssertion {
                subject_ref: "judgment:3".into(),
                predicate: RelationKind::DependsOn,
                object_ref: "judgment:4".into(),
                epistemic_state: EpistemicState::Observed,
                score: None,
                policy_version: None,
                created_by_run_id: None,
                supersedes_id: None,
                release_id: release,
                legacy_ref: Some("d2".into()),
            })
            .unwrap();
        let _ = source;

        let assertions = prov.list_assertions_for_release(release).unwrap();
        let deps = build_dependency_edges(&prov, &assertions).unwrap();
        let by_id = |id: i64| deps.iter().find(|d| d.assertion_id == id).unwrap();
        assert_eq!(by_id(extracted.0).traversal_policy, "visible_only");
        assert_eq!(by_id(observed.0).traversal_policy, "default_traversal");
    }

    fn insert_morphism(
        prov: &ProvenanceStore,
        release: crate::model::ReleaseId,
        source: crate::model::SourceRecordId,
        predicate: RelationKind,
        epistemic_state: EpistemicState,
        evidence_kind: EvidenceKind,
        rationale: Option<&str>,
        accepted: bool,
    ) -> i64 {
        ensure_judgment(prov, 1);
        ensure_judgment(prov, 2);
        let a = prov
            .insert_assertion(&NewRelationAssertion {
                subject_ref: "judgment:1".into(),
                predicate,
                object_ref: "judgment:2".into(),
                epistemic_state,
                score: None,
                policy_version: None,
                created_by_run_id: None,
                supersedes_id: None,
                release_id: release,
                legacy_ref: Some(format!("morphism:{}", predicate.as_str())),
            })
            .unwrap();
        prov.insert_evidence(&NewEvidence {
            assertion_id: a,
            source_record_id: source,
            locator: rationale.map(str::to_string),
            evidence_kind,
            extractor_or_model: None,
            version: None,
            input_hash: None,
            output_hash: None,
            metric_name: None,
            metric_value: None,
            dependency_origin: None,
        })
        .unwrap();
        if accepted {
            prov.insert_review_decision(&NewReviewDecision {
                assertion_id: a,
                decision: ReviewOutcome::Accept,
                reviewer_id: None,
                authorization_level: None,
                scope: None,
                rationale: rationale.map(str::to_string),
                decided_at_unix: 0,
                dataset_version: None,
                expires_at_unix: None,
                supersedes_review_id: None,
            })
            .unwrap();
        }
        a.0
    }

    #[test]
    fn morphism_status_distinguishes_proposed_accepted_rejected_from_evidence_and_review() {
        let (prov, release, source) = setup();
        insert_morphism(&prov, release, source, RelationKind::Specializes, EpistemicState::Proposed, EvidenceKind::ModelOutput, None, false);
        insert_morphism(
            &prov,
            release,
            source,
            RelationKind::Implies,
            EpistemicState::Proposed,
            EvidenceKind::ReviewerNote,
            Some("human said so"),
            true,
        );
        insert_morphism(&prov, release, source, RelationKind::Generalizes, EpistemicState::Rejected, EvidenceKind::ModelOutput, None, false);

        let assertions = prov.list_assertions_for_release(release).unwrap();
        let morphisms = build_morphism_edges(&prov, &assertions, "t", 0).unwrap();
        assert_eq!(morphisms.len(), 3);

        let by_kind = |k: &str| morphisms.iter().find(|m| m.kind == k).unwrap();
        let heuristic_proposed = by_kind("specialization");
        assert_eq!(heuristic_proposed.status, "proposed");
        assert_eq!(heuristic_proposed.origin, "heuristic");

        let manual_accepted = by_kind("implication");
        assert_eq!(manual_accepted.status, "accepted");
        assert_eq!(manual_accepted.origin, "manual");
        assert_eq!(manual_accepted.rationale.as_deref(), Some("human said so"));

        let rejected = by_kind("generalization");
        assert_eq!(rejected.status, "rejected");
    }

    fn insert_relation(
        prov: &ProvenanceStore,
        release: crate::model::ReleaseId,
        source: crate::model::SourceRecordId,
        subject: &str,
        sentence: Option<&str>,
        metric_value: Option<f64>,
    ) -> i64 {
        ensure_concept(prov, subject);
        ensure_concept(prov, "broader");
        let a = prov
            .insert_assertion(&NewRelationAssertion {
                subject_ref: format!("concept:{subject}"),
                predicate: RelationKind::Specializes,
                object_ref: "concept:broader".into(),
                epistemic_state: EpistemicState::Extracted,
                score: None,
                policy_version: None,
                created_by_run_id: None,
                supersedes_id: None,
                release_id: release,
                legacy_ref: Some(format!("concept_relation:{subject}")),
            })
            .unwrap();
        if let Some(sentence) = sentence {
            prov.insert_evidence(&NewEvidence {
                assertion_id: a,
                source_record_id: source,
                locator: Some(sentence.to_string()),
                evidence_kind: EvidenceKind::SourceSpan,
                extractor_or_model: None,
                version: None,
                input_hash: None,
                output_hash: None,
                metric_name: None,
                metric_value: None,
                dependency_origin: None,
            })
            .unwrap();
        }
        if let Some(v) = metric_value {
            prov.insert_evidence(&NewEvidence {
                assertion_id: a,
                source_record_id: source,
                locator: None,
                evidence_kind: EvidenceKind::ModelOutput,
                extractor_or_model: None,
                version: None,
                input_hash: None,
                output_hash: None,
                metric_name: Some("invCL".into()),
                metric_value: Some(v),
                dependency_origin: None,
            })
            .unwrap();
        }
        a.0
    }

    #[test]
    fn relation_edges_exclude_proposed_and_never_fabricate_grounded_confidence() {
        let (prov, release, source) = setup();
        insert_relation(&prov, release, source, "confirmed-case", Some("a sentence"), Some(0.87));
        insert_relation(&prov, release, source, "grounded-case", Some("another sentence"), None);
        insert_relation(&prov, release, source, "proposed-case", None, Some(0.5));

        let assertions = prov.list_assertions_for_release(release).unwrap();
        let relations = build_relation_edges(&prov, &assertions).unwrap();

        assert_eq!(relations.len(), 2, "根拠文の無いProposedは出さない");
        let confirmed = relations.iter().find(|r| r.subject == "confirmed-case").unwrap();
        assert_eq!(confirmed.status, "confirmed");
        assert_eq!(confirmed.confidence, Some(0.87));

        let grounded = relations.iter().find(|r| r.subject == "grounded-case").unwrap();
        assert_eq!(grounded.status, "grounded");
        assert_eq!(grounded.confidence, None, "Groundedに1.0を捏造しない");

        assert_eq!(relations[0].status, "confirmed", "Confirmedを先に出す");
    }

    /// P1/P2安定化パス項目7: 「UI DTOとassertion detail exportは食い違えない」
    /// を、手で選んだ1件だけでなく**生成された全件**について機械的に確かめる。
    /// dependencies/morphisms/relationsを混ぜた現実的なフィクスチャに対し、
    /// 各エントリの`assertionId`が実在し、`build_web_export`が付けた
    /// `kind`/`origin`/`status`/`confidence`が、そのassertion自身の
    /// predicate/epistemic_state/Evidence/ReviewDecisionと矛盾しないことを
    /// 型ごとに横断して検査する。
    #[test]
    fn every_generated_edge_resolves_to_a_consistent_assertion() {
        let (prov, release, source) = setup();
        // 依存関係。
        ensure_judgment(&prov, 10);
        ensure_judgment(&prov, 11);
        let dep_a = prov
            .insert_assertion(&NewRelationAssertion {
                subject_ref: "judgment:10".into(),
                predicate: RelationKind::DependsOn,
                object_ref: "judgment:11".into(),
                epistemic_state: EpistemicState::Extracted,
                score: None,
                policy_version: None,
                created_by_run_id: None,
                supersedes_id: None,
                release_id: release,
                legacy_ref: Some("judgment_dependency:10:11".into()),
            })
            .unwrap();
        prov.insert_evidence(&NewEvidence {
            assertion_id: dep_a,
            source_record_id: source,
            locator: Some("f.lean:9".into()),
            evidence_kind: EvidenceKind::SourceSpan,
            extractor_or_model: None,
            version: None,
            input_hash: None,
            output_hash: None,
            metric_name: None,
            metric_value: None,
            dependency_origin: None,
        })
        .unwrap();
        // 射(未承認・承認済み・却下の3種)と関係(confirmed/grounded)を混ぜる。
        insert_morphism(&prov, release, source, RelationKind::Specializes, EpistemicState::Proposed, EvidenceKind::ModelOutput, None, false);
        insert_morphism(&prov, release, source, RelationKind::EquivalentTo, EpistemicState::Proposed, EvidenceKind::ReviewerNote, Some("r"), true);
        insert_morphism(&prov, release, source, RelationKind::Generalizes, EpistemicState::Rejected, EvidenceKind::ModelOutput, None, false);
        insert_relation(&prov, release, source, "sweep-confirmed", Some("s1"), Some(0.9));
        insert_relation(&prov, release, source, "sweep-grounded", Some("s2"), None);

        let assertions = prov.list_assertions_for_release(release).unwrap();
        let dependencies = build_dependency_edges(&prov, &assertions).unwrap();
        let morphisms = build_morphism_edges(&prov, &assertions, "t", 0).unwrap();
        let relations = build_relation_edges(&prov, &assertions).unwrap();
        assert_eq!(dependencies.len(), 1);
        assert_eq!(morphisms.len(), 3);
        assert_eq!(relations.len(), 2);

        for d in &dependencies {
            let assertion = prov.try_get_assertion(crate::model::AssertionId(d.assertion_id)).unwrap().expect("assertionId must resolve");
            assert_eq!(assertion.predicate, RelationKind::DependsOn);
            assert_eq!(assertion.subject_ref, format!("judgment:{}", d.from));
            assert_eq!(assertion.object_ref, format!("judgment:{}", d.to));
        }

        for m in &morphisms {
            let assertion = prov.try_get_assertion(crate::model::AssertionId(m.id)).unwrap().expect("assertionId must resolve");
            assert_eq!(morphism_kind_str(assertion.predicate), Some(m.kind.as_str()));
            let evidence = evidence_details_for(&prov, assertion.id).unwrap();
            let review = review_decision_details_for(&prov, assertion.id, "t", 0).unwrap();
            // originはEvidenceの種別から、statusはepistemic_state+ReviewDecisionから、
            // それぞれ独立に再計算しても`build_morphism_edges`の出力と一致するはず
            // ——ここがズレたら「DTOとdetail exportが食い違う」ことになる。
            let expected_origin = if evidence.iter().any(|e| e.evidence_kind == "reviewer_note") { "manual" } else { "heuristic" };
            assert_eq!(m.origin, expected_origin);
            let expected_status = if assertion.epistemic_state == EpistemicState::Rejected {
                "rejected"
            } else if review.iter().any(|r| r.decision == "accept") {
                "accepted"
            } else {
                "proposed"
            };
            assert_eq!(m.status, expected_status);
        }

        for r in &relations {
            let assertion = prov.try_get_assertion(crate::model::AssertionId(r.assertion_id)).unwrap().expect("assertionId must resolve");
            assert_eq!(relation_kind_str(assertion.predicate), Some(r.kind.as_str()));
            let evidence = evidence_details_for(&prov, assertion.id).unwrap();
            let has_model_output = evidence.iter().any(|e| e.evidence_kind == "model_output");
            assert_eq!(r.status, if has_model_output { "confirmed" } else { "grounded" });
            // confidenceが捏造されていないことの一般化: model_output Evidenceが
            // 無いなら確信度はNoneでなければならない(逆に有るならSome)。
            assert_eq!(r.confidence.is_some(), has_model_output);
        }
    }
}

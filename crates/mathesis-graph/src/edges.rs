//! 層3の射に対する GraphStore 操作（アノテーション、承認、商グラフ再構築）。

use crate::error::{GraphResult, ValidationError};
use crate::heuristics::{self, JudgmentLite};
use crate::model::{JudgmentId, JudgmentKind};
use crate::morphism::{
    EdgeOrigin, EdgeStatus, MorphismId, MorphismKind, MorphismRecord, NewMorphism,
};
use crate::proof::{analyze_dependencies, ProofTerm};
use crate::quotient::{self, QuotientGraph};
use crate::store::{GraphStore, Result};
use rusqlite::{params, OptionalExtension};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

impl GraphStore {
    pub fn find_judgments_by_name(&self, name: &str) -> Result<Vec<JudgmentId>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT id FROM judgments WHERE name = ?1")?;
        let ids = stmt
            .query_map(params![name], |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(ids.into_iter().map(JudgmentId).collect())
    }

    pub(crate) fn judgment_exists(&self, id: JudgmentId) -> Result<bool> {
        let n: i64 = self
            .conn
            .prepare_cached("SELECT COUNT(*) FROM judgments WHERE id = ?1")?
            .query_row(params![id.0], |r| r.get(0))?;
        Ok(n > 0)
    }

    pub(crate) fn morphism_exists(&self, id: MorphismId) -> Result<bool> {
        let n: i64 = self
            .conn
            .prepare_cached("SELECT COUNT(*) FROM morphisms WHERE id = ?1")?
            .query_row(params![id.0], |r| r.get(0))?;
        Ok(n > 0)
    }

    pub fn morphism_count(&self) -> Result<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM morphisms", [], |r| r.get(0))
    }

    pub(crate) fn count_morphisms_with_status(&self, status: EdgeStatus) -> Result<i64> {
        self.conn
            .prepare_cached("SELECT COUNT(*) FROM morphisms WHERE status = ?1")?
            .query_row(params![status.as_str()], |r| r.get(0))
    }

    pub(crate) fn now_unix() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    /// 人間によるアノテーション。直ちに Accepted として格納する。
    pub fn annotate(
        &self,
        src: JudgmentId,
        dst: JudgmentId,
        kind: MorphismKind,
        rationale: Option<String>,
    ) -> GraphResult<MorphismId> {
        self.insert_morphism(&NewMorphism::manual_accepted(src, dst, kind, rationale).normalized())
    }

    pub fn insert_morphism(&self, new: &NewMorphism) -> GraphResult<MorphismId> {
        let new = new.clone().normalized();
        if new.src == new.dst {
            return Err(ValidationError::SelfLoop.into());
        }
        if !self.judgment_exists(new.src)? {
            return Err(ValidationError::MissingEndpoint {
                which: "src",
                id: new.src,
            }
            .into());
        }
        if !self.judgment_exists(new.dst)? {
            return Err(ValidationError::MissingEndpoint {
                which: "dst",
                id: new.dst,
            }
            .into());
        }

        if let Some(existing) = self.find_morphism_row(new.src, new.dst, new.kind)? {
            return Ok(existing.id);
        }

        if new.status == EdgeStatus::Accepted {
            self.assert_no_exclusive_conflict(new.src, new.dst, new.kind, None)?;
        }

        self.conn
            .prepare_cached(
                "INSERT INTO morphisms
                    (src, dst, kind, origin, status, rationale, proof_term_hash,
                     dependency_signature, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?
            .execute(params![
                new.src.0,
                new.dst.0,
                new.kind.as_str(),
                new.origin.as_str(),
                new.status.as_str(),
                new.rationale,
                new.proof_term_hash,
                new.dependency_signature,
                Self::now_unix(),
            ])?;
        let id = MorphismId(self.conn.last_insert_rowid());
        if new.status == EdgeStatus::Accepted {
            self.suppress_competing_proposals(new.src, new.dst, new.kind)?;
            if new.kind == MorphismKind::Equivalence {
                self.sync_quotient()?;
            }
        }
        Ok(id)
    }

    fn find_morphism_row(
        &self,
        src: JudgmentId,
        dst: JudgmentId,
        kind: MorphismKind,
    ) -> Result<Option<MorphismRecord>> {
        let row = self
            .conn
            .prepare_cached(
                "SELECT id, src, dst, kind, origin, status, rationale,
                        proof_term_hash, dependency_signature, created_at
                 FROM morphisms WHERE src = ?1 AND dst = ?2 AND kind = ?3",
            )?
            .query_row(params![src.0, dst.0, kind.as_str()], Self::morphism_row)
            .optional()?;
        Ok(row)
    }

    pub(crate) fn morphism_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MorphismRecord> {
        Ok(MorphismRecord {
            id: MorphismId(row.get(0)?),
            src: JudgmentId(row.get(1)?),
            dst: JudgmentId(row.get(2)?),
            kind: MorphismKind::from_str(&row.get::<_, String>(3)?)
                .expect("saved kind is always known"),
            origin: EdgeOrigin::from_str(&row.get::<_, String>(4)?)
                .expect("saved origin is always known"),
            status: EdgeStatus::from_str(&row.get::<_, String>(5)?)
                .expect("saved status is always known"),
            rationale: row.get(6)?,
            proof_term_hash: row.get(7)?,
            dependency_signature: row.get(8)?,
            created_at: row.get(9)?,
        })
    }

    fn assert_no_exclusive_conflict(
        &self,
        src: JudgmentId,
        dst: JudgmentId,
        kind: MorphismKind,
        except: Option<MorphismId>,
    ) -> GraphResult<()> {
        let accepted = self.accepted_on_pair(src, dst)?;
        for e in accepted {
            if except.map(|id| id == e.id).unwrap_or(false) {
                continue;
            }
            if e.kind != kind {
                return Err(ValidationError::ExclusiveKindConflict {
                    existing: e.kind,
                    requested: kind,
                    existing_id: e.id,
                }
                .into());
            }
        }
        Ok(())
    }

    /// `(src, dst)` の有向対に対する受理済みの射を探す。同値は無向なので逆方向も見る。
    /// `src`/`dst`/`status` それぞれにインデックスが張られているため（idx_morphisms_status_src /
    /// idx_morphisms_status_dst）、OR で結ばれた2条件のどちらも索引経由で解決できる。
    fn accepted_on_pair(&self, src: JudgmentId, dst: JudgmentId) -> Result<Vec<MorphismRecord>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, src, dst, kind, origin, status, rationale,
                    proof_term_hash, dependency_signature, created_at
             FROM morphisms WHERE status = 'accepted'
               AND ((src = ?1 AND dst = ?2) OR (kind = 'equivalence' AND src = ?2 AND dst = ?1))",
        )?;
        let rows = stmt
            .query_map(params![src.0, dst.0], Self::morphism_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    fn suppress_competing_proposals(
        &self,
        src: JudgmentId,
        dst: JudgmentId,
        kind: MorphismKind,
    ) -> Result<()> {
        if kind == MorphismKind::Equivalence {
            self.conn
                .prepare_cached(
                    "UPDATE morphisms SET status = 'rejected'
                     WHERE status = 'proposed' AND kind != 'equivalence'
                       AND ((src = ?1 AND dst = ?2) OR (src = ?2 AND dst = ?1))",
                )?
                .execute(params![src.0, dst.0])?;
        } else {
            self.conn
                .prepare_cached(
                    "UPDATE morphisms SET status = 'rejected'
                     WHERE status = 'proposed' AND kind != ?3
                       AND src = ?1 AND dst = ?2",
                )?
                .execute(params![src.0, dst.0, kind.as_str()])?;
        }
        Ok(())
    }

    pub fn get_morphism(&self, id: MorphismId) -> GraphResult<MorphismRecord> {
        self.require_morphism(id)
    }

    pub fn list_morphisms(&self) -> Result<Vec<MorphismRecord>> {
        self.list_morphisms_filtered(None, None)
    }

    pub fn list_morphisms_filtered(
        &self,
        kind: Option<MorphismKind>,
        status: Option<EdgeStatus>,
    ) -> Result<Vec<MorphismRecord>> {
        let mut sql = String::from(
            "SELECT id, src, dst, kind, origin, status, rationale,
                    proof_term_hash, dependency_signature, created_at
             FROM morphisms WHERE 1=1",
        );
        if kind.is_some() {
            sql.push_str(" AND kind = ?1");
        }
        if status.is_some() {
            if kind.is_some() {
                sql.push_str(" AND status = ?2");
            } else {
                sql.push_str(" AND status = ?1");
            }
        }
        sql.push_str(" ORDER BY id");

        let mut stmt = self.conn.prepare_cached(&sql)?;
        match (kind, status) {
            (Some(k), Some(s)) => Ok(stmt
                .query_map(params![k.as_str(), s.as_str()], Self::morphism_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?),
            (Some(k), None) => Ok(stmt
                .query_map(params![k.as_str()], Self::morphism_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?),
            (None, Some(s)) => Ok(stmt
                .query_map(params![s.as_str()], Self::morphism_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?),
            (None, None) => Ok(stmt
                .query_map([], Self::morphism_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?),
        }
    }

    pub fn outgoing(
        &self,
        src: JudgmentId,
        kind: Option<MorphismKind>,
        status: Option<EdgeStatus>,
    ) -> Result<Vec<MorphismRecord>> {
        self.incident(src, true, kind, status)
    }

    pub fn incoming(
        &self,
        dst: JudgmentId,
        kind: Option<MorphismKind>,
        status: Option<EdgeStatus>,
    ) -> Result<Vec<MorphismRecord>> {
        self.incident(dst, false, kind, status)
    }

    fn incident(
        &self,
        id: JudgmentId,
        outgoing: bool,
        kind: Option<MorphismKind>,
        status: Option<EdgeStatus>,
    ) -> Result<Vec<MorphismRecord>> {
        let col = if outgoing { "src" } else { "dst" };
        let mut sql = format!(
            "SELECT id, src, dst, kind, origin, status, rationale,
                    proof_term_hash, dependency_signature, created_at
             FROM morphisms WHERE {col} = ?1"
        );
        if kind.is_some() {
            sql.push_str(" AND kind = ?2");
        }
        if status.is_some() {
            let idx = if kind.is_some() { 3 } else { 2 };
            sql.push_str(&format!(" AND status = ?{idx}"));
        }
        sql.push_str(" ORDER BY id");

        let mut stmt = self.conn.prepare_cached(&sql)?;
        match (kind, status) {
            (Some(k), Some(s)) => Ok(stmt
                .query_map(params![id.0, k.as_str(), s.as_str()], Self::morphism_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?),
            (Some(k), None) => Ok(stmt
                .query_map(params![id.0, k.as_str()], Self::morphism_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?),
            (None, Some(s)) => Ok(stmt
                .query_map(params![id.0, s.as_str()], Self::morphism_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?),
            (None, None) => Ok(stmt
                .query_map(params![id.0], Self::morphism_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?),
        }
    }

    pub fn accept_morphism(&self, id: MorphismId) -> GraphResult<MorphismRecord> {
        let rec = self.require_morphism(id)?;
        match rec.status {
            EdgeStatus::Accepted => return Ok(rec),
            EdgeStatus::Rejected => {
                return Err(ValidationError::InvalidStatusTransition {
                    from: EdgeStatus::Rejected,
                    to: EdgeStatus::Accepted,
                }
                .into());
            }
            EdgeStatus::Proposed => {}
        }
        self.assert_no_exclusive_conflict(rec.src, rec.dst, rec.kind, Some(id))?;
        self.conn
            .prepare_cached("UPDATE morphisms SET status = 'accepted' WHERE id = ?1")?
            .execute(params![id.0])?;
        self.suppress_competing_proposals(rec.src, rec.dst, rec.kind)?;
        if rec.kind == MorphismKind::Equivalence {
            self.sync_quotient()?;
        }
        self.require_morphism(id)
    }

    pub fn reject_morphism(&self, id: MorphismId) -> GraphResult<MorphismRecord> {
        let rec = self.require_morphism(id)?;
        if rec.status == EdgeStatus::Rejected {
            return Ok(rec);
        }
        let was_accepted_eq =
            rec.status == EdgeStatus::Accepted && rec.kind == MorphismKind::Equivalence;
        self.conn
            .prepare_cached("UPDATE morphisms SET status = 'rejected' WHERE id = ?1")?
            .execute(params![id.0])?;
        if was_accepted_eq {
            self.rebuild_quotient()?;
        }
        self.require_morphism(id)
    }

    fn require_morphism(&self, id: MorphismId) -> GraphResult<MorphismRecord> {
        self.conn
            .prepare_cached(
                "SELECT id, src, dst, kind, origin, status, rationale,
                        proof_term_hash, dependency_signature, created_at
                 FROM morphisms WHERE id = ?1",
            )?
            .query_row(params![id.0], Self::morphism_row)
            .optional()?
            .ok_or_else(|| ValidationError::NotFound(id).into())
    }

    /// 全判断を毎回 SQLite から読み直して `JudgmentLite` を組み立てる、キャッシュ
    /// なしの版。ヒューリスティック側の「今のDBの真実の状態」を確実に見たい
    /// 呼び出し（テスト等）向けに残してある。`propose_morphisms()` は代わりに
    /// キャッシュを使う `sync_judgment_lites()` を使う。
    pub fn list_judgment_lites(&self) -> Result<Vec<JudgmentLite>> {
        let expr_hashes = self.all_expr_hashes()?;
        self.fetch_judgment_lites_since(&expr_hashes, 0)
    }

    fn all_expr_hashes(&self) -> Result<HashMap<i64, String>> {
        let mut hash_stmt = self
            .conn
            .prepare_cached("SELECT id, canonical_hash FROM expressions")?;
        let map = hash_stmt
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<HashMap<_, _>>>()?;
        Ok(map)
    }

    /// `since_id` より大きい ID を持つ判断だけを読み、`JudgmentLite` を組み立てる。
    fn fetch_judgment_lites_since(
        &self,
        expr_hashes: &HashMap<i64, String>,
        since_id: i64,
    ) -> Result<Vec<JudgmentLite>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT j.id, j.kind, j.name, j.context_json, e.canonical_hash
             FROM judgments j
             JOIN expressions e ON j.statement_expr_id = e.id
             WHERE j.id > ?1
             ORDER BY j.id",
        )?;
        let rows = stmt
            .query_map(params![since_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);

        let mut out = Vec::with_capacity(rows.len());
        for (id, kind, name, ctx_json, stmt_hash) in rows {
            let mut context_hashes = Vec::new();
            if !ctx_json.is_empty() {
                if let Ok(entries) = serde_json::from_str::<Vec<(String, i64)>>(&ctx_json) {
                    for (_, ty_id) in entries {
                        if let Some(h) = expr_hashes.get(&ty_id) {
                            context_hashes.push(h.clone());
                        }
                    }
                }
            }
            out.push(JudgmentLite {
                id: JudgmentId(id),
                kind: JudgmentKind::from_str(&kind).expect("saved kind is always known"),
                name,
                statement_hash: stmt_hash,
                context_hashes,
            });
        }
        Ok(out)
    }

    /// `heuristic_cache` に、前回取り込んだ判断より新しいものだけを読み足す
    /// （フェーズ3.2「インクリメンタル提案」）。判断は挿入後に不変なので、
    /// 一度取り込んだ `JudgmentLite` は破棄・再構築の必要がない。
    fn sync_judgment_lites(&self) -> Result<()> {
        let since_id = self.heuristic_cache.borrow().last_id();
        let expr_hashes = self.all_expr_hashes()?;
        let new_lites = self.fetch_judgment_lites_since(&expr_hashes, since_id)?;
        self.heuristic_cache.borrow_mut().absorb(new_lites);
        Ok(())
    }

    /// ヒューリスティック候補を `proposed` として挿入する。既にある triple は
    /// 再提案しない。判断の読み出しは新規分だけ（`sync_judgment_lites`）だが、
    /// ヒューリスティック自体は毎回「蓄積済みの全判断」に対して実行するため、
    /// 新規判断×既存判断の組み合わせも含め再現率は変わらない——省いているのは
    /// DBへの再アクセスと `JudgmentLite` の再構築であって、比較そのものではない。
    pub fn propose_morphisms(&self) -> GraphResult<Vec<MorphismId>> {
        self.sync_judgment_lites()?;
        let proposals = {
            let cache = self.heuristic_cache.borrow();
            heuristics::propose(cache.judgments())
        };
        // 提案が数千件になりうるので、1件ずつ自動コミットさせず1トランザクションに
        // まとめる（`GraphStore::transaction` のドキュメント参照。実測で
        // 1件あたり約4.9msのコミット待ちが支配的だった）。
        self.transaction(|| {
            let mut ids = Vec::new();
            for p in proposals {
                let new = p.into_new();
                if self.find_morphism_row(new.src, new.dst, new.kind)?.is_some() {
                    continue;
                }
                ids.push(self.insert_morphism(&new)?);
            }
            Ok(ids)
        })
    }

    /// 受理済み同値エッジのうち、永続 Union-Find（`GraphStore::quotient_cache`）に
    /// まだ取り込んでいないものだけを追加で反映してから商グラフを組み立てる。
    ///
    /// フェーズ3アーキテクチャレビュー（問題1・フェーズ3.2「Union-Find永続化」）
    /// が指摘したとおり、判断ごとに別テーブル（旧 `equivalence_classes`）で
    /// クラスIDを管理する設計は`rebuild_quotient()`のたびにテーブル全体を
    /// 作り直す必要があり不利だった。まず `representative_id` 列への移行で
    /// 「テーブル全体の作り直し」を「判断1件あたり1回のUPDATE」に変えた
    /// （O(n log n) → O(n)）。ここではさらに、その O(n) の n を「全判断ノード数」
    /// から「これまでに同値エッジへ一度でも関わった判断ノード数」に絞り込む
    /// ——ここが実際のインクリメンタル化にあたる。受理済み同値射を全件取得する
    /// クエリ自体は毎回発行するが、`kind = 'equivalence'` かつ `status = 'accepted'`
    /// の行に限定されるため（`idx_morphisms_kind_status`）通常は判断ノード全体
    /// よりずっと小さく、かつ Union-Find への取り込みは差分（未取込のIDのみ）
    /// になる。
    fn sync_quotient(&self) -> Result<QuotientGraph> {
        let accepted_equivalences: Vec<(i64, i64, i64)> = {
            let mut stmt = self.conn.prepare_cached(
                "SELECT id, src, dst FROM morphisms WHERE kind = 'equivalence' AND status = 'accepted'",
            )?;
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        let (representative, changed) = {
            let mut cache = self.quotient_cache.borrow_mut();
            let changed = cache.absorb(accepted_equivalences);
            (cache.representative_map(), changed)
        };
        self.materialize_quotient(representative, changed)
    }

    /// `write_representatives` が真のときだけ、渡された代表元写像を
    /// `representative_id` 列へ書き戻す。ベンチマークで判明したとおり
    /// （`quotient_graph/sync_quotient_incremental_no_new_edges` が
    /// `rebuild_quotient_full` とほぼ同じ時間だった）、Union-Find に新規の
    /// 同値射が1件も増えていないのに毎回このUPDATE群を走らせるのは
    /// 「差分更新」の名に反する無駄だった。呼び出し側（`sync_quotient`）が
    /// `QuotientCache::absorb` の戻り値でこれを判定する。
    ///
    /// 一方、`morphisms`（Implication/Specialization/Generalization の
    /// 縮約結果）は同値射が増えていなくても変わりうる（他のエッジ種別が
    /// 新規に受理された場合）ため、こちらは常に再計算する——書き込みだけを
    /// 省略し、読み出しと組み立ては省略しない。
    fn materialize_quotient(
        &self,
        representative: std::collections::BTreeMap<JudgmentId, JudgmentId>,
        write_representatives: bool,
    ) -> Result<QuotientGraph> {
        if write_representatives {
            let tx = self.conn.unchecked_transaction()?;
            {
                let mut stmt = tx
                    .prepare_cached("UPDATE judgments SET representative_id = ?1 WHERE id = ?2")?;
                for (jid, rep) in &representative {
                    let rep_val: Option<i64> = if rep.0 == jid.0 { None } else { Some(rep.0) };
                    stmt.execute(params![rep_val, jid.0])?;
                }
            }
            tx.commit()?;
        }

        let classes = quotient::invert_classes(&representative);
        let accepted = self.list_morphisms_filtered(None, Some(EdgeStatus::Accepted))?;
        let morphisms = quotient::collapse_morphisms(&accepted, &representative);

        Ok(QuotientGraph {
            representative,
            classes,
            morphisms,
        })
    }

    /// 受理済み同値エッジ集合全体から Union-Find を作り直す。`reject_morphism` で
    /// 一度受理した同値エッジを取り消したときのように、Union-Find が原理的に
    /// 「分割」できない変更が起きた場合はこれを使うしかない
    /// （フェーズ3.2「Union-Find永続化」の唯一の弱点: 承認の取り消しだけは
    /// 差分更新できず、キャッシュを空にしてから全受理済み同値射を読み直す）。
    /// 新規の同値射の受理には `sync_quotient()` を使うので、通常経路でこちらが
    /// 呼ばれるのは reject 時だけになる。
    ///
    /// クラスが縮む（reject によりメンバーが1人に戻る等）と、そのメンバーは
    /// 新しい代表元写像には一切登場しなくなる。`sync_quotient`/`materialize_quotient`
    /// は写像に載っていない行を「触らない」設計（未着手の行を毎回書き換えない
    /// ための最適化）なので、そのままでは以前書き込んだ古い `representative_id`
    /// が残ってしまう。そのため full rebuild のときだけ、以前触ったことのある
    /// 行（`representative_id IS NOT NULL`、インデックス済みなので対象は
    /// 「過去に同値エッジへ関わった判断ノード」だけに絞られる）をいったん
    /// NULL に戻してから作り直す。
    pub fn rebuild_quotient(&self) -> Result<QuotientGraph> {
        self.quotient_cache.borrow_mut().reset();
        self.conn
            .prepare_cached(
                "UPDATE judgments SET representative_id = NULL WHERE representative_id IS NOT NULL",
            )?
            .execute([])?;
        self.sync_quotient()
    }

    pub fn quotient_graph(&self) -> Result<QuotientGraph> {
        self.sync_quotient()
    }

    pub fn equivalence_class(&self, id: JudgmentId) -> Result<Vec<JudgmentId>> {
        let q = self.sync_quotient()?;
        Ok(q.class_of(id))
    }

    /// `id` が属する同値類の代表元を返す。受理済み同値エッジが一度もない、または
    /// `id` 自身が代表元の場合は `id` をそのまま返す。値は最後に
    /// `sync_quotient()`/`rebuild_quotient()` が呼ばれた時点のもの
    /// （`insert_morphism`/`accept_morphism`/`reject_morphism` が同値エッジに
    /// 触れるたびに自動で呼ばれるため、通常は常に最新）。
    pub fn representative(&self, id: JudgmentId) -> Result<JudgmentId> {
        let rep: Option<Option<i64>> = self
            .conn
            .prepare_cached("SELECT representative_id FROM judgments WHERE id = ?1")?
            .query_row(params![id.0], |r| r.get(0))
            .optional()?;
        Ok(rep.flatten().map(JudgmentId).unwrap_or(id))
    }

    /// 射に証明項をアタッチし、インターンしたハッシュと依存シグネチャを DB に保存する。
    pub fn attach_proof_term(&self, morphism_id: MorphismId, term: &ProofTerm) -> GraphResult<()> {
        let _rec = self.require_morphism(morphism_id)?;
        let proof_hash = self.intern_proof_term(term)?;

        let analysis = analyze_dependencies(term);
        let dep_sig = analysis.compute_signature();

        self.conn
            .prepare_cached(
                "UPDATE morphisms SET proof_term_hash = ?1, dependency_signature = ?2 WHERE id = ?3",
            )?
            .execute(params![proof_hash, dep_sig, morphism_id.0])?;

        Ok(())
    }

    /// 証明項ハッシュから射を検索する。
    pub fn find_morphisms_by_proof_hash(&self, hash: &str) -> Result<Vec<MorphismRecord>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, src, dst, kind, origin, status, rationale,
                    proof_term_hash, dependency_signature, created_at
             FROM morphisms WHERE proof_term_hash = ?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map(params![hash], Self::morphism_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// 依存シグネチャから射を検索する。
    pub fn find_morphisms_by_dependency_signature(&self, sig: &str) -> Result<Vec<MorphismRecord>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, src, dst, kind, origin, status, rationale,
                    proof_term_hash, dependency_signature, created_at
             FROM morphisms WHERE dependency_signature = ?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map(params![sig], Self::morphism_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

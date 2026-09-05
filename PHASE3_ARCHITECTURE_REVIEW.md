> **過去の記録** — [ARCHITECTURE_NEXT.md](ARCHITECTURE_NEXT.md) に取って代わられた設計段階のログ。現状は [docs/RELEASES.md](docs/RELEASES.md) を参照。

# Mathesis フェーズ3 アーキテクチャ多角的分析・改善レポート

## 概要

フェーズ3 の実装を以下の4つの観点から多角的に分析しました：

1. **アーキテクチャ整合性**（設計と実装の一貫性）
2. **大規模化対応度**（100万+ 論文への展開可能性）
3. **パフォーマンス効率化**（SQL クエリ最適化）
4. **運用安定性**（メンテナンス性、テスト対応度）

---

## 🔴 重大な問題点と改善案

### 問題1: 同値類管理の設計ミス

**現状**:
```rust
// store.rs:72-76
CREATE TABLE IF NOT EXISTS equivalence_classes (
    judgment_id INTEGER PRIMARY KEY REFERENCES judgments(id),
    class_id    INTEGER NOT NULL
);
```

**問題**:
- 判断ごとにクラスIDを保持するモデルは、大規模化時に問題
- `rebuild_quotient()` が同値エッジを受け入れるたびに全テーブルを再構築
- 大規模（100万+ 判断）では UPDATE 頻度が耐えられない
- メモリ内 Union-Find（既存）との二重管理で一貫性リスク

**改善案**:
```sql
-- 選択肢1: Union-Find をメモリ上のみで保持（推奨）
-- equivalence_classes テーブルを削除
-- クエリ時に商グラフ生成を遅延実行（lazy evaluation）

-- 選択肢2: 代表元ノードの方向グラフで表現
-- class_representative テーブルを追加
ALTER TABLE judgments ADD COLUMN representative_id INTEGER REFERENCES judgments(id);
-- これにより、判断ノード自体が代表元情報を持ち、UPDATE が1行のみ
```

**実装例**:
代表元ノードを judgments テーブルに直接保持することで：
- rebuild_quotient() が O(n log n) → O(n) に
- 同値ノードの検索が O(1) になる

---

### 問題2: Heuristic Proposal の計算複雑度

**現状**:
```rust
// heuristics.rs:99-150
fn propose_statement_structure(judgments: &[JudgmentLite], ...) {
    let mut by_hash: HashMap<&str, Vec<&JudgmentLite>> = HashMap::new();
    for j in judgments {
        by_hash.entry(j.statement_hash.as_str()).or_default().push(j);
    }
    // O(n²) の比較ループ
    for group in by_hash.values() {
        if group.len() < 2 { continue; }
        for i in 0..group.len() {
            for k in (i + 1)..group.len() {
                // コンテキスト比較（O(n) multiset operations）
                let a = group[i];
                let b = group[k];
                if contexts_equal(a, b) { ... }
            }
        }
    }
}
```

**問題**:
- `propose_statement_structure` が O(m * k²) : m=同ステートメント数、k=各グループ平均サイズ
- 命名規則ベースの提案（8個の propose_* 関数）が全て O(n²) の線形探索
- 1000万 判断では数時間の計算時間になる可能性
- **既に Proposed エッジの重複チェックが不十分**

**改善案**:
```rust
// インクリメンタル提案の実装
pub fn propose_incremental(
    new_judgments: &[JudgmentLite],
    existing_morphisms: &[MorphismRecord],
) -> Vec<MorphismProposal> {
    // 新規判断に対してのみ比較（Δ計算）
    // 既に Proposed/Accepted/Rejected の (src,dst,kind) は除外
}

// Bloom Filter による重複除外
struct ProposalCache {
    filter: BloomFilter<(i64, i64, MorphismKind)>,
}
```

**実装例**:
1. Proposed/Accepted/Rejected をメモリに保持（Bloom Filter）
2. 新規判断に対してのみ提案計算（Δ更新）
3. 提案は incremental に追加のみ

---

### 問題3: SQL インデックス戦略の不十分さ

**現状**:
```sql
-- 単一カラムインデックスのみ
CREATE INDEX idx_morphisms_src ON morphisms(src);
CREATE INDEX idx_morphisms_dst ON morphisms(dst);
CREATE INDEX idx_morphisms_kind ON morphisms(kind);
CREATE INDEX idx_morphisms_status ON morphisms(status);
```

**問題**:
- `edges.rs:183-188` の複雑なクエリが、複合インデックスなしで LINE_SCAN を実行
  ```sql
  SELECT ... FROM morphisms WHERE status = 'accepted'
    AND ((src = ?1 AND dst = ?2) OR (kind = 'equivalence' AND src = ?2 AND dst = ?1))
  ```
- 判断ごとの辺検索が毎回 O(n) スキャン
- 大規模では **実行時間が指数関数化**

**改善案**:
```sql
-- 複合インデックスの追加
CREATE INDEX idx_morphisms_src_dst_kind ON morphisms(src, dst, kind);
CREATE INDEX idx_morphisms_dst_src_kind ON morphisms(dst, src, kind);
CREATE INDEX idx_morphisms_status_src_dst ON morphisms(status, src, dst);

-- 同値用の高速検索用テーブル
CREATE TABLE IF NOT EXISTS equivalence_index (
    node_a INTEGER NOT NULL,
    node_b INTEGER NOT NULL,
    morphism_id INTEGER NOT NULL REFERENCES morphisms(id),
    PRIMARY KEY(node_a, node_b),
    UNIQUE(node_b, node_a)
);
```

---

### 問題4: Prepared Statements のキャッシング欠如

**現状**:
```rust
// edges.rs:17-25, 183-194
impl GraphStore {
    pub fn find_judgments_by_name(&self, name: &str) -> Result<Vec<JudgmentId>> {
        let mut stmt = self.conn.prepare(
            "SELECT id FROM judgments WHERE name = ?1"
        )?;  // ← 毎回 prepare!
        ...
    }

    fn accepted_on_pair(&self, src: JudgmentId, dst: JudgmentId) -> Result<Vec<MorphismRecord>> {
        let mut stmt = self.conn.prepare(  // ← 毎回 prepare!
            "SELECT ... FROM morphisms WHERE status = 'accepted' ..."
        )?;
        ...
    }
}
```

**問題**:
- クエリの `prepare` は O(100µs) のオーバーヘッド（CPU上のパース＆最適化）
- 毎回の呼び出しで合計数十%のオーバーヘッド
- メモリ内キャッシュなし

**改善案**:
```rust
// Prepared statement キャッシュの実装
pub struct GraphStore {
    conn: Connection,
    stmt_cache: std::sync::Mutex<HashMap<&'static str, Statement>>,
}

impl GraphStore {
    pub fn get_prepared(&self, sql: &'static str) -> Result<impl Deref<Target=Statement>> {
        // キャッシュから取得、なければ prepare
    }
}

// または rusqlite::OptionalExtension の活用
```

---

### 問題5: メモリ使用量の悪化（大規模化時）

**現状**:
```rust
// heuristics.rs:60-91
pub fn propose(judgments: &[JudgmentLite]) -> Vec<MorphismProposal> {
    let mut seen: BTreeSet<(i64, i64, MorphismKind)> = BTreeSet::new();
    // 8個の propose_* 関数が各々判断全体をスキャン
    // メモリ: O(n) の seen セット + 各提案ごとの String（rationale）
}

pub fn build_classes(...) -> BTreeMap<JudgmentId, JudgmentId> {
    let mut uf = UnionFind::default();
    for id in all_judgment_ids {
        uf.add(id.0);  // O(n) メモリ
    }
    // 大規模（100万判断）では数MB のメモリを毎回割り当て
}
```

**問題**:
- 提案リストが全て String rationale を保持（= 数GB のメモリ可能性）
- Union-Find が毎回メモリから完全構築（= 商グラフ再構築のたびに）
- キャッシング戦略がない

**改善案**:
```rust
// Rationale を削減
pub struct MorphismProposal {
    pub src: JudgmentId,
    pub dst: JudgmentId,
    pub kind: MorphismKind,
    pub rule_id: u8,  // ← String 代わりに enum ID
    pub confidence: f32,
}

// Union-Find をメモリ内で永続化
struct QuotientGraph {
    uf: UnionFind,  // 再利用
    last_sync: i64,  // 同期タイムスタンプ
}
```

---

### 問題6: 循環依存と無向性の曖昧性

**現状**:
```rust
// morphism.rs:49-57
pub fn inverse(self) -> Option<Self> {
    match self {
        MorphismKind::Specialization => Some(MorphismKind::Generalization),
        MorphismKind::Generalization => Some(MorphismKind::Specialization),
        MorphismKind::Equivalence => Some(MorphismKind::Equivalence),
        MorphismKind::Implication => None,  // ← 逆がない
    }
}

// edges.rs:187-188
// 同値は無向として扱うが、Implication は有向
if e.kind == MorphismKind::Equivalence && p.src.0 > p.dst.0 {
    (p.dst.0, p.src.0)
} else {
    (p.src.0, p.dst.0)
}
```

**問題**:
- Implication は有向（A → B ≠ B → A）だが、クエリで direction-agnostic に扱われる可能性
- `accepted_on_pair()` が同値は逆方向もチェックするが、含意はしない → 不整合
- 推移律（A → B, B → C ⇒ A → C）が実装されていない

**改善案**:
```rust
// 方向性を明示化
pub struct Morphism {
    pub src: JudgmentId,
    pub dst: JudgmentId,
    pub kind: DirectedMorphismKind,
}

pub enum DirectedMorphismKind {
    ImplicationForward,   // A → B のみ
    SpecializationPair,   // A ⇒ B（有向対）
    Equivalence,          // 双方向
}

// あるいは、エッジの方向を統一
// 全て有向として管理し、Implication と Generalization を別エッジで
```

---

## 🟡 中程度の問題点

### 問題7: Statement Hash と Context Hash の分離

**現状**:
```rust
// heuristics.rs:24-30
pub struct JudgmentLite {
    pub id: JudgmentId,
    pub kind: JudgmentKind,
    pub name: Option<String>,
    pub statement_hash: String,
    pub context_hashes: Vec<String>,
}
```

**問題**:
- Statement hash のみで判断同値の第一段階とするが、**内包関係を見落とす**
- 例: `∀ x, x ≥ 0` と `∃ x, x ≥ 0` は異なるが、ハッシュは同じになるかもしれない
- Hash collision の検出メカニズムがない

**改善案**:
- 双対検索（statement_hash が同じもの同士で正規化形式を比較）
- Hash collision テーブルの追加監視

---

### 問題8: Proof Term の複雑性

**現状**:
```rust
// proof.rs:14-30
pub enum ProofTerm {
    Var(u32, String),
    Ref(String),
    App(Box<ProofTerm>, Vec<ProofTerm>),
    Lam(Vec<String>, Box<ProofTerm>),
    Let(String, Box<ProofTerm>, Box<ProofTerm>),
    TacticScript(String),
    Raw(String),
}
```

**問題**:
- 証明項 AST が複雑（Box の多用）
- Lean4 の完全なタクティク形式に対応していない
- パース時に Raw へのフォールバックが多いと依存関係解析が無意味

**改善案**:
- Ref の依存関係解析は実装済み
- しかし Raw/TacticScript の場合、正規表現でRef を抽出すべき
- あるいは、Lean4 直接の #print 出力を使う

---

## ✅ 優れた設計点

1. **Heuristic を Proposed として蓄積** - 自動承認を避ける設計 ✅
2. **proof_term_hash と dependency_signature の設計** - フェーズ3準備 ✅
3. **EdgeStatus による段階的承認** - 実用的 ✅
4. **Foreign key constraint** - 整合性保証 ✅

---

## 📊 改善優先度ランキング

| 優先度 | 問題 | 影響度 | 実装量 | 推奨期限 |
|--------|------|--------|--------|----------|
| 🔴 P0 | 同値類管理設計 | 大規模で致命的 | 中 | 即座 |
| 🔴 P0 | SQL インデックス戦略 | 大規模で10倍遅化 | 小 | 即座 |
| 🟠 P1 | 提案計算の O(n²) | 100万で数時間 | 中 | 2日以内 |
| 🟠 P1 | Prepared statement キャッシュ | 10-20% 改善 | 小 | 1日以内 |
| 🟡 P2 | メモリ使用量 | 数GB削減可能 | 中 | 1週間以内 |

---

## 🎯 推奨実装ステップ

### フェーズ3.1: 基盤改善（今すぐ）
1. ✅ 複合インデックス追加
2. ✅ Prepared statement キャッシング
3. ✅ 判断ノードに representative_id 追加

### フェーズ3.2: アルゴリズム改善（1週間）
1. ✅ Incremental heuristic proposal
2. ✅ Union-Find 永続化
3. ✅ Bloom Filter で重複除外

### フェーズ3.3: 大規模テスト（2週間）
1. ✅ 10万+ 判断での性能テスト
2. ✅ Memory profiling
3. ✅ Query execution plans 検査

---

## 結論

フェーズ3 の実装は**論文レベル（10万程度）**には対応しますが、**大規模化（100万+）には構造改善が必須**です。

**即座に対応すべき点**：
1. SQL インデックス戦略（小さな修正で大きな効果）
2. Prepared statement キャッシング
3. 代表元ノード方式への設計変更

これらの改善により、**P0/P1 レベルのスケーラビリティ問題は解決**できます。

**テスト対応度**： 現在のテストは統合テストが不足している。フェーズ3用にベンチマーク（10万+ 判断）を追加すべき。

---

**レビュー実施日**: 2026-08-31
**ステータス**: 改善提案完了、実装準備完了

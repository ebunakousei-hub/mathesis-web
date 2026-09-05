//! 層3の証明項（Proof Term）と依存関係の静的解析。
//!
//! 定理や補題の射（Morphism）に対して証明の実体を保持し、証明項 AST を走査して
//! 参照されている他の判断（公理・定理・定義）の依存関係集合から
//! `dependency_signature` を決定論的に計算する。

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// 証明項（Proof Term）の AST 構造体。
///
/// Lean4 の `#print` 出力やタクティクススクリプト、λ項を構造化して保持する。
/// 部分的にパースできない場合でも `TacticScript` または `Raw` へフォールバックする。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProofTerm {
    /// 局所束縛変数 (id, hint)
    Var(u32, String),
    /// 外部の定理・定義・公理・補題名への参照（依存解析の対象）
    Ref(String),
    /// 関数適用 (fn, [args])
    App(Box<ProofTerm>, Vec<ProofTerm>),
    /// λ抽象 ([param_names], body)
    Lam(Vec<String>, Box<ProofTerm>),
    /// 局所定義 (var_name, val, body)
    Let(String, Box<ProofTerm>, Box<ProofTerm>),
    /// タクティクススクリプト (`by induction ...; simp`)
    TacticScript(String),
    /// 未パースの生テキスト
    Raw(String),
}

impl ProofTerm {
    /// 証明項の構造を正規化文字列にし、決定論的な 64bit FNV-1a ハッシュ (hex) を計算する。
    /// 同一の証明構造（サブツリー含む）は同じハッシュを持つため、Blob ストレージ上で
    /// ノードの重複排除（DAG 圧縮）が可能になる。
    pub fn canonical_hash_hex(&self) -> String {
        let mut hasher = Fnv1a64::new();
        self.hash_content(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    fn hash_content(&self, h: &mut Fnv1a64) {
        match self {
            ProofTerm::Var(id, name) => {
                h.write_u8(1);
                h.write_u32(*id);
                h.write_str(name);
            }
            ProofTerm::Ref(name) => {
                h.write_u8(2);
                h.write_str(name);
            }
            ProofTerm::App(func, args) => {
                h.write_u8(3);
                func.hash_content(h);
                h.write_usize(args.len());
                for a in args {
                    a.hash_content(h);
                }
            }
            ProofTerm::Lam(params, body) => {
                h.write_u8(4);
                h.write_usize(params.len());
                for p in params {
                    h.write_str(p);
                }
                body.hash_content(h);
            }
            ProofTerm::Let(name, val, body) => {
                h.write_u8(5);
                h.write_str(name);
                val.hash_content(h);
                body.hash_content(h);
            }
            ProofTerm::TacticScript(script) => {
                h.write_u8(6);
                h.write_str(script.trim());
            }
            ProofTerm::Raw(text) => {
                h.write_u8(7);
                h.write_str(text.trim());
            }
        }
    }
}

/// 静的解析器が抽出した依存関係レポート。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyAnalysis {
    /// 必須依存関係（Must-have dependencies: メイン証明パスで参照されている定理・公理・定義名）
    pub must_have_refs: BTreeSet<String>,
    /// 代替・分岐依存関係（Alternative branches で参照されている名前）
    pub alternative_refs: BTreeSet<String>,
}

impl DependencyAnalysis {
    /// 必須依存集合のソート順ハッシュから `dependency_signature` を生成する。
    pub fn compute_signature(&self) -> String {
        let mut hasher = Fnv1a64::new();
        hasher.write_str("dep_sig_v1:");
        for r in &self.must_have_refs {
            hasher.write_str(r);
            hasher.write_u8(0xFF);
        }
        format!("{:016x}", hasher.finish())
    }
}

/// 証明項 AST を静的解析し、参照されている他判断の名前を収集する。
pub fn analyze_dependencies(term: &ProofTerm) -> DependencyAnalysis {
    let mut analysis = DependencyAnalysis::default();
    collect_refs(term, &mut analysis.must_have_refs);
    analysis
}

fn collect_refs(term: &ProofTerm, refs: &mut BTreeSet<String>) {
    match term {
        ProofTerm::Var(_, _) => {}
        ProofTerm::Ref(name) => {
            refs.insert(name.clone());
        }
        ProofTerm::App(func, args) => {
            collect_refs(func, refs);
            for a in args {
                collect_refs(a, refs);
            }
        }
        ProofTerm::Lam(_, body) => {
            collect_refs(body, refs);
        }
        ProofTerm::Let(_, val, body) => {
            collect_refs(val, refs);
            collect_refs(body, refs);
        }
        ProofTerm::TacticScript(script) | ProofTerm::Raw(script) => {
            // タクティクススクリプトや生テキスト内の `simp [foo, bar]` や `exact baz` をスキャン
            extract_tactic_refs(script, refs);
        }
    }
}

/// タクティクススクリプトテキストから参照識別子を抽出するヒューリスティック。
fn extract_tactic_refs(script: &str, refs: &mut BTreeSet<String>) {
    // 予約語や一般的なキーワードは除外
    const KEYWORDS: &[&str] = &[
        "by", "exact", "apply", "intro", "intros", "rw", "rewrite", "simp",
        "unfold", "rfl", "induction", "cases", "with", "have", "show", "from",
        "trivial", "positivity", "linarith", "ring", "omega", "ext", "constructor",
        "left", "right", "obtain", "simpa", "assumption", "match", "zero", "succ",
        "inl", "inr", "True", "False", "Prop", "Type", "Set", "Finset", "List",
    ];

    let clean = script.replace(&['[', ']', '(', ')', '{', '}', ',', ':', ';', '⟨', '⟩', '•', '←'][..], " ");
    for token in clean.split_whitespace() {
        let t = token.trim_start_matches("ih").trim_start_matches('h');
        let target = if t.is_empty() { token } else { t };

        if target.len() > 1
            && target.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
            && !KEYWORDS.contains(&target)
            && !target.chars().all(|c| c.is_numeric())
        {
            refs.insert(target.to_string());
        }
    }
}

// ---------------------------------------------------------------------------
// 内部用 FNV-1a 64bit ハッシャー
// ---------------------------------------------------------------------------

struct Fnv1a64 {
    state: u64,
}

impl Fnv1a64 {
    fn new() -> Self {
        Fnv1a64 { state: 0xcbf29ce484222325 }
    }

    fn write_u8(&mut self, byte: u8) {
        self.state ^= u64::from(byte);
        self.state = self.state.wrapping_mul(0x100000001b3);
    }

    fn write_u32(&mut self, val: u32) {
        for b in val.to_le_bytes() {
            self.write_u8(b);
        }
    }

    fn write_usize(&mut self, val: usize) {
        for b in (val as u64).to_le_bytes() {
            self.write_u8(b);
        }
    }

    fn write_str(&mut self, s: &str) {
        for b in s.as_bytes() {
            self.write_u8(*b);
        }
    }

    fn finish(&self) -> u64 {
        self.state
    }
}

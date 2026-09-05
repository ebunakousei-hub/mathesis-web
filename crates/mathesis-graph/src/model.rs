//! 層2: 判断・命題層（Judgment/Statement Layer）のデータモデル。
//!
//! 定理・定義・補題・公理・予想を単一の型 `Judgment` で表す。これらを区別するのは
//! `kind` フィールドのみであり、本質的にはどのノードも判断 `Γ ⊢ P` の形をしている。
//! 前提条件（コンテキスト Γ）はノードの属性としてではなく `Hypothesis` の列という
//! 明示的な構造として保持するため、「ペアノ公理下での加法の結合律」と
//! 「群論下での結合律」はコンテキストが異なる別ノードとして区別される。

use crate::paper::PaperId;
use mathesis_ast::Expr;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ExprId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct JudgmentId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JudgmentKind {
    Theorem,
    Lemma,
    Definition,
    Axiom,
    Conjecture,
    Example,
    Instance,
    /// 診断⑥への対応（`mathesis-fulltext`のLaTeX定理環境抽出）で追加。
    /// Leanの構文キーワードは{theorem,lemma,definition,axiom,example,
    /// instance}の閉じた集合だが、arXivの数学論文が`\newtheorem`で
    /// 宣言する種別はこれよりずっと多様——中でもCorollary・Proposition・
    /// Claim・Remarkは実データで頻出する（`mathesis-fulltext::theorem::
    /// FALLBACK_THEOREM_ENVS`/`NON_PROVABLE_KINDS`参照）にもかかわらず
    /// 対応する種別が無かった。近い既存の種別（例: CorollaryをLemmaに）へ
    /// 押し込めるのは黙って情報を失うので、素直に4種を追加する。
    Corollary,
    Proposition,
    Claim,
    Remark,
}

impl JudgmentKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            JudgmentKind::Theorem => "theorem",
            JudgmentKind::Lemma => "lemma",
            JudgmentKind::Definition => "definition",
            JudgmentKind::Axiom => "axiom",
            JudgmentKind::Conjecture => "conjecture",
            JudgmentKind::Example => "example",
            JudgmentKind::Instance => "instance",
            JudgmentKind::Corollary => "corollary",
            JudgmentKind::Proposition => "proposition",
            JudgmentKind::Claim => "claim",
            JudgmentKind::Remark => "remark",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "theorem" => JudgmentKind::Theorem,
            "lemma" => JudgmentKind::Lemma,
            "definition" => JudgmentKind::Definition,
            "axiom" => JudgmentKind::Axiom,
            "conjecture" => JudgmentKind::Conjecture,
            "example" => JudgmentKind::Example,
            "instance" => JudgmentKind::Instance,
            "corollary" => JudgmentKind::Corollary,
            "proposition" => JudgmentKind::Proposition,
            "claim" => JudgmentKind::Claim,
            "remark" => JudgmentKind::Remark,
            _ => return None,
        })
    }
}

/// コンテキスト Γ の一項目（変数名 : 型）。型は層1の式ノードを指す。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hypothesis {
    pub name: String,
    pub ty: ExprId,
}

/// パース器がソース全体をどこまで構造化できたか。フォールバックが生じても
/// `raw_text` は必ず保持されるため、後段の処理は失敗を無視して安全に進める。
///
/// `Informal`は診断⑥への対応で追加した第4の値。Full/Partial/Failedは
/// いずれも「形式言語（Lean）を構造化しようとした結果」の軸だが、
/// LaTeXの定理文はそもそも構造化された形式表現ではなく自然文であり、
/// 「パースに失敗した」わけではない——`statement`は`mathesis_ast::Expr::
/// Unparsed`（層1が元々持っていた「これ以上構造化できない残余テキスト」
/// のフォールバック）に包んで保持する。Failedへ倒すと「パースを試みて
/// 失敗した」という誤った含意になるため、独立した値にした。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParseStatus {
    Full,
    Partial,
    Failed,
    Informal,
}

impl ParseStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ParseStatus::Full => "full",
            ParseStatus::Partial => "partial",
            ParseStatus::Failed => "failed",
            ParseStatus::Informal => "informal",
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "full" => ParseStatus::Full,
            "partial" => ParseStatus::Partial,
            "informal" => ParseStatus::Informal,
            _ => ParseStatus::Failed,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SourceRef {
    pub file: String,
    pub line: u32,
}

/// これから保存する判断ノード（ID 未採番）
#[derive(Debug, Clone)]
pub struct NewJudgment {
    pub kind: JudgmentKind,
    pub name: Option<String>,
    pub context: Vec<Hypothesis>,
    pub statement: ExprId,
    /// 定義の右辺など、層1で構造化していない残余（Phase 3 の証明項埋め込みまでは
    /// 生テキストのまま保持する）
    pub definition_body_raw: Option<String>,
    pub source: SourceRef,
    pub raw_text: String,
    pub parse_status: ParseStatus,
    /// Phase 9: この判断の由来となった論文（arXiv等）。`source`（ファイル名・
    /// 行番号というファイルシステム上の由来）とは別軸の、概念タクソノミー側
    /// （`mathesis-taxonomy`/`mathesis-ingest`）への書誌的な由来。任意項目——
    /// 手書きのfixtureや論文と紐付かないLeanファイルではNoneのままでよい。
    pub source_paper: Option<PaperId>,
}

/// 保存済みの判断ノード（ID 採番済み、Γ の各型は展開して式に戻したもの）
#[derive(Debug, Clone)]
pub struct JudgmentRecord {
    pub id: JudgmentId,
    pub kind: JudgmentKind,
    pub name: Option<String>,
    pub context: Vec<(String, Expr)>,
    pub statement: Expr,
    pub statement_hash: String,
    pub definition_body_raw: Option<String>,
    pub source_file: String,
    pub source_line: u32,
    pub raw_text: String,
    pub parse_status: ParseStatus,
    pub source_paper: Option<PaperId>,
}

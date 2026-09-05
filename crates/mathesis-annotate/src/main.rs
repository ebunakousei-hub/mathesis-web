//! mathesis-annotate — 層3のエッジを人間が対話的にアノテーションするCLIツール。
//!
//! アーキテクチャ設計の警告に従い、フェーズ2では自動検出を本採用せず、
//! 人間の数学者によるアノテーションを最優先とする。このツールは Wikipedia/nLab
//! ライクなシンプルなインターフェースで射の承認・棄却・追加を行う。
//!
//! ## コマンド一覧
//!
//! ```text
//! # ヒューリスティック提案の確認と承認
//! mathesis-annotate <db.sqlite> proposals [--kind implication|specialization|generalization|equivalence]
//! mathesis-annotate <db.sqlite> accept <morphism-id>
//! mathesis-annotate <db.sqlite> reject <morphism-id>
//!
//! # 人間による直接アノテーション
//! mathesis-annotate <db.sqlite> annotate <src-name> <dst-name> <kind> [--rationale <text>]
//!
//! # 判断ノード一覧
//! mathesis-annotate <db.sqlite> list [--kind theorem|lemma|definition|axiom|...]
//! mathesis-annotate <db.sqlite> show <judgment-name>
//!
//! # 同値クラス・商グラフ
//! mathesis-annotate <db.sqlite> classes
//! mathesis-annotate <db.sqlite> morphisms [--kind <kind>] [--status proposed|accepted|rejected]
//!
//! # 整合性検証
//! mathesis-annotate <db.sqlite> validate
//!
//! # ヒューリスティック再実行（新規 Proposed のみ追加、既存は再提案しない）
//! mathesis-annotate <db.sqlite> propose
//! ```

use anyhow::{anyhow, bail, Result};
use mathesis_graph::{
    EdgeStatus, GraphStore, JudgmentId, MorphismId, MorphismKind, ProofTerm,
};
use std::path::PathBuf;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 3 {
        print_usage();
        std::process::exit(1);
    }

    let db_path = PathBuf::from(&args[1]);
    if !db_path.exists() {
        bail!("DB ファイルが見つかりません: {}", db_path.display());
    }

    let store = GraphStore::open(&db_path)
        .map_err(|e| anyhow!("DB を開けませんでした: {}", e))?;

    let sub = args[2].as_str();
    let rest = &args[3..];

    match sub {
        "proposals" | "list-proposals" => cmd_proposals(&store, rest)?,
        "accept" => cmd_accept(&store, rest)?,
        "reject" => cmd_reject(&store, rest)?,
        "annotate" => cmd_annotate(&store, rest)?,
        "list" => cmd_list(&store, rest)?,
        "show" => cmd_show(&store, rest)?,
        "classes" => cmd_classes(&store)?,
        "morphisms" => cmd_morphisms(&store, rest)?,
        "validate" => cmd_validate(&store)?,
        "propose" => cmd_propose(&store)?,
        "attach-proof" => cmd_attach_proof(&store, rest)?,
        "show-proof" => cmd_show_proof(&store, rest)?,
        "help" | "--help" | "-h" => {
            print_usage();
        }
        _ => {
            eprintln!("不明なサブコマンド: {}", sub);
            print_usage();
            std::process::exit(1);
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// サブコマンド実装
// ---------------------------------------------------------------------------

/// ヒューリスティック提案の一覧表示。
fn cmd_proposals(store: &GraphStore, args: &[String]) -> Result<()> {
    let kind_filter = parse_flag_kind(args, "--kind")?;
    let status_filter = Some(EdgeStatus::Proposed);

    let proposals = store
        .list_morphisms_filtered(kind_filter, status_filter)
        .map_err(|e| anyhow!("{}", e))?;

    if proposals.is_empty() {
        println!("提案されたエッジはありません。`propose` で再実行できます。");
        return Ok(());
    }

    println!(
        "{:>6}  {:>8}  {:>8}  {:^14}  {:^10}  rationale",
        "id", "src", "dst", "kind", "origin"
    );
    println!("{}", "-".repeat(80));

    for m in &proposals {
        let src_name = store
            .get_judgment(m.src)
            .ok()
            .and_then(|j| j.name)
            .unwrap_or_else(|| format!("#{}", m.src.0));
        let dst_name = store
            .get_judgment(m.dst)
            .ok()
            .and_then(|j| j.name)
            .unwrap_or_else(|| format!("#{}", m.dst.0));
        println!(
            "{:>6}  {:>8}  {:>8}  {:^14}  {:^10}  {}",
            m.id.0,
            src_name,
            dst_name,
            m.kind.as_str(),
            m.origin.as_str(),
            m.rationale.as_deref().unwrap_or("-")
        );
    }
    println!("\n合計: {} 件の提案", proposals.len());
    Ok(())
}

/// 提案されたエッジを承認する。
fn cmd_accept(store: &GraphStore, args: &[String]) -> Result<()> {
    let id = parse_morphism_id(args)?;
    let rec = store
        .accept_morphism(id)
        .map_err(|e| anyhow!("承認失敗: {}", e))?;
    println!(
        "✓ エッジ #{} を承認しました ({} → {}, {})",
        rec.id.0, rec.src.0, rec.dst.0, rec.kind.as_str()
    );
    if rec.kind == MorphismKind::Equivalence {
        let class = store
            .equivalence_class(rec.src)
            .map_err(|e| anyhow!("{}", e))?;
        println!("  同値クラスのサイズ: {} 件", class.len());
    }
    Ok(())
}

/// 提案されたエッジを棄却する。
fn cmd_reject(store: &GraphStore, args: &[String]) -> Result<()> {
    let id = parse_morphism_id(args)?;
    let rec = store
        .reject_morphism(id)
        .map_err(|e| anyhow!("棄却失敗: {}", e))?;
    println!(
        "✗ エッジ #{} を棄却しました ({} → {}, {})",
        rec.id.0, rec.src.0, rec.dst.0, rec.kind.as_str()
    );
    Ok(())
}

/// 人間が直接エッジをアノテーションする。
/// 使い方: annotate <src-name> <dst-name> <kind> [--rationale <text>]
fn cmd_annotate(store: &GraphStore, args: &[String]) -> Result<()> {
    if args.len() < 3 {
        bail!("使い方: annotate <src-name> <dst-name> <kind> [--rationale <text>]");
    }
    let src_name = &args[0];
    let dst_name = &args[1];
    let kind = parse_kind(&args[2])?;
    let rationale = parse_flag_string(args, "--rationale");

    let src_ids = store
        .find_judgments_by_name(src_name)
        .map_err(|e| anyhow!("{}", e))?;
    if src_ids.is_empty() {
        bail!("判断ノードが見つかりません: '{}'", src_name);
    }
    let dst_ids = store
        .find_judgments_by_name(dst_name)
        .map_err(|e| anyhow!("{}", e))?;
    if dst_ids.is_empty() {
        bail!("判断ノードが見つかりません: '{}'", dst_name);
    }

    // 複数マッチがある場合は最初のものを使い、警告を出す
    if src_ids.len() > 1 {
        eprintln!(
            "警告: '{}' に {} 件の判断が見つかりました。最初の #{} を使います。",
            src_name,
            src_ids.len(),
            src_ids[0].0
        );
    }
    if dst_ids.len() > 1 {
        eprintln!(
            "警告: '{}' に {} 件の判断が見つかりました。最初の #{} を使います。",
            dst_name,
            dst_ids.len(),
            dst_ids[0].0
        );
    }

    let id = store
        .annotate(src_ids[0], dst_ids[0], kind, rationale)
        .map_err(|e| anyhow!("アノテーション失敗: {}", e))?;

    println!(
        "✓ エッジ #{} を作成しました: '{}' ({}) → '{}' ({}), kind={}",
        id.0, src_name, src_ids[0].0, dst_name, dst_ids[0].0, kind.as_str()
    );
    Ok(())
}

/// 判断ノード一覧表示。
fn cmd_list(store: &GraphStore, args: &[String]) -> Result<()> {
    let kind_filter = parse_flag_string(args, "--kind");

    let judgments = store.list_judgments().map_err(|e| anyhow!("{}", e))?;
    let filtered: Vec<_> = judgments
        .iter()
        .filter(|j| {
            if let Some(ref k) = kind_filter {
                j.kind.as_str() == k.as_str()
            } else {
                true
            }
        })
        .collect();

    println!(
        "{:>6}  {:^12}  {:^30}  source",
        "id", "kind", "name"
    );
    println!("{}", "-".repeat(70));
    for j in &filtered {
        println!(
            "{:>6}  {:^12}  {:^30}  {}:{}",
            j.id.0,
            j.kind.as_str(),
            j.name.as_deref().unwrap_or("(無名)"),
            j.source_file,
            j.source_line
        );
    }
    println!("\n合計: {} 件", filtered.len());
    Ok(())
}

/// 判断ノード詳細表示（名前で検索）。
fn cmd_show(store: &GraphStore, args: &[String]) -> Result<()> {
    if args.is_empty() {
        bail!("使い方: show <judgment-name>");
    }
    let name = &args[0];
    let ids = store
        .find_judgments_by_name(name)
        .map_err(|e| anyhow!("{}", e))?;
    if ids.is_empty() {
        bail!("判断ノードが見つかりません: '{}'", name);
    }

    for id in ids {
        let j = store.get_judgment(id).map_err(|e| anyhow!("{}", e))?;
        println!("=== #{}: {} ===", j.id.0, j.name.as_deref().unwrap_or("(無名)"));
        println!("  kind      : {}", j.kind.as_str());
        println!("  statement : {}", j.statement);
        println!("  hash      : {}", j.statement_hash);
        println!(
            "  context   : {}",
            if j.context.is_empty() {
                "∅".to_string()
            } else {
                j.context
                    .iter()
                    .map(|(n, t)| format!("{} : {}", n, t))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        );
        println!("  source    : {}:{}", j.source_file, j.source_line);
        println!("  parse     : {}", j.parse_status.as_str());
        if let Some(ref body) = j.definition_body_raw {
            println!("  body      : {}", body);
        }

        // 射を表示
        let out = store
            .outgoing(id, None, Some(EdgeStatus::Accepted))
            .map_err(|e| anyhow!("{}", e))?;
        let inc = store
            .incoming(id, None, Some(EdgeStatus::Accepted))
            .map_err(|e| anyhow!("{}", e))?;

        if !out.is_empty() {
            println!("  --- 出ていく射 (accepted) ---");
            for m in &out {
                let dst_name = store
                    .get_judgment(m.dst)
                    .ok()
                    .and_then(|j| j.name)
                    .unwrap_or_else(|| format!("#{}", m.dst.0));
                println!("    #{} --[{}]--> {}", m.id.0, m.kind.as_str(), dst_name);
            }
        }
        if !inc.is_empty() {
            println!("  --- 入ってくる射 (accepted) ---");
            for m in &inc {
                let src_name = store
                    .get_judgment(m.src)
                    .ok()
                    .and_then(|j| j.name)
                    .unwrap_or_else(|| format!("#{}", m.src.0));
                println!("    {} --[{}]--> #{}", src_name, m.kind.as_str(), m.id.0);
            }
        }
        println!();
    }
    Ok(())
}

/// 同値クラス一覧。
fn cmd_classes(store: &GraphStore) -> Result<()> {
    let q = store.quotient_graph().map_err(|e| anyhow!("{}", e))?;

    let mut class_list: Vec<(&JudgmentId, &Vec<JudgmentId>)> = q.classes.iter().collect();
    class_list.sort_by_key(|(rep, _)| rep.0);

    let singleton_count = class_list.iter().filter(|(_, m)| m.len() == 1).count();
    let equiv_count = class_list.iter().filter(|(_, m)| m.len() > 1).count();

    println!("同値クラス数: {} (うち同値 {} クラス, 単独 {} クラス)", class_list.len(), equiv_count, singleton_count);

    for (rep, members) in &class_list {
        if members.len() == 1 {
            continue; // 単独ノードは省略
        }
        let names: Vec<String> = members
            .iter()
            .map(|id| {
                store
                    .get_judgment(*id)
                    .ok()
                    .and_then(|j| j.name)
                    .unwrap_or_else(|| format!("#{}", id.0))
            })
            .collect();
        println!("  [代表 #{}] {{ {} }}", rep.0, names.join(", "));
    }

    println!("\n商グラフの射数: {}", q.morphisms.len());
    Ok(())
}

/// 射一覧表示。
fn cmd_morphisms(store: &GraphStore, args: &[String]) -> Result<()> {
    let kind_filter = parse_flag_kind(args, "--kind")?;
    let status_filter = parse_flag_status(args, "--status")?;

    let morphisms = store
        .list_morphisms_filtered(kind_filter, status_filter)
        .map_err(|e| anyhow!("{}", e))?;

    if morphisms.is_empty() {
        println!("該当する射はありません。");
        return Ok(());
    }

    println!(
        "{:>6}  {:>14}  {:>14}  {:^14}  {:^8}  {:^10}",
        "id", "src", "dst", "kind", "status", "origin"
    );
    println!("{}", "-".repeat(80));

    for m in &morphisms {
        let src_name = store
            .get_judgment(m.src)
            .ok()
            .and_then(|j| j.name)
            .unwrap_or_else(|| format!("#{}", m.src.0));
        let dst_name = store
            .get_judgment(m.dst)
            .ok()
            .and_then(|j| j.name)
            .unwrap_or_else(|| format!("#{}", m.dst.0));
        println!(
            "{:>6}  {:>14}  {:>14}  {:^14}  {:^8}  {:^10}",
            m.id.0,
            src_name,
            dst_name,
            m.kind.as_str(),
            m.status.as_str(),
            m.origin.as_str()
        );
    }
    println!("\n合計: {} 件", morphisms.len());
    Ok(())
}

/// ストレージの整合性検証。
fn cmd_validate(store: &GraphStore) -> Result<()> {
    let report = store.validate().map_err(|e| anyhow!("{}", e))?;
    println!("{}", report.summary());
    if !report.is_valid() {
        for issue in &report.broken_references {
            println!("  ✗ {}", issue);
        }
        std::process::exit(1);
    }
    Ok(())
}

/// ヒューリスティック再実行（新規 Proposed のみ追加）。
fn cmd_propose(store: &GraphStore) -> Result<()> {
    let ids = store
        .propose_morphisms()
        .map_err(|e| anyhow!("{}", e))?;
    println!(
        "✓ {} 件の新規提案を追加しました（status=proposed）",
        ids.len()
    );
    if !ids.is_empty() {
        println!("  `proposals` コマンドで確認し、`accept` または `reject` してください。");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 引数パースユーティリティ
// ---------------------------------------------------------------------------

/// 射に証明項を手動でアタッチする。
/// 使い方: attach-proof <morphism-id> <proof-script-text>
fn cmd_attach_proof(store: &GraphStore, args: &[String]) -> Result<()> {
    if args.len() < 2 {
        bail!("使い方: attach-proof <morphism-id> <proof-script-text>");
    }
    let id = parse_morphism_id(args)?;
    let script = args[1..].join(" ");

    let term = ProofTerm::TacticScript(script);
    store
        .attach_proof_term(id, &term)
        .map_err(|e| anyhow!("証明項アタッチ失敗: {}", e))?;

    let rec = store.get_morphism(id).map_err(|e| anyhow!("{}", e))?;
    println!(
        "✓ 射 #{} ({} → {}) に証明項をアタッチしました",
        id.0, rec.src.0, rec.dst.0
    );
    println!("  proof_term_hash      : {}", rec.proof_term_hash.as_deref().unwrap_or("-"));
    println!("  dependency_signature : {}", rec.dependency_signature.as_deref().unwrap_or("-"));
    Ok(())
}

/// 射の証明項と依存解析結果を表示する。
fn cmd_show_proof(store: &GraphStore, args: &[String]) -> Result<()> {
    let id = parse_morphism_id(args)?;
    let rec = store.get_morphism(id).map_err(|e| anyhow!("{}", e))?;

    println!("=== 射 #{}: {} → {} ({}) ===", rec.id.0, rec.src.0, rec.dst.0, rec.kind.as_str());
    println!("  status              : {}", rec.status.as_str());
    println!("  proof_term_hash     : {}", rec.proof_term_hash.as_deref().unwrap_or("(未アタッチ)"));
    println!("  dependency_signature: {}", rec.dependency_signature.as_deref().unwrap_or("(未アタッチ)"));

    if let Some(ref hash) = rec.proof_term_hash {
        if let Ok(term) = store.get_proof_term(hash) {
            println!("  proof_term AST      : {:?}", term);
            let analysis = mathesis_graph::analyze_dependencies(&term);
            println!("  must-have deps      : {:?}", analysis.must_have_refs);
        }
    }
    Ok(())
}

fn parse_morphism_id(args: &[String]) -> Result<MorphismId> {
    let s = args
        .first()
        .ok_or_else(|| anyhow!("morphism-id が必要です"))?;
    let n: i64 = s
        .parse()
        .map_err(|_| anyhow!("morphism-id は整数でなければなりません: {}", s))?;
    Ok(MorphismId(n))
}

fn parse_kind(s: &str) -> Result<MorphismKind> {
    MorphismKind::from_str(s).ok_or_else(|| {
        anyhow!(
            "不明なエッジ種別: {}  (implication|specialization|generalization|equivalence)",
            s
        )
    })
}

fn parse_flag_kind(args: &[String], flag: &str) -> Result<Option<MorphismKind>> {
    if let Some(v) = parse_flag_string(args, flag) {
        Ok(Some(parse_kind(&v)?))
    } else {
        Ok(None)
    }
}

fn parse_flag_status(args: &[String], flag: &str) -> Result<Option<EdgeStatus>> {
    if let Some(v) = parse_flag_string(args, flag) {
        let s = EdgeStatus::from_str(&v)
            .ok_or_else(|| anyhow!("不明なステータス: {} (proposed|accepted|rejected)", v))?;
        Ok(Some(s))
    } else {
        Ok(None)
    }
}

fn parse_flag_string(args: &[String], flag: &str) -> Option<String> {
    for i in 0..args.len() {
        if args[i] == flag {
            return args.get(i + 1).cloned();
        }
    }
    None
}

fn print_usage() {
    eprintln!(
        r#"mathesis-annotate — 数学知識グラフ アノテーションCLI

使い方: mathesis-annotate <db.sqlite> <subcommand> [options]

サブコマンド:
  proposals [--kind <kind>]                  提案されたエッジ一覧
  accept <id>                                エッジを承認
  reject <id>                                エッジを棄却
  annotate <src> <dst> <kind> [--rationale]  直接アノテーション
  list [--kind <kind>]                       判断ノード一覧
  show <name>                                判断ノード詳細
  classes                                    同値クラス一覧
  morphisms [--kind <k>] [--status <s>]     射一覧
  validate                                   整合性検証
  propose                                    ヒューリスティック再実行
  attach-proof <id> <script>                 証明項を射にアタッチ
  show-proof <id>                            射の証明項と依存構造を表示

<kind>: implication | specialization | generalization | equivalence
<status>: proposed | accepted | rejected
"#
    );
}

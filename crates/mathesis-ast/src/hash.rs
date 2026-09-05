//! 正規化ハッシュ（Canonical Hash）。
//!
//! 束縛変数をド・ブラウン индекス（最も内側の束縛子からの距離）に変換してから
//! 構造をシリアライズし、FNV-1a でハッシュする。これにより `∀ x, x^2 ≥ 0` と
//! `∀ y, y^2 ≥ 0` のような α同値な式は必ず同じハッシュ値を持つ一方、自由変数
//! （未束縛の識別子）はそのまま名前で区別されるため `x^2` と `y^2`（束縛子なし）
//! は異なるハッシュになる。

use crate::ast::{BinderKind, Expr};

/// FNV-1a 64bit。外部依存を避けるための最小実装（決定的・プラットフォーム非依存）。
fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;
    let mut hash = OFFSET_BASIS;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

impl Expr {
    /// α同値な式が一致する 64bit の正規化ハッシュを計算する。
    pub fn canonical_hash(&self) -> u64 {
        let mut env: Vec<u32> = Vec::new();
        let mut out = String::new();
        write_canonical(self, &mut env, &mut out);
        fnv1a64(out.as_bytes())
    }

    /// 表示・保存用に 16 桁 16 進文字列で返す。
    pub fn canonical_hash_hex(&self) -> String {
        format!("{:016x}", self.canonical_hash())
    }
}

fn write_canonical(e: &Expr, env: &mut Vec<u32>, out: &mut String) {
    match e {
        Expr::Var(id, _name) => {
            match env.iter().rev().position(|v| *v == id.0) {
                Some(de_bruijn_index) => {
                    out.push('#');
                    out.push_str(&de_bruijn_index.to_string());
                }
                // 本来スコープ解決済みなら起こらないが、壊れた入力に対する安全策
                None => {
                    out.push_str("!FREE!");
                }
            }
        }
        Expr::Const(name) => {
            out.push('C');
            out.push_str(&name.len().to_string());
            out.push(':');
            out.push_str(name);
        }
        Expr::Lit(s) => {
            out.push('L');
            out.push_str(&s.len().to_string());
            out.push(':');
            out.push_str(s);
        }
        Expr::App(func, args) => {
            out.push('(');
            write_canonical(func, env, out);
            for a in args {
                out.push(' ');
                write_canonical(a, env, out);
            }
            out.push(')');
        }
        Expr::Bind(kind, bindings, body) => {
            out.push(match kind {
                BinderKind::Pi => 'P',
                BinderKind::Lambda => 'L',
                BinderKind::Exists => 'E',
            });
            out.push('[');
            let mut pushed = 0usize;
            for b in bindings {
                match &b.ty {
                    Some(ty) => write_canonical(ty, env, out),
                    None => out.push('_'),
                }
                out.push(';');
                // この束縛子以降の型・本体から参照できるよう、書き終えてから push する
                // （依存束縛 (x : A) (y : B x) の B が x を参照できるようにするため）
                env.push(b.var.0);
                pushed += 1;
            }
            out.push_str("].");
            write_canonical(body, env, out);
            for _ in 0..pushed {
                env.pop();
            }
        }
        Expr::Unparsed(t) => {
            out.push('U');
            out.push_str(&t.len().to_string());
            out.push(':');
            out.push_str(t);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ast::{Binding, Expr};
    use crate::parser::parse_expr;

    fn h(src: &str) -> u64 {
        parse_expr(src).unwrap().expr.canonical_hash()
    }

    #[test]
    fn alpha_equivalent_binders_hash_equal() {
        assert_eq!(h("∀ x, x^2 ≥ 0"), h("∀ y, y^2 ≥ 0"));
    }

    #[test]
    fn different_bodies_hash_differ() {
        assert_ne!(h("∀ x, x^2 ≥ 0"), h("∀ x, x^3 ≥ 0"));
    }

    #[test]
    fn free_variables_are_not_alpha_equivalent() {
        // 束縛子がない自由変数 x, y は別の識別子として区別される
        assert_ne!(h("x^2"), h("y^2"));
    }

    #[test]
    fn shadowing_does_not_break_canonicalization() {
        assert_eq!(h("∀ x, ∀ x, x = x"), h("∀ a, ∀ b, b = b"));
    }

    #[test]
    fn manual_binding_smoke() {
        // Binding/BinderKind を直接組み立てても canonical_hash が壊れないことの確認
        use crate::ast::BinderKind;
        let e1 = Expr::Bind(
            BinderKind::Pi,
            vec![Binding {
                var: crate::ast::VarId(1),
                hint: "x".into(),
                ty: None,
            }],
            Box::new(Expr::Var(crate::ast::VarId(1), "x".into())),
        );
        let e2 = Expr::Bind(
            BinderKind::Pi,
            vec![Binding {
                var: crate::ast::VarId(99),
                hint: "z".into(),
                ty: None,
            }],
            Box::new(Expr::Var(crate::ast::VarId(99), "z".into())),
        );
        assert_eq!(e1.canonical_hash(), e2.canonical_hash());
    }

    #[test]
    fn different_binder_kinds_never_collide() {
        // フェーズ3アーキテクチャレビュー問題7への回帰テスト:
        // 「∀ x, x≥0 と ∃ x, x≥0 のハッシュが同じになるかもしれない」という懸念に対し、
        // canonical_hash は束縛子の種類（Pi/Lambda/Exists）を正規化文字列の先頭タグ
        // （write_canonical の 'P'/'L'/'E'）に含めるため、本文が同一でも束縛子が違えば
        // 必ず異なるハッシュになることを確認する。
        assert_ne!(h("∀ x, x ≥ 0"), h("∃ x, x ≥ 0"));
        assert_ne!(h("∀ x, x ≥ 0"), h("λ x, x ≥ 0"));
        assert_ne!(h("∃ x, x ≥ 0"), h("λ x, x ≥ 0"));
    }
}

export function escapeHtml(s: string): string {
  const div = document.createElement("div");
  div.textContent = s;
  return div.innerHTML;
}

/**
 * エクスポートJSONの`generatedAtUnix`（UNIX秒）を、閲覧者のロケール・
 * タイムゾーンで読める日付へ変換する。外部レビュー（2026-09-05）で
 * 「データがいつ生成されたか画面に見えない」と指摘されたための追加——
 * Rust側は新規の日時クレートを増やさず秒数だけを返し、人間向けの表示
 * 形式への変換はここに集約する。
 */
export function formatGeneratedAt(unixSeconds: number): string {
  if (!Number.isFinite(unixSeconds) || unixSeconds <= 0) return "?";
  return new Date(unixSeconds * 1000).toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

/**
 * Leanのインポータが完全に構文解析できなかった字句は、命題本文の中に
 * `«Symbol(">")»` という内部表現のまま残っている（実データ1,431件のうち
 * 105件）。これは「`>` という記号」を指す表記そのものなので、画面には
 * 中身の記号だけを出す——読む側にとって `«Symbol(">")»` という文字列は
 * 一切の情報を持たず、命題を読み下す邪魔にしかならない。
 *
 * 解析が不完全であること自体を隠すわけではない: 同じ画面に出ている
 * `partial` バッジがそれを示していて、この関数はその内訳の表記を
 * 人間が読める形に直すだけ。
 */
export function unwrapLeanSymbols(statement: string): string {
  return statement.replace(/«Symbol\("([^"]*)"\)»/g, "$1");
}

/**
 * Phase 1証拠層サイドカー（`judgments.provenance.json`等）の読み込みで
 * 問題が起きたときの報告口。外部レビュー（2026-09-05）指摘への対応:
 * 「サイドカーが無ければ黙って今までどおり表示する」という互換動作は
 * 正しいが、それを**壊れたリリースの見落とし**にしてはいけない。
 *
 * 開発時（`npm run dev`、Viteの`import.meta.env.DEV`）は画面上にも
 * 警告を出す——本番ビルドでは`console.error`だけに留め、閲覧者の画面は
 * 今までどおり静かに動く（レガシー互換モード）。この非対称は意図的:
 * 壊れたリリースを検出すべきなのは開発・デプロイ確認の場であって、
 * 一般の閲覧者に警告バナーを見せることではない。
 */
let provenanceWarningBanner: HTMLElement | null = null;
export function reportProvenanceIssue(message: string): void {
  console.error(`[provenance] ${message}`);
  if (!import.meta.env.DEV) return;
  if (!provenanceWarningBanner) {
    provenanceWarningBanner = document.createElement("div");
    provenanceWarningBanner.className = "provenance-dev-warning";
    provenanceWarningBanner.setAttribute("role", "alert");
    document.body.appendChild(provenanceWarningBanner);
  }
  const line = document.createElement("div");
  line.textContent = `⚠ provenance: ${message}`;
  provenanceWarningBanner.appendChild(line);
}

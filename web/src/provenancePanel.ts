/**
 * assertion単位のprovenance詳細パネル（外部レビュー2026-09-05、提案4）。
 *
 * これまでは「Provenance: assertion #N (release TAG)」という1行の
 * ツールチップ文字列だけだった——研究者が実際に検証に使うには、それが
 * 何のEvidenceで、どのSourceRecordから来て、既定のトラバースに含まれる
 * 主張なのかまで見えて初めて意味を持つ。この増分ではネイティブの
 * `<dialog>`（新規のモーダル基盤を作らない、この既存コードベースの
 * 「native `title`属性でツールチップを済ませる」のと同じ最小主義）で
 * `assertions.json`（`mathesis-provenance reconcile`が書き出す、id引き
 * 辞書）を1回だけ取得してその場に表示する。
 */
import type { AssertionDetail } from "./types";
import { escapeHtml, reportProvenanceIssue } from "./util";

let assertionsPromise: Promise<Record<string, AssertionDetail>> | null = null;

function loadAssertions(): Promise<Record<string, AssertionDetail>> {
  if (!assertionsPromise) {
    assertionsPromise = fetch(`${import.meta.env.BASE_URL}assertions.json`)
      .then((resp) => {
        if (!resp.ok) {
          if (resp.status !== 404) reportProvenanceIssue(`assertions.json returned HTTP ${resp.status}`);
          return {};
        }
        return resp.json() as Promise<Record<string, AssertionDetail>>;
      })
      .catch((err) => {
        reportProvenanceIssue(`assertions.json failed to load: ${err}`);
        return {};
      });
  }
  return assertionsPromise;
}

let dialogEl: HTMLDialogElement | null = null;

function ensureDialog(): HTMLDialogElement {
  if (!dialogEl) {
    dialogEl = document.createElement("dialog");
    dialogEl.className = "provenance-dialog";
    document.body.appendChild(dialogEl);
    dialogEl.addEventListener("click", (ev) => {
      // 背景クリックで閉じる（native dialogの流儀）。
      if (ev.target === dialogEl) dialogEl?.close();
    });
  }
  return dialogEl;
}

/**
 * P6.1（`docs/LEAN_DEPENDENCY_POLICY.md`）: `origin`("type"|"body"|"both")の
 * 短い説明——「対象宣言の型・値(証明項)のどちらに、この依存の参照が
 * 実際に現れたか」。
 */
const DEPENDENCY_ORIGIN_LABEL: Record<string, string> = {
  type: "in the declaration's type",
  body: "in the declaration's value (proof term)",
  both: "in both the declaration's type and value",
};

function renderEvidence(e: AssertionDetail["evidence"][number]): string {
  const metric = e.metricName !== null && e.metricValue !== null ? `${escapeHtml(e.metricName)} = ${e.metricValue.toFixed(4)}` : null;
  const isFormalExport = e.evidenceKind === "formal_export";
  const originLabel = e.dependencyOrigin ? DEPENDENCY_ORIGIN_LABEL[e.dependencyOrigin] ?? e.dependencyOrigin : null;
  return `
    <li class="prov-evidence-item">
      <div class="prov-evidence-kind">${escapeHtml(e.evidenceKind)}${e.extractorOrModel ? ` · ${escapeHtml(e.extractorOrModel)}` : ""}</div>
      ${e.locator ? `<div class="prov-evidence-locator">"${escapeHtml(e.locator)}"</div>` : `<div class="prov-evidence-locator prov-muted">(no source span retained)</div>`}
      ${metric ? `<div class="prov-evidence-metric">${metric}</div>` : ""}
      <div class="prov-evidence-source">source: ${escapeHtml(e.sourceProvider)}:${escapeHtml(e.sourceProviderId)}</div>
      ${
        isFormalExport
          ? `<div class="prov-evidence-formal">
              <div class="prov-muted">Checker-derived dependency${originLabel ? ` — found ${escapeHtml(originLabel)}` : ""}.</div>
              ${e.formalRevision ? `<div class="prov-muted">${escapeHtml(e.formalRevision)}</div>` : ""}
              <div class="prov-muted">This means the elaborated declaration's type-checked term contains this constant — not that it is a minimal mathematical dependency (a proof may cite more than it strictly needs).</div>
            </div>`
          : ""
      }
    </li>`;
}

function renderReview(r: AssertionDetail["reviewDecisions"][number]): string {
  return `
    <li class="prov-review-item">
      <div>${escapeHtml(r.decision)}${r.reviewerId ? ` by ${escapeHtml(r.reviewerId)}` : " (reviewer unknown)"}</div>
      ${r.rationale ? `<div class="prov-muted">${escapeHtml(r.rationale)}</div>` : ""}
    </li>`;
}

/**
 * P3, Increment 1（`docs/P3_STATUS.md`）: カタログにラベルがあれば
 * 「表示名 (タグ付き参照)」、無ければタグ付き参照だけを見せる——無い
 * ラベルを捏造しない。
 */
function renderRef(ref: string, label: string | null): string {
  return label ? `${escapeHtml(label)} <code>${escapeHtml(ref)}</code>` : `<code>${escapeHtml(ref)}</code>`;
}

/**
 * P5, Item 1（`docs/P5_PLAN.md`）: `traversalPolicy`の4値それぞれに、
 * 短い理由書きを添える——`eligibleForDefaultTraversal`という真偽値だけでは
 * 「却下されたのか、まだ根拠が弱いだけなのか、形式的な文脈限定なのか」が
 * 伝わらない。
 */
const TRAVERSAL_POLICY_LABEL: Record<AssertionDetail["traversalPolicy"], [string, string]> = {
  default_traversal: ["default-traversal eligible", "Shown by default in the lineage view."],
  visible_only: ["visible, opt-in only", "Not shown by default — visible only when the lineage view's \"trusted only\" filter is off."],
  formal_only: ["formal contexts only", "Reviewed, but only eligible for traversal in a formally-scoped context, not the default view."],
  excluded: ["excluded", "Rejected — never shown, even with the \"trusted only\" filter off."],
};

function renderDetail(d: AssertionDetail): string {
  const [policyBadge, policyHint] = TRAVERSAL_POLICY_LABEL[d.traversalPolicy] ?? TRAVERSAL_POLICY_LABEL.visible_only;
  return `
    <form method="dialog" class="prov-dialog-form">
      <div class="prov-dialog-header">
        <span class="prov-dialog-title">Assertion #${d.id}</span>
        <button type="submit" class="prov-dialog-close" aria-label="Close">×</button>
      </div>
      <div class="prov-dialog-body">
        <div class="prov-row"><span class="prov-label">relation</span>
          ${renderRef(d.subjectRef, d.subjectLabel)} —<b>${escapeHtml(d.predicate)}</b>→ ${renderRef(d.objectRef, d.objectLabel)}
        </div>
        <div class="prov-row"><span class="prov-label">epistemic state</span>
          <span class="prov-badge">${escapeHtml(d.epistemicState)}</span>
          <span class="prov-badge ${d.eligibleForDefaultTraversal ? "prov-badge-trusted" : "prov-badge-untrusted"}" title="${escapeHtml(policyHint)}">${escapeHtml(policyBadge)}</span>
        </div>
        ${d.score !== null ? `<div class="prov-row"><span class="prov-label">score</span>${d.score.toFixed(4)}</div>` : ""}
        <div class="prov-row"><span class="prov-label">release</span>${escapeHtml(d.releaseTag)}</div>
        <div class="prov-section-title">Evidence (${d.evidence.length})</div>
        <ul class="prov-evidence-list">${d.evidence.map(renderEvidence).join("")}</ul>
        ${
          d.reviewDecisions.length > 0
            ? `<div class="prov-section-title">Review decisions (${d.reviewDecisions.length})</div><ul class="prov-review-list">${d.reviewDecisions.map(renderReview).join("")}</ul>`
            : ""
        }
        <p class="prov-caveat">Traceable to a source ≠ independently reviewed ≠ formally verified ≠ mathematically true. This panel shows only where the claim above came from and how confident the extractor was — not whether it is correct.</p>
      </div>
    </form>`;
}

/** クリックハンドラから直接呼べる、assertion詳細パネルの表示口。 */
export async function showAssertionDetail(assertionId: number): Promise<void> {
  const dialog = ensureDialog();
  dialog.innerHTML = `<p class="prov-loading">loading…</p>`;
  dialog.showModal();
  const all = await loadAssertions();
  const detail = all[String(assertionId)];
  dialog.innerHTML = detail
    ? renderDetail(detail)
    : `<form method="dialog" class="prov-dialog-form"><p>Assertion #${assertionId} not found in assertions.json.</p><button type="submit">Close</button></form>`;
}

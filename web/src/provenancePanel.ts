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

function renderEvidence(e: AssertionDetail["evidence"][number]): string {
  const metric = e.metricName !== null && e.metricValue !== null ? `${escapeHtml(e.metricName)} = ${e.metricValue.toFixed(4)}` : null;
  return `
    <li class="prov-evidence-item">
      <div class="prov-evidence-kind">${escapeHtml(e.evidenceKind)}${e.extractorOrModel ? ` · ${escapeHtml(e.extractorOrModel)}` : ""}</div>
      ${e.locator ? `<div class="prov-evidence-locator">"${escapeHtml(e.locator)}"</div>` : `<div class="prov-evidence-locator prov-muted">(no source span retained)</div>`}
      ${metric ? `<div class="prov-evidence-metric">${metric}</div>` : ""}
      <div class="prov-evidence-source">source: ${escapeHtml(e.sourceProvider)}:${escapeHtml(e.sourceProviderId)}</div>
    </li>`;
}

function renderReview(r: AssertionDetail["reviewDecisions"][number]): string {
  return `
    <li class="prov-review-item">
      <div>${escapeHtml(r.decision)}${r.reviewerId ? ` by ${escapeHtml(r.reviewerId)}` : " (reviewer unknown)"}</div>
      ${r.rationale ? `<div class="prov-muted">${escapeHtml(r.rationale)}</div>` : ""}
    </li>`;
}

function renderDetail(d: AssertionDetail): string {
  return `
    <form method="dialog" class="prov-dialog-form">
      <div class="prov-dialog-header">
        <span class="prov-dialog-title">Assertion #${d.id}</span>
        <button type="submit" class="prov-dialog-close" aria-label="Close">×</button>
      </div>
      <div class="prov-dialog-body">
        <div class="prov-row"><span class="prov-label">relation</span>
          <code>${escapeHtml(d.subjectRef)}</code> —<b>${escapeHtml(d.predicate)}</b>→ <code>${escapeHtml(d.objectRef)}</code>
        </div>
        <div class="prov-row"><span class="prov-label">epistemic state</span>
          <span class="prov-badge">${escapeHtml(d.epistemicState)}</span>
          ${d.eligibleForDefaultTraversal ? `<span class="prov-badge prov-badge-trusted">default-traversal eligible</span>` : `<span class="prov-badge prov-badge-untrusted">opt-in only</span>`}
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

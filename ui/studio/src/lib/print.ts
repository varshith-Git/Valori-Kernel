/**
 * Shared print-to-popup utility.
 * Previously duplicated between CertifyTab (printCertificate) and
 * AskTab (printReceipt).
 */

/**
 * Opens a new browser window, writes raw HTML into it, and calls window.print().
 * The caller is responsible for building the HTML string; this function only
 * owns the popup lifecycle.
 */
export function printHtml(html: string, title = "Valori"): void {
  const w = window.open("", "_blank", "width=860,height=900");
  if (!w) {
    alert("Allow popups to print.");
    return;
  }
  w.document.write(`<!DOCTYPE html><html lang="en"><head>
<meta charset="UTF-8"/>
<title>${title}</title>
</head><body>${html}</body></html>`);
  w.document.close();
  w.focus();
  setTimeout(() => w.print(), 400);
}

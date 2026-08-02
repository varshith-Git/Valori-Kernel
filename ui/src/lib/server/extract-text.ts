export async function extractText(file: File): Promise<string> {
  const buf = Buffer.from(await file.arrayBuffer());
  const name = file.name.toLowerCase();

  if (name.endsWith(".txt") || name.endsWith(".md") || file.type === "text/plain") {
    return buf.toString("utf-8");
  }

  if (name.endsWith(".pdf") || file.type === "application/pdf") {
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    const pdfParse = require("pdf-parse/lib/pdf-parse.js");

    function renderPageWithLayout(pageData: {
      getTextContent: (opts: unknown) => Promise<{
        items: { str: string; transform: number[]; width: number }[];
      }>;
    }) {
      return pageData.getTextContent({ normalizeWhitespace: false, disableCombineTextItems: true })
        .then((textContent: { items: { str: string; transform: number[]; width: number }[] }) => {
          if (!textContent.items.length) return "";

          const avgCharWidth = textContent.items.reduce((s, it) => {
            const len = it.str.replace(/\s/g, "").length;
            return len > 0 ? s + it.width / len : s;
          }, 0) / (textContent.items.filter((it) => it.str.replace(/\s/g, "").length > 0).length || 1);
          const spaceWidth = Math.max(avgCharWidth * 0.5, 3);

          const rows = new Map<number, { x: number; w: number; str: string }[]>();
          for (const item of textContent.items) {
            const y = Math.round(item.transform[5]);
            if (!rows.has(y)) rows.set(y, []);
            rows.get(y)!.push({ x: item.transform[4], w: item.width, str: item.str });
          }

          const sortedYs = [...rows.keys()].sort((a, b) => b - a);

          return sortedYs.map((y) => {
            const items = rows.get(y)!.sort((a, b) => a.x - b.x);
            let line = "";
            let cursor = items[0].x;
            for (const item of items) {
              const gap = item.x - cursor;
              if (line.length > 0 && gap > spaceWidth) {
                const spaces = Math.min(Math.round(gap / spaceWidth), 8);
                line += " ".repeat(spaces);
              }
              line += item.str;
              cursor = item.x + item.w;
            }
            return line.trimEnd();
          }).join("\n");
        });
    }

    try {
      // pdf-parse defaults to a bundled pdf.js from 2017 (v1.10.100); v2.0.550
      // is also bundled and handles a wider range of real-world PDFs.
      const data = await pdfParse(buf, { pagerender: renderPageWithLayout, version: "v2.0.550" });
      return data.text as string;
    } catch (e) {
      console.error("PDF extraction failed:", e);
      throw new Error(
        "Could not read this PDF — it may be corrupted, password-protected, or a scanned image with no embedded text. " +
        "Try re-exporting it, or upload a .txt/.md file instead."
      );
    }
  }

  if (
    name.endsWith(".docx") ||
    file.type === "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
  ) {
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    const mammoth = require("mammoth");
    const result = await mammoth.extractRawText({ buffer: buf });
    return result.value as string;
  }

  return buf.toString("utf-8");
}

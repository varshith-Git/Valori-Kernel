import { NextRequest, NextResponse } from "next/server";
import { fetchWithTimeout } from "@/lib/server/http";
import { resolveProjectNodeUrl, ProjectNotFoundError, ProjectNotReadyError } from "@/lib/server/project";
import { extractText } from "@/lib/server/extract-text";

const JSON_HEADERS = { "Content-Type": "application/json" };

// If the text has no markdown headers, detect section titles by pattern and
// add "## " prefixes so the Rust TreeIndex parser can find them.
// Mirrors the header patterns in the JS ingest chunker (chunkTextTree).
function toMarkdown(text: string): string {
  const lines = text.replace(/\r\n/g, "\n").split("\n");

  // Already has markdown headers → use as-is
  if (lines.some((l) => /^#{1,4}\s/.test(l))) return text;

  const NUMBERED = /^(\d+(\.\d+)*)[.)]\s+[A-Z][^\n]{2,80}$/;
  const TITLE_CASE = /^[A-Z][A-Za-z0-9 ,:\-–/]{4,70}[^.!?,]$/;
  const ALL_CAPS = /^[A-Z][A-Z0-9 :]{4,60}$/;

  return lines
    .map((line, i) => {
      const s = line.trim();
      if (!s) return line;
      // Skip lines inside code blocks
      if (s.startsWith("```")) return line;

      if (NUMBERED.test(s) || ALL_CAPS.test(s)) {
        return `## ${s}`;
      }
      if (TITLE_CASE.test(s) && s.length < 80) {
        // Only treat as header if followed by non-empty content
        const next = lines[i + 1]?.trim();
        if (next && next.length > 10 && !TITLE_CASE.test(next)) {
          return `## ${s}`;
        }
      }
      return line;
    })
    .join("\n");
}

export async function POST(req: NextRequest, { params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  let nodeUrl: string;
  try {
    nodeUrl = await resolveProjectNodeUrl(id);
  } catch (e) {
    if (e instanceof ProjectNotFoundError) return NextResponse.json({ error: "not found" }, { status: 404 });
    if (e instanceof ProjectNotReadyError) return NextResponse.json({ error: "project not active yet" }, { status: 409 });
    return NextResponse.json({ error: "node unreachable" }, { status: 503 });
  }

  try {
    let text: string;
    let doc_name: string;

    const contentType = req.headers.get("content-type") ?? "";

    if (contentType.includes("multipart/form-data")) {
      // File upload path — extract text server-side (PDF, DOCX, TXT, MD)
      const form = await req.formData();
      const file = form.get("file") as File | null;
      if (!file) return NextResponse.json({ error: "No file provided" }, { status: 400 });
      text = await extractText(file);
      doc_name = (form.get("doc_name") as string) || file.name;
    } else {
      // JSON path — text already extracted client-side
      const body = (await req.json()) as { text: string; doc_name?: string };
      text = body.text;
      doc_name = body.doc_name ?? "document";
    }

    if (!text?.trim()) {
      return NextResponse.json({ error: "No text extracted from file" }, { status: 400 });
    }

    const markdown = toMarkdown(text);

    const res = await fetchWithTimeout(`${nodeUrl}/v1/tree/build`, {
      method: "POST",
      headers: JSON_HEADERS,
      body: JSON.stringify({ text: markdown, doc_name }),
    });
    const data = await res.json().catch(() => ({ error: "invalid response" }));
    return NextResponse.json(data, { status: res.status });
  } catch (e) {
    return NextResponse.json({ error: e instanceof Error ? e.message : "build failed" }, { status: 503 });
  }
}

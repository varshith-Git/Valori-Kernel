"use client";

import { useState, useCallback, useEffect, useRef } from "react";
import { useTheme } from "@/lib/theme";

// -- Types ---------------------------------------------------------------------

export interface CodePanelResult {
  id: number;
  score: number;
}

interface Props {
  isOpen: boolean;
  onClose: () => void;
  queryVector: number[];
  queryText?: string;     // original plain-text query (when text-embed mode was used)
  k: number;
  collection: string;     // full namespace e.g. "sigmoid:hr"
  result: CodePanelResult;
  embedProvider?: string; // "ollama" | "openai" | "cohere" | "custom"
  embedModel?: string;
  embedEndpoint?: string;
}

type Lang = "python" | "typescript" | "curl";

// -- Cosine helper -------------------------------------------------------------
// Valori returns L2² distance (f32). For unit-normalised vectors:
//   cosine = 1 - L2²/2   (ranges 0 to 4 for unit vectors)
function cosineFromScore(score: number) {
  return Math.max(0, Math.min(100, (1 - score / 2) * 100));
}

// -- Vector formatting ---------------------------------------------------------
function fmtVecShort(v: number[], lang: "python" | "typescript" | "json"): string {
  const open  = lang === "python" ? "[" : "[";
  const close = lang === "python" ? "]" : "]";
  if (v.length <= 8) {
    return `${open}${v.map((n) => +n.toFixed(6)).join(", ")}${close}`;
  }
  const head = v.slice(0, 4).map((n) => +n.toFixed(6)).join(", ");
  const tail = v.slice(-2).map((n) => +n.toFixed(6)).join(", ");
  return `${open}${head}, ..., ${tail}${close}  # ${v.length} values`;
}

function fullVecJSON(v: number[]): string {
  return "[" + v.map((n) => +n.toFixed(8)).join(", ") + "]";
}

// -- Embed code snippets -------------------------------------------------------

function embedSnippetPython(provider = "ollama", model = "nomic-embed-text", endpoint?: string): string {
  switch (provider) {
    case "openai":
      return `import openai

client = openai.OpenAI()  # uses OPENAI_API_KEY env var
embedding = client.embeddings.create(
    model="${model || "text-embedding-3-small"}",
    input="your query text here",
)
query = embedding.data[0].embedding
`;
    case "cohere":
      return `import cohere

co = cohere.Client()  # uses COHERE_API_KEY env var
resp = co.embed(
    texts=["your query text here"],
    model="${model || "embed-english-v3.0"}",
    input_type="search_query",
)
query = resp.embeddings[0]
`;
    default: { // ollama or custom
      const base = endpoint?.replace(/\/api\/embed(?:dings)?$/, "").replace(/\/$/, "") || "http://localhost:11434";
      return `import requests

resp = requests.post("${base}/api/embed", json={
    "model": "${model || "nomic-embed-text"}",
    "input": "your query text here",
})
query = resp.json()["embeddings"][0]
`;
    }
  }
}

function embedSnippetTS(provider = "ollama", model = "nomic-embed-text", endpoint?: string): string {
  switch (provider) {
    case "openai":
      return `import OpenAI from "openai";

const ai = new OpenAI();  // uses OPENAI_API_KEY env var
const emb = await ai.embeddings.create({
  model: "${model || "text-embedding-3-small"}",
  input: "your query text here",
});
const query = emb.data[0].embedding;
`;
    case "cohere":
      return `import { CohereClient } from "cohere-ai";

const co = new CohereClient();  // uses COHERE_API_KEY env var
const emb = await co.embed({
  texts: ["your query text here"],
  model: "${model || "embed-english-v3.0"}",
  inputType: "search_query",
});
const query = (emb.embeddings as number[][])[0];
`;
    default: {
      const base = endpoint?.replace(/\/api\/embed(?:dings)?$/, "").replace(/\/$/, "") || "http://localhost:11434";
      return `const embRes = await fetch("${base}/api/embed", {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ model: "${model || "nomic-embed-text"}", input: "your query text here" }),
});
const { embeddings } = await embRes.json();
const query: number[] = embeddings[0];
`;
    }
  }
}

// -- Code generators -----------------------------------------------------------

function genPython(
  vector: number[], k: number, collection: string,
  result: CodePanelResult, provider?: string, model?: string, endpoint?: string,
  queryText?: string,
): string {
  const cosine = cosineFromScore(result.score);
  const vec = fmtVecShort(vector, "python");
  const embedCode = embedSnippetPython(provider, model, endpoint);
  const queryLine = queryText
    ? embedCode.replace(`"your query text here"`, `"${queryText.replace(/"/g, '\\"')}"`)
    : embedCode;

  return `from valoricore.remote import SyncRemoteClient

# -- Embed your query (${provider ?? "ollama"} · ${model ?? "nomic-embed-text"} · ${vector.length} dims) -----------
${queryLine}
# -- Connect to Valori ---------------------------------------------
client = SyncRemoteClient("http://localhost:3000")

# -- Pre-computed vector (already embedded above) ------------------
# query = ${vec}

# -- Search --------------------------------------------------------
results = client.search(query, k=${k}, collection="${collection}")

# -- This search returned record #${result.id} ---------------------
#   L2² score  = ${result.score.toExponential(4)}
#   cosine sim ≈ ${cosine.toFixed(1)}%  (for unit-normalised vectors)
for r in results:
    print("  #{} score={:.6f}".format(r["id"], r["score"]))
`;
}

function genTypeScript(
  vector: number[], k: number, collection: string,
  result: CodePanelResult, provider?: string, model?: string, endpoint?: string,
  queryText?: string,
): string {
  const cosine = cosineFromScore(result.score);
  const vec = fmtVecShort(vector, "typescript");
  const rawEmbed = embedSnippetTS(provider, model, endpoint);
  const embedCode = queryText
    ? rawEmbed.replace(`"your query text here"`, `"${queryText.replace(/"/g, '\\"')}"`)
    : rawEmbed;

  return `// -- Embed your query (${provider ?? "ollama"} · ${model ?? "nomic-embed-text"} · ${vector.length} dims) -----
${embedCode}
// -- Pre-computed vector (already embedded above) -----------------
// const query: number[] = ${vec};

// -- Search --------------------------------------------------------
const res = await fetch("http://localhost:3000/search", {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ query, k: ${k}, collection: "${collection}" }),
});

const { results } = await res.json() as {
  results: { id: number; score: number }[];
};

// -- This search returned record #${result.id} ---------------------
//   L2² score  = ${result.score.toExponential(4)}
//   cosine sim ≈ ${cosine.toFixed(1)}%  (for unit-normalised vectors)
for (const r of results) {
  console.log(\`  #\${r.id} score=\${r.score.toFixed(6)}\`);
}
`;
}

function genCurl(vector: number[], k: number, collection: string, result: CodePanelResult): string {
  const cosine = cosineFromScore(result.score);
  const shortVec = vector.slice(0, 4).map((n) => +n.toFixed(6)).join(", ");

  return `# Embed your query first with the same model, then:

curl -X POST http://localhost:3000/search \\
  -H "Content-Type: application/json" \\
  -d '{
    "query": [${shortVec}, ...],
    "k": ${k},
    "collection": "${collection}"
  }'

# This search returned record #${result.id}
#   L2² score  = ${result.score.toExponential(4)}
#   cosine sim ≈ ${cosine.toFixed(1)}%

# Full vector (${vector.length} dims) — paste into "query" above:
# ${fullVecJSON(vector).slice(0, 120)}...
`;
}

// -- Syntax highlighter --------------------------------------------------------

const PY_KW = new Set([
  "import", "from", "def", "class", "return", "if", "else", "for", "in",
  "not", "and", "or", "True", "False", "None", "print", "as", "with",
  "async", "await", "lambda", "yield",
]);
const TS_KW = new Set([
  "const", "let", "var", "async", "await", "function", "return", "if",
  "else", "for", "of", "in", "import", "from", "export", "default",
  "true", "false", "null", "undefined", "interface", "type", "new",
]);

interface Token { kind: "kw" | "str" | "comment" | "num" | "fn" | "plain"; text: string }

function tokenize(code: string, lang: "python" | "typescript" | "curl"): Token[] {
  const tokens: Token[] = [];
  const keywords = lang === "python" ? PY_KW : TS_KW;
  const commentChar = lang === "python" || lang === "curl" ? "#" : "//";
  let i = 0;

  while (i < code.length) {
    // Line comment
    if (code.startsWith(commentChar, i)) {
      const end = code.indexOf("\n", i);
      const t = end === -1 ? code.slice(i) : code.slice(i, end);
      tokens.push({ kind: "comment", text: t });
      i += t.length;
      continue;
    }
    // String (single, double, backtick)
    if (`"'\``.includes(code[i])) {
      const q = code[i];
      let j = i + 1;
      while (j < code.length && code[j] !== q) {
        if (code[j] === "\\" && j + 1 < code.length) j += 2;
        else j++;
      }
      j++; // closing quote
      tokens.push({ kind: "str", text: code.slice(i, j) });
      i = j;
      continue;
    }
    // Number
    if (/\d/.test(code[i]) || (code[i] === "-" && /\d/.test(code[i + 1] ?? ""))) {
      let j = i;
      if (code[j] === "-") j++;
      while (j < code.length && /[\d.eE+\-]/.test(code[j])) j++;
      tokens.push({ kind: "num", text: code.slice(i, j) });
      i = j;
      continue;
    }
    // Word / keyword / function call
    if (/[a-zA-Z_]/.test(code[i])) {
      let j = i;
      while (j < code.length && /[a-zA-Z0-9_]/.test(code[j])) j++;
      const word = code.slice(i, j);
      const isFn = code[j] === "(";
      tokens.push({
        kind: keywords.has(word) ? "kw" : isFn ? "fn" : "plain",
        text: word,
      });
      i = j;
      continue;
    }
    // Plain char
    const last = tokens.at(-1);
    if (last && last.kind === "plain") last.text += code[i];
    else tokens.push({ kind: "plain", text: code[i] });
    i++;
  }
  return tokens;
}

// Two full palettes rather than one set of "works everywhere" colors: the
// dark set is tuned for a near-black background, the light set for
// near-white — reusing one hex for both leaves "plain" (the majority of any
// snippet — punctuation, whitespace-adjacent identifiers) illegible in
// whichever theme it wasn't tuned for.
const TOKEN_COLOR_DARK: Record<Token["kind"], string> = {
  kw:      "#38bdf8",   // sky
  str:     "#4ade80",   // emerald
  comment: "#71717a",   // zinc-500
  num:     "#fb923c",   // orange
  fn:      "#fbbf24",   // amber
  plain:   "#d4d4d8",   // zinc-300
};
const TOKEN_COLOR_LIGHT: Record<Token["kind"], string> = {
  kw:      "#0369a1",   // sky-700
  str:     "#15803d",   // emerald-700
  comment: "#71717a",   // zinc-500
  num:     "#c2410c",   // orange-700
  fn:      "#a16207",   // amber-700
  plain:   "#3f3f46",   // zinc-700
};

function SyntaxCode({ code, lang }: { code: string; lang: "python" | "typescript" | "curl" }) {
  const { theme } = useTheme();
  const tokenColor = theme === "light" ? TOKEN_COLOR_LIGHT : TOKEN_COLOR_DARK;
  const tokens = tokenize(code, lang);
  return (
    <pre className="font-mono text-[12.5px] leading-[1.65] p-5 overflow-x-auto">
      {tokens.map((t, i) => (
        <span key={i} style={{ color: tokenColor[t.kind] }}>
          {t.text}
        </span>
      ))}
    </pre>
  );
}

// -- Copy button ---------------------------------------------------------------
function CopyBtn({ text, label = "copy" }: { text: string; label?: string }) {
  const [done, setDone] = useState(false);
  const copy = useCallback(async () => {
    await navigator.clipboard.writeText(text);
    setDone(true);
    setTimeout(() => setDone(false), 1500);
  }, [text]);
  return (
    <button
      onClick={copy}
      className={`text-[11px] px-3 py-1 rounded border transition-all ${
        done
          ? "border-emerald-700 bg-emerald-950/50 text-emerald-400"
          : "border-input text-muted-foreground hover:text-foreground hover:border-ring bg-card"
      }`}
    >
      {done ? "✓ copied" : label}
    </button>
  );
}

// -- Main panel ----------------------------------------------------------------
export function CodePanel({
  isOpen,
  onClose,
  queryVector,
  queryText,
  k,
  collection,
  result,
  embedProvider,
  embedModel,
  embedEndpoint,
}: Props) {
  const [lang, setLang] = useState<Lang>("python");
  const panelRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!isOpen) return;
    // Move focus into the panel on open
    const firstFocusable = panelRef.current?.querySelector<HTMLElement>(
      'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
    );
    firstFocusable?.focus();

    function onKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") { onClose(); return; }
      if (e.key !== "Tab") return;
      const panel = panelRef.current;
      if (!panel) return;
      const focusable = Array.from(
        panel.querySelectorAll<HTMLElement>(
          'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
        )
      ).filter((el) => !el.hasAttribute("disabled"));
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    }
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [isOpen, onClose]);

  if (!isOpen) return null;

  const cosine = cosineFromScore(result.score);

  const code = lang === "python"
    ? genPython(queryVector, k, collection, result, embedProvider, embedModel, embedEndpoint, queryText)
    : lang === "typescript"
    ? genTypeScript(queryVector, k, collection, result, embedProvider, embedModel, embedEndpoint, queryText)
    : genCurl(queryVector, k, collection, result);

  const TABS: { key: Lang; label: string }[] = [
    { key: "python",     label: "Python" },
    { key: "typescript", label: "TypeScript" },
    { key: "curl",       label: "curl" },
  ];

  return (
    <>
      {/* Backdrop */}
      <div
        className="fixed inset-0 z-40 bg-black/40 backdrop-blur-[2px]"
        onClick={onClose}
      />

      {/* Panel */}
      <div ref={panelRef} role="dialog" aria-modal="true" aria-label="Code generation panel" className="fixed right-0 top-0 bottom-0 z-50 w-[560px] flex flex-col bg-background border-l border-border shadow-2xl">

        {/* Header */}
        <div className="flex items-center justify-between px-5 py-3 border-b border-border flex-shrink-0">
          <div className="flex flex-col gap-0.5">
            <span className="font-mono text-sm text-foreground">
              record <span className="text-amber-400">#{result.id}</span>
            </span>
            <span className="text-[11px] text-muted-foreground font-mono">
              L2² {result.score.toExponential(3)} · cosine ≈{" "}
              <span className={cosine >= 85 ? "text-emerald-400" : cosine >= 70 ? "text-amber-400" : "text-muted-foreground"}>
                {cosine.toFixed(1)}%
              </span>
              {" · "}k={k} · {queryVector.length} dims
            </span>
          </div>
          <button
            onClick={onClose}
            className="text-muted-foreground hover:text-card-foreground transition-colors text-xl leading-none"
            aria-label="Close"
          >
            ×
          </button>
        </div>

        {/* Tabs */}
        <div className="flex items-center gap-0 px-5 pt-3 pb-0 border-b border-border flex-shrink-0">
          {TABS.map((t) => (
            <button
              key={t.key}
              onClick={() => setLang(t.key)}
              className={`px-4 py-2 text-xs font-mono border-b-2 transition-colors -mb-px ${
                lang === t.key
                  ? "border-sky-500 text-sky-400"
                  : "border-transparent text-muted-foreground hover:text-accent-foreground"
              }`}
            >
              {t.label}
            </button>
          ))}
          <div className="flex-1" />
        </div>

        {/* Code area */}
        <div className="flex-1 overflow-y-auto bg-background">
          <SyntaxCode code={code} lang={lang === "curl" ? "curl" : lang} />
        </div>

        {/* Footer */}
        <div className="flex items-center gap-2 px-4 py-3 border-t border-border flex-shrink-0 flex-wrap">
          <CopyBtn text={code} label="copy code" />
          <CopyBtn text={fullVecJSON(queryVector)} label={`copy full vector (${queryVector.length} dims)`} />
          <span className="ml-auto text-[10px] text-muted-foreground font-mono">
            {collection}
          </span>
        </div>
      </div>
    </>
  );
}

import { EmbeddingProvider } from "./provider";

export class OllamaProvider implements EmbeddingProvider {
  private url: string;
  private model: string;

  constructor(url: string = "http://localhost:11434", model: string = "nomic-embed-text") {
    this.url = url;
    this.model = model;
  }

  async embedText(text: string): Promise<number[]> {
    const safeText = text.slice(0, 1800);
    const res = await fetch(`${this.url}/api/embeddings`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ model: this.model, prompt: safeText }),
    });
    if (!res.ok) throw new Error(`Ollama error: ${res.statusText}`);
    const data = await res.json();
    return data.embedding;
  }

  async embedBatch(texts: string[]): Promise<number[][]> {
    const results: number[][] = [];
    for (const text of texts) {
      results.push(await this.embedText(text));
    }
    return results;
  }
}

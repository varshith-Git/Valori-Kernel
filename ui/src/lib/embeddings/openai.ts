import OpenAI from "openai";
import { EmbeddingProvider } from "./provider";

export class OpenAIProvider implements EmbeddingProvider {
  private client: OpenAI;
  private model: string;
  private dimensions?: number;

  constructor(apiKey?: string, model: string = "text-embedding-3-small", dimensions?: number) {
    this.client = new OpenAI({ apiKey: apiKey || process.env.OPENAI_API_KEY });
    this.model = model;
    this.dimensions = dimensions;
  }

  async embedText(text: string): Promise<number[]> {
    const res = await this.client.embeddings.create({
      model: this.model,
      input: text,
      dimensions: this.dimensions,
    });
    return res.data[0].embedding;
  }

  async embedBatch(texts: string[]): Promise<number[][]> {
    const res = await this.client.embeddings.create({
      model: this.model,
      input: texts,
      dimensions: this.dimensions,
    });
    return res.data.map(d => d.embedding);
  }
}

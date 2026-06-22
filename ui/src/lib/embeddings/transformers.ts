import { pipeline, env } from "@xenova/transformers";
import { EmbeddingProvider } from "./provider";

// We disable local models directory to ensure it reliably fetches from HuggingFace Hub
env.allowLocalModels = false;

export class TransformersProvider implements EmbeddingProvider {
  private modelName: string;
  // Cache the pipeline across requests to prevent downloading/loading on every call
  private static pipelines = new Map<string, any>();

  constructor(modelName: string = "Xenova/all-MiniLM-L6-v2") {
    // We expect a valid HF model id, typically prefixed with Xenova/ for JS compatibility
    this.modelName = modelName;
  }

  private async getPipeline() {
    if (!TransformersProvider.pipelines.has(this.modelName)) {
      // feature-extraction is the pipeline type for embeddings
      const extractor = await pipeline("feature-extraction", this.modelName);
      TransformersProvider.pipelines.set(this.modelName, extractor);
    }
    return TransformersProvider.pipelines.get(this.modelName);
  }

  async embedText(text: string): Promise<number[]> {
    const extractor = await this.getPipeline();
    const output = await extractor(text, { pooling: "mean", normalize: true });
    return Array.from(output.data);
  }

  async embedBatch(texts: string[]): Promise<number[][]> {
    const extractor = await this.getPipeline();
    const output = await extractor(texts, { pooling: "mean", normalize: true });
    
    // The output is a Tensor. tolist() returns a deeply nested array
    const list = output.tolist();
    
    // Depending on the batch processing, it might return [batch_size, seq_length, hidden_dim]
    // or [batch_size, hidden_dim] because we used pooling: "mean". 
    // tolist() with pooling usually gives [batch, hidden_dim].
    return list;
  }
}

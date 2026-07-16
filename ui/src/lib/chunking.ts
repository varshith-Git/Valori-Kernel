export interface ChunkingOptions {
  chunkSize: number;
  chunkOverlap: number;
}

export function recursiveCharacterSplit(text: string, options: ChunkingOptions): string[] {
  let { chunkSize, chunkOverlap } = options;

  // Enforce sensible guardrails without restricting large-context models
  chunkSize = Math.max(100, Math.min(100000, chunkSize));
  chunkOverlap = Math.min(Math.floor(chunkSize / 2), Math.max(0, chunkOverlap));

  // The separators to try, from most semantic (paragraphs) to least (characters)
  const separators = ["\n\n", "\n", " ", ""];

  return splitText(text, chunkSize, chunkOverlap, separators);
}

function splitText(text: string, chunkSize: number, chunkOverlap: number, separators: string[]): string[] {
  if (text.length <= chunkSize) {
    return [text];
  }

  const separator = separators.find(s => text.includes(s)) ?? "";
  const splits = text.split(separator);

  const chunks: string[] = [];
  let currentChunk = "";

  for (const split of splits) {
    if ((currentChunk.length + split.length + separator.length) > chunkSize) {
      if (currentChunk) {
        chunks.push(currentChunk.trim());
      }
      // Start new chunk with overlap from the end of the previous chunk
      if (chunkOverlap > 0 && currentChunk.length > chunkOverlap) {
         currentChunk = currentChunk.slice(-chunkOverlap) + split + separator;
      } else {
         currentChunk = split + separator;
      }
    } else {
      currentChunk += split + separator;
    }
  }

  if (currentChunk) {
    chunks.push(currentChunk.trim());
  }

  return chunks;
}

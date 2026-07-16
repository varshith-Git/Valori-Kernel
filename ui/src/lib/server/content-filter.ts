/** Returns true for bibliography / reference-list chunks that should be deprioritised. */
export function isReferenceChunk(text: string): boolean {
  const inlineCitations = (text.match(/\[\d{1,3}\]/g) ?? []).length;
  if (inlineCitations >= 3) return true;

  const urls = (text.match(/https?:\/\//g) ?? []).length;
  if (urls >= 2) return true;

  const lines = text.split("\n").filter(Boolean);
  if (lines.length >= 2) {
    const citLines = lines.filter((l) => /^\[\d+\]/.test(l.trim()));
    if (citLines.length / lines.length > 0.25) return true;
  }

  const firstChars = text.trim().slice(0, 120).toLowerCase();
  if (
    inlineCitations >= 1 &&
    /^[a-z]/.test(text.trim()) &&
    (firstChars.includes("conference") || firstChars.includes("proceedings") ||
     firstChars.includes("arxiv") || firstChars.includes("preprint"))
  ) return true;

  return false;
}

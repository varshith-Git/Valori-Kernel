"use client";

interface Props {
  hash: string | null;
  chainHeight: number | null;
}

export function ProofExport({ hash, chainHeight }: Props) {
  const download = () => {
    if (!hash) return;
    const payload = {
      final_state_hash: hash,
      chain_height: chainHeight,
      exported_at: new Date().toISOString(),
      algorithm: "BLAKE3",
      format: "Q16.16 fixed-point",
    };
    const blob = new Blob([JSON.stringify(payload, null, 2)], {
      type: "application/json",
    });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = `valori-proof-${Date.now()}.json`;
    a.click();
    URL.revokeObjectURL(a.href);
  };

  return (
    <button
      onClick={download}
      disabled={!hash}
      className="rounded-md border border-input px-3 py-1.5 text-xs text-accent-foreground hover:bg-accent hover:text-foreground transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
    >
      Export proof JSON
    </button>
  );
}

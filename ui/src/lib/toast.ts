export type ToastKind = "error" | "success" | "warning";

export interface ToastPayload {
  id: string;
  kind: ToastKind;
  message: string;
}

export function toast(message: string, kind: ToastKind = "error") {
  if (typeof window === "undefined") return;
  const id = Math.random().toString(36).slice(2, 9);
  window.dispatchEvent(
    new CustomEvent<ToastPayload>("valori:toast", { detail: { id, kind, message } })
  );
}

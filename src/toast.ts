export type ToastItem = {
  id: number;
  message: string;
};

const TOAST_EVENT = "wavetrace-toast";

/** Show a non-blocking in-app toast (replaces browser alert). */
export function showToast(message: string): void {
  const detail: ToastItem = { id: Date.now() + Math.random(), message };
  window.dispatchEvent(new CustomEvent(TOAST_EVENT, { detail }));
}

export function subscribeToasts(listener: (toast: ToastItem) => void): () => void {
  const handler = (event: Event) => {
    listener((event as CustomEvent<ToastItem>).detail);
  };
  window.addEventListener(TOAST_EVENT, handler);
  return () => window.removeEventListener(TOAST_EVENT, handler);
}

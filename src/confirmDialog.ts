export type ConfirmRequest = {
  id: number;
  title: string;
  message: string;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
};

const CONFIRM_EVENT = "wavetrace-confirm";

type PendingConfirm = {
  request: ConfirmRequest;
  resolve: (confirmed: boolean) => void;
};

let pending: PendingConfirm | null = null;

/** In-app confirm dialog (replaces browser confirm). */
export function confirmDialog(
  options: Omit<ConfirmRequest, "id">,
): Promise<boolean> {
  return new Promise((resolve) => {
    if (pending) {
      pending.resolve(false);
    }
    const request: ConfirmRequest = {
      id: Date.now() + Math.random(),
      confirmLabel: "OK",
      cancelLabel: "Cancel",
      ...options,
    };
    pending = { request, resolve };
    window.dispatchEvent(new CustomEvent(CONFIRM_EVENT, { detail: request }));
  });
}

export function settleConfirm(id: number, confirmed: boolean): void {
  if (!pending || pending.request.id !== id) {
    return;
  }
  pending.resolve(confirmed);
  pending = null;
}

export function subscribeConfirmRequests(
  listener: (request: ConfirmRequest) => void,
): () => void {
  const handler = (event: Event) => {
    listener((event as CustomEvent<ConfirmRequest>).detail);
  };
  window.addEventListener(CONFIRM_EVENT, handler);
  return () => window.removeEventListener(CONFIRM_EVENT, handler);
}

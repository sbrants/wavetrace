import { useEffect, useState } from "react";
import { subscribeToasts, type ToastItem } from "../toast";

const TOAST_MS = 10_000;

export default function ToastStack() {
  const [toasts, setToasts] = useState<ToastItem[]>([]);

  useEffect(() => {
    return subscribeToasts((toast) => {
      setToasts((prev) => [...prev, toast]);
      window.setTimeout(() => {
        setToasts((prev) => prev.filter((t) => t.id !== toast.id));
      }, TOAST_MS);
    });
  }, []);

  if (toasts.length === 0) {
    return null;
  }

  return (
    <div className="toast-stack" aria-live="polite" aria-relevant="additions">
      {toasts.map((toast) => (
        <div key={toast.id} className="toast" role="alert">
          <p className="toast-message">{toast.message}</p>
          <button
            type="button"
            className="toast-dismiss"
            aria-label="Dismiss"
            onClick={() =>
              setToasts((prev) => prev.filter((t) => t.id !== toast.id))
            }
          >
            ×
          </button>
        </div>
      ))}
    </div>
  );
}

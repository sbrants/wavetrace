import { useEffect, useRef, useState } from "react";
import {
  settleConfirm,
  subscribeConfirmRequests,
  type ConfirmRequest,
} from "../confirmDialog";

export default function ConfirmDialog() {
  const [request, setRequest] = useState<ConfirmRequest | null>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    return subscribeConfirmRequests((next) => setRequest(next));
  }, []);

  useEffect(() => {
    if (request) {
      cancelRef.current?.focus();
    }
  }, [request]);

  useEffect(() => {
    if (!request) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        settleConfirm(request.id, false);
        setRequest(null);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [request]);

  if (!request) {
    return null;
  }

  const close = (confirmed: boolean) => {
    settleConfirm(request.id, confirmed);
    setRequest(null);
  };

  return (
    <div
      className="confirm-overlay"
      role="presentation"
      onClick={() => close(false)}
    >
      <div
        className="confirm-dialog"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="confirm-dialog-title"
        aria-describedby="confirm-dialog-message"
        onClick={(e) => e.stopPropagation()}
      >
        <h2 id="confirm-dialog-title" className="confirm-dialog-title">
          {request.title}
        </h2>
        <p id="confirm-dialog-message" className="confirm-dialog-message">
          {request.message}
        </p>
        <div className="confirm-dialog-actions">
          <button
            ref={cancelRef}
            type="button"
            onClick={() => close(false)}
          >
            {request.cancelLabel ?? "Cancel"}
          </button>
          <button
            type="button"
            className={request.danger ? "danger" : "primary"}
            onClick={() => close(true)}
          >
            {request.confirmLabel ?? "OK"}
          </button>
        </div>
      </div>
    </div>
  );
}

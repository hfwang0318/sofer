import { useEffect } from "react";

import { Icon } from "./Icon";
import type { ToastMessage } from "../lib/types";

interface ToastProps {
  toast: ToastMessage;
  onDismiss: () => void;
}

export function Toast({ toast, onDismiss }: ToastProps) {
  useEffect(() => {
    const timer = window.setTimeout(onDismiss, toast.tone === "error" ? 5200 : 3200);
    return () => window.clearTimeout(timer);
  }, [toast.id, toast.tone, onDismiss]);

  return (
    <div className={`toast tone-${toast.tone}`} role="status" aria-live="polite">
      <span className="toast-icon">
        <Icon
          name={toast.tone === "error" ? "alert" : toast.tone === "ok" ? "check" : "info"}
          size={15}
        />
      </span>
      <div className="toast-copy">
        <strong>{toast.title}</strong>
        {toast.detail && <span>{toast.detail}</span>}
      </div>
      <button type="button" className="toast-close" aria-label="Dismiss" onClick={onDismiss}>
        <Icon name="close" size={13} />
      </button>
    </div>
  );
}

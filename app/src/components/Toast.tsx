import { useEffect } from "react";

import { Icon } from "./Icon";
import type { ToastMessage } from "../lib/types";

interface ToastProps {
  toast: ToastMessage;
  onDismiss: () => void;
}

export function Toast({ toast, onDismiss }: ToastProps) {
  useEffect(() => {
    const timer = window.setTimeout(onDismiss, toast.tone === "error" ? 6000 : 3600);
    return () => window.clearTimeout(timer);
  }, [toast.id, toast.tone, onDismiss]);

  const isError = toast.tone === "error";
  return (
    <div
      className={`toast tone-${toast.tone}`}
      role={isError ? "alert" : "status"}
      aria-live={isError ? "assertive" : "polite"}
    >
      <span className="toast-icon">
        <Icon
          name={isError ? "alert" : toast.tone === "ok" ? "check" : "info"}
          size={15}
        />
      </span>
      <div className="toast-copy">
        <strong>{toast.title}</strong>
        {toast.detail && <span>{toast.detail}</span>}
      </div>
      <button type="button" className="toast-close" aria-label="关闭提示" onClick={onDismiss}>
        <Icon name="close" size={13} />
      </button>
    </div>
  );
}

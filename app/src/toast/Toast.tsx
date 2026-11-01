import { useCallback, useEffect, useRef, useState } from "react";

import { Icon } from "../components/Icon";
import { onToast, toastDismiss, toastInfo } from "../lib/bridge";
import type { ToastPayload } from "../lib/types";

/** Visible time from docs/ui-guidelines.md, then the fade below. */
const VISIBLE_MS = 2500;
const FADE_MS = 220;

/**
 * The whole toast window: one line, no interaction. Rust decides when it
 * appears and where it is; this page only renders and times the fade-out.
 */
export function Toast() {
  const [payload, setPayload] = useState<ToastPayload | null>(null);
  const [leaving, setLeaving] = useState(false);
  const visibleTimer = useRef<number | null>(null);
  const fadeTimer = useRef<number | null>(null);

  const clearTimers = useCallback(() => {
    if (visibleTimer.current !== null) window.clearTimeout(visibleTimer.current);
    if (fadeTimer.current !== null) window.clearTimeout(fadeTimer.current);
    visibleTimer.current = null;
    fadeTimer.current = null;
  }, []);

  const present = useCallback(
    (next: ToastPayload) => {
      // A second capture restarts the clock instead of stacking a window.
      clearTimers();
      setLeaving(false);
      setPayload(next);
      visibleTimer.current = window.setTimeout(() => {
        setLeaving(true);
        fadeTimer.current = window.setTimeout(() => {
          setPayload(null);
          setLeaving(false);
          void toastDismiss(next.id);
        }, FADE_MS);
      }, VISIBLE_MS);
    },
    [clearTimers],
  );

  useEffect(() => {
    let active = true;
    // Covers the first show: the event may have been emitted while this page
    // was still loading.
    void toastInfo().then((info) => {
      if (active && info) present(info);
    });
    const stop = onToast((next) => present(next));
    return () => {
      active = false;
      void stop.then((unlisten) => unlisten());
      clearTimers();
    };
  }, [present, clearTimers]);

  if (!payload) return <div className="bottom-toast" aria-hidden="true" />;

  return (
    <div className={`bottom-toast${leaving ? " is-leaving" : ""}`}>
      <div key={payload.id} className="toast-pill" role="status" aria-live="polite">
        <span className="toast-check">
          <Icon name="check" size={13} />
        </span>
        <span>已复制</span>
        <span className="toast-separator" aria-hidden="true">
          ·
        </span>
        <span className="toast-count">{payload.lines} 行</span>
      </div>
    </div>
  );
}

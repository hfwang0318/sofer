import { useEffect, useRef } from "react";

import { Icon } from "./Icon";
import type { ScreenAccess } from "../lib/types";

interface PermissionDialogProps {
  open: boolean;
  access: ScreenAccess;
  busy: boolean;
  onRequest: () => void;
  onOpenSettings: () => void;
  onRecheck: () => void;
  onRestart: () => void;
  onCapture: () => void;
  onClose: () => void;
}

export function PermissionDialog({
  open,
  access,
  busy,
  onRequest,
  onOpenSettings,
  onRecheck,
  onRestart,
  onCapture,
  onClose,
}: PermissionDialogProps) {
  const dialogRef = useRef<HTMLElement>(null);
  const primaryRef = useRef<HTMLButtonElement>(null);
  const closeRef = useRef(onClose);
  const granted = access === "granted" || access === "not-needed";

  useEffect(() => {
    closeRef.current = onClose;
  }, [onClose]);

  useEffect(() => {
    if (!open) return;
    const primary = primaryRef.current;
    if (primary && !primary.disabled) primary.focus();
    else dialogRef.current?.focus();
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") closeRef.current();
      if (event.key !== "Tab" || !dialogRef.current) return;
      const focusables = Array.from(
        dialogRef.current.querySelectorAll<HTMLElement>(
          'button:not(:disabled), [href], input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])',
        ),
      ).filter((element) => element.offsetParent !== null);
      if (focusables.length === 0) return;
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      if (!dialogRef.current.contains(document.activeElement)) {
        // Permission can be granted while the dialog is open; the focused
        // button may be gone, so keep Tab inside the dialog.
        event.preventDefault();
        first.focus();
      } else if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, granted]);

  useEffect(() => {
    if (!open) return;
    const previous = document.activeElement as HTMLElement | null;
    return () => {
      if (previous?.isConnected) previous.focus();
    };
  }, [open]);

  if (!open) return null;

  return (
    <div className="setup-backdrop" role="presentation">
      <section
        ref={dialogRef}
        className="setup-dialog permission-dialog"
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-labelledby="permission-title"
      >
        <button type="button" className="setup-close" aria-label="关闭" onClick={onClose}>
          <Icon name="close" size={15} />
        </button>
        <div className="setup-mark" aria-hidden="true">
          <Icon name={granted ? "check" : "capture"} size={20} />
        </div>

        {granted ? (
          <>
            <p className="eyebrow">屏幕录制权限</p>
            <h2 id="permission-title">权限已开启</h2>
            <p className="setup-lede">现在可以框选屏幕上的代码了。</p>
            <div className="setup-actions">
              <button
                ref={primaryRef}
                type="button"
                className="btn btn-primary"
                onClick={onCapture}
              >
                <Icon name="capture" size={15} />
                截图
              </button>
              <button type="button" className="btn btn-secondary" onClick={onClose}>
                完成
              </button>
            </div>
          </>
        ) : (
          <>
            <p className="eyebrow">需要权限</p>
            <h2 id="permission-title">允许 ocr4code 录制屏幕</h2>
            <p className="setup-lede">
              没有这项权限，截出来会是空白图，识别也不会成功。
            </p>

            <div className="setup-checklist">
              <div className="setup-row">
                <span className="setup-row-icon tone-warn">
                  <Icon name="info" size={15} />
                </span>
                <div className="setup-row-copy">
                  <span className="setup-row-title">1. 点“授权”</span>
                  <span className="setup-row-detail">在系统弹窗里选择“打开系统设置”。</span>
                </div>
              </div>
              <div className="setup-row">
                <span className="setup-row-icon tone-warn">
                  <Icon name="info" size={15} />
                </span>
                <div className="setup-row-copy">
                  <span className="setup-row-title">2. 勾选 ocr4code</span>
                  <span className="setup-row-detail">
                    位置：隐私与安全性 → 屏幕录制。
                  </span>
                </div>
              </div>
              <div className="setup-row">
                <span className="setup-row-icon tone-warn">
                  <Icon name="info" size={15} />
                </span>
                <div className="setup-row-copy">
                  <span className="setup-row-title">3. 回来检查</span>
                  <span className="setup-row-detail">
                    状态没变就重启应用，权限会在重启后生效。
                  </span>
                </div>
              </div>
            </div>

            <div className="setup-actions setup-actions-wrap">
              <button
                ref={primaryRef}
                type="button"
                className="btn btn-primary"
                onClick={onRequest}
                disabled={busy}
              >
                <Icon name="refresh" size={15} />
                {busy ? "正在检查…" : "授权"}
              </button>
              <button type="button" className="btn btn-secondary" onClick={onOpenSettings}>
                打开系统设置
              </button>
              <button type="button" className="btn btn-secondary" onClick={onRecheck}>
                重新检查
              </button>
            </div>
            <div className="permission-foot">
              <button type="button" className="link-btn" onClick={onRestart}>
                重启应用
              </button>
              <button type="button" className="link-btn" onClick={onClose}>
                稍后再说
              </button>
            </div>
          </>
        )}
      </section>
    </div>
  );
}

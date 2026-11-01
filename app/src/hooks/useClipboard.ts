import { useCallback, useEffect, useRef } from "react";

import { copyText } from "../lib/bridge";
import type { OcrResult } from "../lib/types";
import type { PushToast } from "./useToast";

interface UseClipboardOptions {
  result: OcrResult | null;
  setCopied: (copied: boolean) => void;
  pushToast: PushToast;
}

export interface ClipboardActions {
  handleCopy: (text: string, _id?: string) => Promise<void>;
  handleCopyError: (text: string) => Promise<void>;
}

/** Copying the current result shows the in-place badge; everything else toasts. */
export function useClipboard({
  result,
  setCopied,
  pushToast,
}: UseClipboardOptions): ClipboardActions {
  const copiedTimer = useRef<number | null>(null);

  const handleCopy = useCallback(
    async (text: string, _id?: string) => {
      try {
        await copyText(text);
        if (result && text === result.text) {
          setCopied(true);
          if (copiedTimer.current) window.clearTimeout(copiedTimer.current);
          copiedTimer.current = window.setTimeout(() => setCopied(false), 1800);
        } else {
          pushToast("ok", "已复制到剪贴板");
        }
      } catch (cause) {
        pushToast(
          "error",
          "写入剪贴板失败",
          cause instanceof Error ? cause.message : String(cause),
        );
      }
    },
    [pushToast, result, setCopied],
  );

  const handleCopyError = useCallback(
    async (text: string) => {
      try {
        await copyText(text);
        pushToast("ok", "错误信息已复制", "可以贴到反馈里，方便排查。");
      } catch (cause) {
        pushToast(
          "error",
          "写入剪贴板失败",
          cause instanceof Error ? cause.message : String(cause),
        );
      }
    },
    [pushToast],
  );

  useEffect(() => {
    return () => {
      if (copiedTimer.current) window.clearTimeout(copiedTimer.current);
    };
  }, []);

  return { handleCopy, handleCopyError };
}

import { useCallback, useRef, useState } from "react";

import type { ToastMessage, ToastTone } from "../lib/types";

export type PushToast = (tone: ToastTone, title: string, detail?: string) => void;

export interface ToastController {
  toast: ToastMessage | null;
  pushToast: PushToast;
  dismissToast: () => void;
}

/** One app-wide toast slot; a new message always replaces the previous one. */
export function useToast(): ToastController {
  const [toast, setToast] = useState<ToastMessage | null>(null);
  const toastId = useRef(0);

  const pushToast = useCallback<PushToast>((tone, title, detail) => {
    toastId.current += 1;
    setToast({ id: toastId.current, tone, title, detail });
  }, []);

  const dismissToast = useCallback(() => setToast(null), []);

  return { toast, pushToast, dismissToast };
}

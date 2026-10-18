import { useEffect } from "react";
import { CheckCircle2, X } from "lucide-react";
import type { Toast as ToastState } from "../types.ts";

interface ToastProps {
  toast: ToastState | null;
  onDismiss: () => void;
  onAction: (action: NonNullable<ToastState["action"]>) => void;
}

const ACTION_LABELS: Record<NonNullable<ToastState["action"]>, string> = {
  backup: "View backups",
};

/** Confirmation for completed writes; the region stays mounted so it is announced. */
export function Toast({ toast, onDismiss, onAction }: ToastProps) {
  useEffect(() => {
    if (!toast) return;
    const timer = setTimeout(onDismiss, 8000);
    return () => clearTimeout(timer);
  }, [toast?.id]);

  return (
    <div role="status" aria-live="polite" className="fixed z-[60] top-20 right-4 left-4 sm:left-auto sm:w-96 pointer-events-none">
      {toast && (
        <div
          className="pointer-events-auto flex items-start gap-3 rounded-2xl border border-emerald-200 bg-white p-4 shadow-xl animate-in fade-in slide-in-from-top-2 duration-200"
          data-testid="toast"
        >
          <CheckCircle2 size={18} className="mt-0.5 shrink-0 text-emerald-600" aria-hidden="true" />
          <div className="min-w-0 flex-1 text-sm text-stone-800">
            <p>{toast.message}</p>
            {toast.action && (
              <button
                type="button"
                onClick={() => onAction(toast.action!)}
                className="mt-1 font-semibold text-stone-900 underline underline-offset-2 min-h-[32px]"
              >
                {ACTION_LABELS[toast.action]}
              </button>
            )}
          </div>
          <button
            type="button"
            onClick={onDismiss}
            aria-label="Dismiss"
            className="shrink-0 rounded-lg p-1 text-stone-500 hover:bg-stone-100 min-h-[32px] min-w-[32px] flex items-center justify-center"
          >
            <X size={14} aria-hidden="true" />
          </button>
        </div>
      )}
    </div>
  );
}

/**
 * FailureBanner.tsx - States what happened to the bike, then offers the next step.
 */

import { AlertCircle, X } from "lucide-react";
import type { InstrumentState } from "../types.ts";
import type { SessionController } from "../core/sessionController.ts";
import { describeFailure } from "../domain/errors.ts";
import { Button } from "./ui/Button.tsx";

interface FailureBannerProps {
  state: InstrumentState;
  controller: SessionController;
  onShowDiagnostics?: () => void;
}

export function FailureBanner({ state, controller, onShowDiagnostics }: FailureBannerProps) {
  if (!state.errorBanner) return null;
  const failure = describeFailure(state.errorBanner, state.failure, state.transport, state.schema);
  const connected = Boolean(state.identity);
  const busy = state.phase !== "ready";

  return (
    <div
      role="alert"
      className="p-4 rounded-2xl bg-red-50 border border-red-200 text-red-950 flex items-start gap-3 shadow-xs"
      data-testid="error-banner"
    >
      <AlertCircle size={20} className="text-red-600 shrink-0 mt-0.5" aria-hidden="true" />
      <div className="min-w-0 flex-1 text-sm">
        <p className="font-semibold">{failure.title}</p>
        {failure.outcome && <p className="mt-1 font-medium" data-testid="error-outcome">{failure.outcome}</p>}
        <p className="mt-1 text-red-800 [overflow-wrap:anywhere]">{failure.message}</p>
        {connected && (
          <div className="mt-3 flex flex-wrap gap-2">
            {failure.linkLost ? (
              <Button
                variant="primary"
                size="sm"
                disabled={busy}
                onClick={() => controller.reconnect().catch(() => {})}
                data-testid="btn-error-reconnect"
              >
                Reconnect
              </Button>
            ) : state.failure?.kind !== "connect" ? (
              <Button
                variant="primary"
                size="sm"
                disabled={busy}
                onClick={() => controller.readAllParameters().catch(() => {})}
                data-testid="btn-error-read"
              >
                Read settings from bike
              </Button>
            ) : null}
            {onShowDiagnostics && (
              <Button variant="outline" size="sm" onClick={onShowDiagnostics}>
                View details
              </Button>
            )}
          </div>
        )}
      </div>
      <button
        type="button"
        onClick={() => controller.dismissError()}
        aria-label="Dismiss error"
        className="shrink-0 rounded-lg p-1.5 text-red-700 hover:bg-red-100 min-h-[36px] min-w-[36px] flex items-center justify-center"
      >
        <X size={16} aria-hidden="true" />
      </button>
    </div>
  );
}

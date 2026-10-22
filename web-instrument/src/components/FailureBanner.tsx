/**
 * FailureBanner.tsx - States what happened to the bike, then offers the next step.
 */

import { Alert, Button, CloseButton } from "@heroui/react";
import type { InstrumentState } from "../types.ts";
import type { SessionController } from "../core/sessionController.ts";
import { describeFailure } from "../domain/errors.ts";

interface FailureBannerProps {
  state: InstrumentState;
  controller: SessionController;
  onShowDiagnostics?: () => void;
}

export function FailureBanner({ state, controller, onShowDiagnostics }: FailureBannerProps) {
  if (!state.failure) return null;
  const failure = describeFailure(state.failure, state.transport, state.schema);
  // A later rejection (e.g. staging an Undo) replaces the failure text, not the lost link.
  const linkLost = failure.linkLost || state.linkLost;
  const connected = Boolean(state.identity);
  const busy = state.phase !== "ready";

  return (
    <Alert status="danger" data-testid="error-banner">
      <Alert.Indicator />
      <Alert.Content className="min-w-0">
        <Alert.Title>{failure.title}</Alert.Title>
        {failure.outcome && <p className="mt-1 text-sm font-medium" data-testid="error-outcome">{failure.outcome}</p>}
        <Alert.Description className="[overflow-wrap:anywhere]">{failure.message}</Alert.Description>
        {connected && (
          <div className="mt-3 flex flex-wrap gap-2">
            {linkLost ? (
              <Button size="sm" isDisabled={busy} onPress={() => controller.reconnect().catch(() => {})} data-testid="btn-error-reconnect">
                Reconnect
              </Button>
            ) : !["connect", "stage"].includes(state.failure.kind) ? (
              <Button size="sm" isDisabled={busy} onPress={() => controller.readAllParameters().catch(() => {})} data-testid="btn-error-read">
                Read settings from bike
              </Button>
            ) : null}
            {onShowDiagnostics && (
              <Button variant="tertiary" size="sm" onPress={onShowDiagnostics}>
                View details
              </Button>
            )}
          </div>
        )}
      </Alert.Content>
      <CloseButton aria-label="Dismiss error" onPress={() => controller.dismissError()} />
    </Alert>
  );
}

import { Alert, Modal, Spinner } from "@heroui/react";
import type { InstrumentState, SessionPhase, TransportType } from "../../types.ts";

const TITLES: Partial<Record<SessionPhase, string>> = {
  disconnecting: "Disconnecting…",
  probing: "Discovering components…",
  reading: "Reading settings…",
  committing: "Writing to the bike…",
  flashing: "Updating firmware…",
};

function connectingTitle(transport: TransportType | null): string {
  switch (transport) {
    case "ble": return "Connecting over Bluetooth…";
    case "usb": return "Connecting over USB…";
    case "serial": return "Connecting over serial…";
    case "sim": return "Starting simulator…";
    default: return "Connecting…";
  }
}

/** What the operator must not do while this phase runs, per transport. */
function guidance(phase: SessionPhase, transport: TransportType | null, check: boolean): { text: string; critical: boolean } {
  if (phase === "disconnecting") {
    return { text: "Keep this page open until the connection closes. The bike may then take a few seconds to return to riding mode.", critical: false };
  }
  if (transport === "sim") {
    return { text: "Simulated bike components. No hardware is involved.", critical: false };
  }
  const link = transport === "ble"
    ? "Keep this page open and the bike powered on and in connection mode"
    : "Keep the cable connected and the bike powered on";
  if (phase === "flashing" && check) {
    return { text: `Nothing is written, but the motor enters its bootloader for this check. ${link} until it finishes.`, critical: true };
  }
  if (phase === "flashing") {
    return { text: `Firmware is being written. ${link} until this finishes, or the motor may need recovery.`, critical: true };
  }
  if (phase === "committing") {
    return { text: `${link} until the write finishes.`, critical: true };
  }
  return { text: `${link}.`, critical: false };
}

export function OperationModal({ state }: { state: InstrumentState }) {
  const busy = ["connecting", "disconnecting", "probing", "reading", "committing", "flashing"].includes(state.phase);
  // Steps aside for the passkey prompt so focus and the on-screen keyboard go to its field.
  const open = busy && !state.passkeyPromptOpen && !state.bluetoothSystemPrompt;
  const check = state.phase === "flashing" && state.firmware.operation === "check";
  const title = state.phase === "connecting"
    ? connectingTitle(state.transport)
    : check ? "Checking firmware compatibility…" : TITLES[state.phase] ?? "Working…";
  const notice = guidance(state.phase, state.transport, check);

  // Operations cannot be cancelled from here, so the dialog ignores outside clicks and Escape.
  return (
    <Modal>
      <Modal.Backdrop isOpen={open} isDismissable={false} isKeyboardDismissDisabled>
        <Modal.Container size="sm" placement="center">
          <Modal.Dialog role="alertdialog" data-testid="operation-modal" className="items-center text-center">
            <Modal.Header className="items-center">
              <Spinner size="lg" color="warning" aria-hidden="true" />
              <Modal.Heading>{title}</Modal.Heading>
            </Modal.Header>
            <Modal.Body className="flex flex-col gap-4">
              <p className="text-sm text-muted [overflow-wrap:anywhere]" aria-live="polite">
                {state.statusMessage}
              </p>
              <Alert status={notice.critical ? "warning" : "default"} className="text-left">
                <Alert.Indicator />
                <Alert.Content>
                  <Alert.Description className="text-xs leading-relaxed">{notice.text}</Alert.Description>
                </Alert.Content>
              </Alert>
            </Modal.Body>
          </Modal.Dialog>
        </Modal.Container>
      </Modal.Backdrop>
    </Modal>
  );
}

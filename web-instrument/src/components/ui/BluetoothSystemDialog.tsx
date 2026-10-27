import { Button, Modal } from "@heroui/react";
import type { InstrumentState } from "../../types.ts";
import type { SessionController } from "../../core/sessionController.ts";

export function BluetoothSystemDialog({ state, controller }: { state: InstrumentState; controller: SessionController }) {
  return (
    <Modal>
      <Modal.Backdrop isOpen={state.bluetoothSystemPrompt !== null} isDismissable={false}
        onOpenChange={(open) => !open && controller.answerBluetoothSystem(null)}>
        <Modal.Container size="sm" placement="center">
          <Modal.Dialog data-testid="bluetooth-system-dialog">
            <Modal.Header><Modal.Heading>Choose the bike system</Modal.Heading></Modal.Header>
            <Modal.Body className="flex flex-col gap-3">
              <p className="text-sm text-muted">{state.bluetoothSystemPrompt}</p>
              <p className="text-sm text-muted">STEPS bikes can also have Di2 components. Choose based on whether the bike has a STEPS motor.</p>
              <Button fullWidth className="min-h-11 h-auto py-3 whitespace-normal" onPress={() => controller.answerBluetoothSystem("steps")}>STEPS e-bike</Button>
              <Button fullWidth className="min-h-11 h-auto py-3 whitespace-normal" onPress={() => controller.answerBluetoothSystem("di2")}>Di2 shifting only (no STEPS motor)</Button>
            </Modal.Body>
            <Modal.Footer><Button variant="ghost" onPress={() => controller.answerBluetoothSystem(null)}>Cancel connection</Button></Modal.Footer>
          </Modal.Dialog>
        </Modal.Container>
      </Modal.Backdrop>
    </Modal>
  );
}

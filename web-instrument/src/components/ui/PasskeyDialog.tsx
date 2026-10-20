/**
 * PasskeyDialog.tsx - Asks for the 6-digit passkey of the Bluetooth gateway.
 *
 * Rendered by React (not a native <dialog>) so React Aria's overlay stack owns
 * focus: the field receives focus and the on-screen keyboard on mobile.
 */

import { useEffect, useState, type FormEvent } from "react";
import { Button, Form, Input, Label, Modal, TextField } from "@heroui/react";
import type { InstrumentState } from "../../types.ts";
import type { SessionController } from "../../core/sessionController.ts";

const PASSKEY = /^[0-9]{6}$/;

export function PasskeyDialog({ state, controller }: { state: InstrumentState; controller: SessionController }) {
  const [passkey, setPasskey] = useState("");
  const open = state.passkeyPromptOpen;

  useEffect(() => {
    if (!open) setPasskey("");
  }, [open]);

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (PASSKEY.test(passkey)) controller.answerPasskey(passkey);
  };

  return (
    <Modal>
      <Modal.Backdrop
        isOpen={open}
        isDismissable={false}
        onOpenChange={(next) => !next && controller.answerPasskey(null)}
      >
        <Modal.Container size="sm" placement="center">
          <Modal.Dialog data-testid="passkey-dialog">
            <Form onSubmit={submit} className="contents">
              <Modal.Header>
                <Modal.Heading>Bluetooth passkey</Modal.Heading>
                <p className="text-sm text-muted">Enter the 6-digit passkey for the Shimano E-TUBE Bluetooth gateway.</p>
              </Modal.Header>
              <Modal.Body>
                <TextField value={passkey} onChange={(value) => setPasskey(value.replace(/\D/g, "").slice(0, 6))} fullWidth autoFocus>
                  <Label>Passkey</Label>
                  <Input
                    type="password"
                    inputMode="numeric"
                    autoComplete="off"
                    maxLength={6}
                    className="font-mono tracking-[0.4em]"
                    style={{ fontSize: "20px" }}
                    data-testid="passkey-input"
                  />
                </TextField>
              </Modal.Body>
              <Modal.Footer className="flex-wrap">
                <Button variant="tertiary" onPress={() => controller.answerPasskey(null)} data-testid="btn-passkey-cancel">
                  Cancel
                </Button>
                <Button type="submit" isDisabled={!PASSKEY.test(passkey)} data-testid="btn-passkey-submit">
                  Authenticate
                </Button>
              </Modal.Footer>
            </Form>
          </Modal.Dialog>
        </Modal.Container>
      </Modal.Backdrop>
    </Modal>
  );
}

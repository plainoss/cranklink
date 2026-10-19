import { ShieldAlert } from "lucide-react";
import { Alert, Button, Modal } from "@heroui/react";

interface RiskWarningModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onDismiss: () => void;
}

export function RiskWarningModal({ open, onOpenChange, onDismiss }: RiskWarningModalProps) {
  return (
    <Modal>
      <Modal.Backdrop isOpen={open} onOpenChange={onOpenChange}>
        <Modal.Container size="lg" scroll="inside">
          <Modal.Dialog data-testid="risk-warning-modal">
            <Modal.CloseTrigger />
            <Modal.Header>
              <Modal.Heading>Unofficial Software &amp; Hardware Risk Notice</Modal.Heading>
            </Modal.Header>
            <Modal.Body className="flex flex-col gap-4 text-xs sm:text-sm leading-relaxed">
              <Alert status="warning">
                <Alert.Indicator />
                <Alert.Content>
                  <Alert.Description>
                    cranklink is an <strong>independent, unofficial open-source toolkit</strong> and is not affiliated with,
                    sponsored by, or endorsed by Shimano.
                  </Alert.Description>
                </Alert.Content>
              </Alert>

              <section>
                <h3 className="font-semibold mb-1 flex items-center gap-1.5">
                  <ShieldAlert size={16} className="text-danger" aria-hidden="true" />
                  <span>Hardware Risk &amp; Bricking Warning</span>
                </h3>
                <p className="text-muted">
                  Operating directly on drive unit hardware carries substantial risk. Using this software may{" "}
                  <strong>permanently brick or destroy drive units, displays, batteries, or other connected bike components</strong>,
                  and will likely void manufacturer warranties.
                </p>
              </section>

              <section className="border-t border-separator pt-3">
                <h3 className="font-semibold mb-1">Firmware Recovery &amp; Flashing Caveats</h3>
                <ul className="list-disc pl-5 space-y-1.5 text-muted">
                  <li>
                    <strong>Tested on EP800-CRG only:</strong> Flashing and bootloader recovery routines have{" "}
                    <strong>only been tested on a single motor model: DU-EP800-CRG</strong> (Series 0x23, Unit 0x00, DCASX board
                    1). Other models and revisions are completely untested.
                  </li>
                  <li>
                    <strong>No compatibility verification:</strong> cranklink <strong>does not verify firmware compatibility</strong>{" "}
                    against your specific drive unit or board revision. Flashing mismatched or corrupt containers (.dat) can
                    permanently brick the unit into an unrecoverable state.
                  </li>
                  <li>
                    <strong>Knowledgeable users only:</strong> Firmware operations are experimental and strictly intended for
                    technical users who know what they are doing.
                  </li>
                </ul>
              </section>

              <p className="border-t border-separator pt-3 text-xs text-muted">
                By proceeding, you acknowledge that you are using this software entirely at your own risk and accept full
                responsibility for any damage, hardware failure, or data loss.
              </p>
            </Modal.Body>
            <Modal.Footer>
              <Button onPress={onDismiss} data-testid="btn-dismiss-warning" className="w-full sm:w-auto">
                I Understand &amp; Accept the Risks
              </Button>
            </Modal.Footer>
          </Modal.Dialog>
        </Modal.Container>
      </Modal.Backdrop>
    </Modal>
  );
}

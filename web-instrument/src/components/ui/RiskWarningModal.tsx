import { AlertTriangle, ShieldAlert } from "lucide-react";
import { Modal } from "./Dialog.tsx";
import { Button } from "./Button.tsx";

interface RiskWarningModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onDismiss: () => void;
}

export function RiskWarningModal({
  open,
  onOpenChange,
  onDismiss,
}: RiskWarningModalProps) {
  return (
    <Modal
      open={open}
      onOpenChange={onOpenChange}
      title="Unofficial Software & Hardware Risk Notice"
      maxWidth="lg"
      data-testid="risk-warning-modal"
      footer={
        <Button
          variant="primary"
          size="md"
          onClick={onDismiss}
          data-testid="btn-dismiss-warning"
          className="w-full sm:w-auto bg-stone-900 hover:bg-stone-800 text-white min-h-[44px]"
        >
          I Understand &amp; Accept the Risks
        </Button>
      }
    >
      <div
        className="flex flex-col gap-4 text-xs sm:text-sm text-stone-700 leading-relaxed"
      >
        <div className="p-3.5 rounded-xl bg-amber-50 border border-amber-200 text-amber-950 flex items-start gap-3">
          <AlertTriangle size={20} className="text-amber-700 shrink-0 mt-0.5" />
          <p className="font-medium">
            cranklink is an <strong>independent, unofficial open-source toolkit</strong> and is not affiliated with, sponsored by, or endorsed by Shimano.
          </p>
        </div>

        <div className="space-y-3">
          <div>
            <h4 className="font-semibold text-stone-900 mb-1 flex items-center gap-1.5">
              <ShieldAlert size={16} className="text-red-600" />
              <span>Hardware Risk &amp; Bricking Warning</span>
            </h4>
            <p>
              Operating directly on drive unit hardware carries substantial risk. Using this software may <strong>permanently brick or destroy drive units, displays, batteries, or other connected bike components</strong>, and will likely void manufacturer warranties.
            </p>
          </div>

          <div className="border-t border-stone-100 pt-3">
            <h4 className="font-semibold text-stone-900 mb-1">
              Firmware Recovery &amp; Flashing Caveats
            </h4>
            <ul className="list-disc pl-5 space-y-1.5 text-stone-600">
              <li>
                <strong>Tested on EP800-CRG only:</strong> Flashing and bootloader recovery routines have <strong>only been tested on a single motor model: DU-EP800-CRG</strong> (Series 0x23, Unit 0x00, DCASX board 1). Other models and revisions are completely untested.
              </li>
              <li>
                <strong>No compatibility verification:</strong> cranklink <strong>does not verify firmware compatibility</strong> against your specific drive unit or board revision. Flashing mismatched or corrupt containers (.dat) can permanently brick the unit into an unrecoverable state.
              </li>
              <li>
                <strong>Knowledgeable users only:</strong> Firmware operations are experimental and strictly intended for technical users who know what they are doing.
              </li>
            </ul>
          </div>

          <div className="border-t border-stone-100 pt-3 text-stone-500 text-xs">
            By proceeding, you acknowledge that you are using this software entirely at your own risk and accept full responsibility for any damage, hardware failure, or data loss.
          </div>
        </div>
      </div>
    </Modal>
  );
}

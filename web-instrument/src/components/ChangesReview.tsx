/**
 * ChangesReview.tsx - Staged-change rail and the review dialog that every save uses.
 *
 * Every save goes through the review dialog: it lists each change in plain
 * words, lets the operator undo single rows, and asks for an explicit
 * acknowledgment when a change alters legal or factory limits.
 */

import { useEffect, useState } from "react";
import { AlertTriangle, RotateCcw, ShieldCheck } from "lucide-react";
import type { InstrumentState } from "../types.ts";
import type { SessionController } from "../core/sessionController.ts";
import { formatSetting, isRegulatedSetting, settingLabel } from "../domain/settingFormat.ts";
import { Alert, Button, Card, Checkbox, Label, Modal } from "@heroui/react";

interface StagedChange {
  name: string;
  label: string;
  before: string;
  after: string;
  regulated: boolean;
}

function stagedChanges(state: InstrumentState): StagedChange[] {
  return Object.entries(state.stagedValues).map(([name, value]) => {
    const spec = state.schema.find((setting) => setting.name === name);
    return {
      name,
      label: settingLabel(state.schema, name),
      before: formatSetting(state.schema, name, state.liveValues[name]?.value),
      after: formatSetting(state.schema, name, value),
      regulated: isRegulatedSetting(spec, name),
    };
  });
}

interface ChangesRailProps {
  state: InstrumentState;
  controller: SessionController;
  onReview: () => void;
}

/** Sticky summary at the bottom of the content column while changes are staged. */
export function ChangesRail({ state, controller, onReview }: ChangesRailProps) {
  const changes = stagedChanges(state);
  const count = changes.length;
  if (count === 0) return null;
  return (
    <Card
      render={(props) => <aside {...props} aria-label="Changes to save" />}
      className="sticky bottom-6 z-40 mt-auto flex-row items-center justify-between gap-4 bg-stone-900 text-white shadow-2xl"
      data-testid="transaction-rail"
    >
      <div className="min-w-0">
        <p className="text-sm font-semibold text-amber-300">
          {count} change{count === 1 ? "" : "s"} to save
        </p>
        <ul className="mt-1 text-xs text-stone-300">
          {changes.slice(0, 2).map((change) => (
            <li key={change.name} data-testid={`tx-item-${change.name}`} className="truncate">
              {change.label}: {change.before} → {change.after}
            </li>
          ))}
          {count > 2 && <li>and {count - 2} more</li>}
        </ul>
      </div>
      <div className="flex shrink-0 items-center gap-2">
        <Button variant="ghost" onPress={() => controller.discardAllStaged()} data-testid="btn-discard-staged" className="text-stone-300 hover:text-white">
          Discard all
        </Button>
        <Button onPress={onReview} data-testid="btn-review-save" className="bg-amber-400 text-stone-950 hover:bg-amber-300 font-bold">
          Review & save
        </Button>
      </div>
    </Card>
  );
}

interface ReviewDialogProps {
  state: InstrumentState;
  controller: SessionController;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/** The single save path: every write is reviewed here first. */
export function ReviewDialog({ state, controller, open, onOpenChange }: ReviewDialogProps) {
  const changes = stagedChanges(state);
  const count = changes.length;
  const regulated = changes.filter((change) => change.regulated);
  const [acknowledged, setAcknowledged] = useState(false);
  const saving = state.phase === "committing";

  useEffect(() => {
    if (count === 0) onOpenChange(false);
  }, [count, onOpenChange]);
  useEffect(() => {
    if (!open) setAcknowledged(false);
  }, [open]);

  const save = async () => {
    onOpenChange(false);
    await controller.commitStagedBatch().catch(() => {
      // The failure banner explains the bike's state; staged values remain.
    });
  };

  return (
    <Modal>
      <Modal.Backdrop isOpen={open && count > 0} onOpenChange={onOpenChange}>
        <Modal.Container size="lg" scroll="inside">
          <Modal.Dialog data-testid="review-dialog">
            <Modal.CloseTrigger />
            <Modal.Header>
              <Modal.Heading>{`Save ${count} change${count === 1 ? "" : "s"} to the bike?`}</Modal.Heading>
              <p className="text-sm text-muted">Check each change. You can undo any of them before saving.</p>
            </Modal.Header>
            <Modal.Body className="flex flex-col gap-4">
              <ul className="flex flex-col divide-y divide-separator rounded-xl border border-separator" aria-label="Staged changes">
                {changes.map((change) => (
                  <li key={change.name} className="flex items-center justify-between gap-3 px-3 py-2.5" data-testid={`review-item-${change.name}`}>
                    <div className="min-w-0">
                      <p className="text-sm font-medium flex items-center gap-1.5">
                        {change.regulated && <AlertTriangle size={14} className="text-warning shrink-0" aria-label="Regulated setting" />}
                        {change.label}
                      </p>
                      <p className="text-xs text-muted [overflow-wrap:anywhere]">
                        <span className="sr-only">from </span>
                        <span className="line-through">{change.before}</span>
                        <span aria-hidden="true"> → </span>
                        <span className="sr-only"> to </span>
                        <strong className="font-semibold text-foreground">{change.after}</strong>
                      </p>
                    </div>
                    <Button
                      variant="ghost"
                      size="sm"
                      onPress={() => controller.revertParameter(change.name)}
                      aria-label={`Undo change to ${change.label}`}
                      className="shrink-0"
                    >
                      <RotateCcw size={13} aria-hidden="true" />
                      Undo
                    </Button>
                  </li>
                ))}
              </ul>

              {regulated.length > 0 && (
                <Alert status="warning">
                  <Alert.Content>
                    <Checkbox isSelected={acknowledged} onChange={setAcknowledged} data-testid="ack-regulated">
                      <Checkbox.Control>
                        <Checkbox.Indicator />
                      </Checkbox.Control>
                      <Checkbox.Content>
                        <Label className="text-xs leading-relaxed">
                          {regulated.map((change) => change.label).join(" and ")} {regulated.length === 1 ? "changes" : "change"} legal
                          or factory limits. I confirm this is allowed where the bike is ridden and within what the manufacturer
                          approved.
                        </Label>
                      </Checkbox.Content>
                    </Checkbox>
                  </Alert.Content>
                </Alert>
              )}

              <p className="flex items-start gap-2 text-xs text-muted">
                <ShieldCheck size={14} className="mt-0.5 shrink-0 text-success" aria-hidden="true" />
                Each setting’s current value is backed up before it is written, so you can restore it from Backup & Restore.
              </p>
              {state.linkLost && (
                <p className="flex items-start gap-2 text-xs font-medium" data-testid="review-link-lost">
                  <AlertTriangle size={14} className="mt-0.5 shrink-0 text-warning" aria-hidden="true" />
                  The link to the bike closed. Close this review and reconnect before saving.
                </p>
              )}
            </Modal.Body>
            <Modal.Footer className="flex-wrap">
              <Button variant="ghost" onPress={() => controller.discardAllStaged()} className="mr-auto">
                Discard all
              </Button>
              <Button variant="tertiary" slot="close">
                Keep editing
              </Button>
              <Button
                isDisabled={saving || state.linkLost || (regulated.length > 0 && !acknowledged)}
                onPress={save}
                data-testid="btn-commit-batch"
              >
                Save to bike
              </Button>
            </Modal.Footer>
          </Modal.Dialog>
        </Modal.Container>
      </Modal.Backdrop>
    </Modal>
  );
}

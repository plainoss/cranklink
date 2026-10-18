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
import { Button } from "./ui/Button.tsx";
import { Modal } from "./ui/Dialog.tsx";

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
    <aside
      className="sticky bottom-6 z-40 mt-auto flex items-center justify-between gap-4 rounded-2xl border border-stone-700 bg-stone-900 p-4 text-white shadow-2xl"
      data-testid="transaction-rail"
      aria-label="Changes to save"
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
        <Button
          variant="custom"
          size="sm"
          onClick={() => controller.discardAllStaged()}
          data-testid="btn-discard-staged"
          className="bg-transparent text-stone-300 hover:bg-white/10 hover:text-white text-xs min-h-[44px]"
        >
          Discard all
        </Button>
        <Button
          variant="custom"
          size="sm"
          onClick={onReview}
          data-testid="btn-review-save"
          className="bg-amber-400 text-stone-950 hover:bg-amber-300 text-xs font-bold min-h-[44px]"
        >
          Review & save
        </Button>
      </div>
    </aside>
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
      <Modal
        open={open && count > 0}
        onOpenChange={onOpenChange}
        title={`Save ${count} change${count === 1 ? "" : "s"} to the bike?`}
        description="Check each change. You can undo any of them before saving."
        maxWidth="lg"
        data-testid="review-dialog"
        footer={
          <>
            <Button
              variant="ghost"
              size="md"
              onClick={() => controller.discardAllStaged()}
              className="mr-auto text-stone-600"
            >
              Discard all
            </Button>
            <Button variant="outline" size="md" onClick={() => onOpenChange(false)}>
              Keep editing
            </Button>
            <Button
              variant="primary"
              size="md"
              disabled={saving || (regulated.length > 0 && !acknowledged)}
              onClick={save}
              data-testid="btn-commit-batch"
            >
              Save to bike
            </Button>
          </>
        }
      >
        <ul className="flex flex-col divide-y divide-stone-200 rounded-xl border border-stone-200" aria-label="Staged changes">
          {changes.map((change) => (
            <li key={change.name} className="flex items-center justify-between gap-3 px-3 py-2.5" data-testid={`review-item-${change.name}`}>
              <div className="min-w-0">
                <p className="text-sm font-medium text-stone-900 flex items-center gap-1.5">
                  {change.regulated && <AlertTriangle size={14} className="text-amber-700 shrink-0" aria-label="Regulated setting" />}
                  {change.label}
                </p>
                <p className="text-xs text-stone-600 [overflow-wrap:anywhere]">
                  <span className="sr-only">from </span>
                  <span className="text-stone-500 line-through decoration-stone-400">{change.before}</span>
                  <span aria-hidden="true"> → </span>
                  <span className="sr-only"> to </span>
                  <strong className="font-semibold text-amber-800">{change.after}</strong>
                </p>
              </div>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => controller.revertParameter(change.name)}
                aria-label={`Undo change to ${change.label}`}
                className="shrink-0 gap-1 text-xs text-stone-600 min-h-[40px]"
              >
                <RotateCcw size={13} aria-hidden="true" />
                Undo
              </Button>
            </li>
          ))}
        </ul>

        {regulated.length > 0 && (
          <label className="mt-4 flex items-start gap-3 rounded-xl border border-amber-300 bg-amber-50 p-3 text-xs leading-relaxed text-amber-950 cursor-pointer">
            <input
              type="checkbox"
              checked={acknowledged}
              onChange={(event) => setAcknowledged(event.target.checked)}
              className="mt-0.5 h-4 w-4 shrink-0 accent-stone-900"
              data-testid="ack-regulated"
            />
            <span>
              {regulated.map((change) => change.label).join(" and ")} {regulated.length === 1 ? "changes" : "change"} legal
              or factory limits. I confirm this is allowed where the bike is ridden and within what the manufacturer approved.
            </span>
          </label>
        )}

        <p className="mt-4 flex items-start gap-2 text-xs text-stone-600">
          <ShieldCheck size={14} className="mt-0.5 shrink-0 text-emerald-700" aria-hidden="true" />
          Each setting’s current value is backed up before it is written, so you can restore it from Backup & Restore.
        </p>
      </Modal>
  );
}

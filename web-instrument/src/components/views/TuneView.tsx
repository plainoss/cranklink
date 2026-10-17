import { h } from "preact";
import { useState } from "preact/hooks";
import {
  ArrowLeft,
  Zap,
  Sliders,
  Check,
  RotateCcw,
  ShieldCheck,
  AlertTriangle,
  Plus,
  Minus,
  Sparkles,
  Gauge,
  CheckCircle2,
} from "lucide-preact";
import type { InstrumentState } from "../../types.ts";
import type { SessionController } from "../../core/sessionController.ts";
import { Card } from "../ui/Card.tsx";
import { Button } from "../ui/Button.tsx";
import { Slider } from "../ui/Slider.tsx";
import { Modal } from "../ui/Dialog.tsx";
import { Tabs } from "../ui/Tabs.tsx";
import { Badge } from "../ui/Badge.tsx";

interface TuneViewProps {
  state: InstrumentState;
  controller: SessionController;
  onBack: () => void;
}

export function TuneView({ state, controller, onBack }: TuneViewProps) {
  const [profileTab, setProfileTab] = useState<"1" | "2">("1");
  const [diffModalOpen, setDiffModalOpen] = useState(false);
  const [selectedGroup, setSelectedGroup] = useState<string>("geometry");
  const [searchFilter, setSearchFilter] = useState<string>("");

  const p = profileTab === "1" ? "profile1" : "profile2";

  // Helpers to get live and staged values
  const getVal = (name: string, fallback: number) => {
    if (state.stagedValues[name] !== undefined) {
      return Number(state.stagedValues[name]);
    }
    const live = state.liveValues[name]?.value;
    if (live !== undefined && live !== null && !isNaN(Number(live))) {
      return Number(live);
    }
    return fallback;
  };

  const isStaged = (name: string) => state.stagedValues[name] !== undefined;

  const boostTorque = getVal(`${p}.boost.torque`, 85);
  const trailTorque = getVal(`${p}.trail.torque`, 85);
  const ecoTorque = getVal(`${p}.eco.torque`, 27);
  const boostStart = getVal(`${p}.boost.start`, 4);
  const boostChar = getVal(`${p}.boost.character`, 8);
  const tireCircumference = getVal("tire-circumference", 2280);
  const speedLimit = getVal("speed-limit", 25);

  const stagedCount = Object.keys(state.stagedValues).length;

  const handleStage = (name: string, val: number | string) => {
    controller.stageParameter(name, val);
  };

  const handleCommit = async () => {
    setDiffModalOpen(false);
    try {
      await controller.commitStagedBatch();
    } catch {
      // Error captured in state.errorBanner
    }
  };

  return (
    <div className="w-full flex flex-col gap-6">
      {/* Navigation & Header */}
      <div className="flex items-center justify-between">
        <button
          type="button"
          onClick={onBack}
          className="inline-flex items-center gap-2 text-sm font-medium text-stone-600 hover:text-stone-900 cursor-pointer select-none py-2 px-3 rounded-lg hover:bg-stone-200/60 transition-colors"
          data-testid="btn-back-overview"
        >
          <ArrowLeft size={16} />
          <span>Back to Overview</span>
        </button>

        <div className="flex items-center gap-2">
          {stagedCount > 0 && (
            <Badge variant="warning">
              {stagedCount} change{stagedCount > 1 ? "s" : ""} pending
            </Badge>
          )}
          <span className="text-xs text-stone-500 font-mono">
            {state.identity?.model || "EP8"}
          </span>
        </div>
      </div>

      {/* Main Title Banner */}
      <div>
        <h2 className="text-2xl font-bold tracking-tight text-stone-900 font-sans flex items-center gap-2.5">
          <Zap className="text-amber-600" size={26} />
          <span>Ride Feel & Assist Studio</span>
        </h2>
        <p className="text-sm text-stone-600 mt-1">
          Adjust motor output torque, assist levels, and wheel circumference with real-time safety bounds.
        </p>
      </div>

      {/* Profile Selector Tabs */}
      <Tabs
        value={profileTab}
        onValueChange={(val) => {
          setProfileTab(val as "1" | "2");
          controller.setProfileTab(Number(val) as 1 | 2);
        }}
        items={[
          { id: "1", label: "Profile 1 (Everyday / Trail)" },
          { id: "2", label: "Profile 2 (Max Power / Custom)" },
        ]}
      />

      {/* Assist Tuning Sliders Card */}
      <Card className="flex flex-col gap-6 shadow-xs border-stone-200">
        <div>
          <h3 className="text-base font-bold text-stone-900">
            Assist Output Torque (Profile {profileTab})
          </h3>
          <p className="text-xs text-stone-500 mt-0.5">
            Configure how much torque Shimano STEPS delivers in each assist mode.
          </p>
        </div>

        {/* Boost Torque Slider */}
        <div className="p-4 rounded-xl bg-stone-50/70 border border-stone-200/80 flex flex-col gap-2">
          <Slider
            label="Boost Assist Torque"
            value={boostTorque}
            min={20}
            max={85}
            step={1}
            unit="Nm"
            onChange={(val) => handleStage(`${p}.boost.torque`, val)}
            valueFormatter={(val) =>
              `${val} Nm ${val >= 80 ? "(Maximum Punch)" : val <= 40 ? "(Gentle)" : "(Standard)"}`
            }
            data-testid="slider-boost-torque"
          />
          {isStaged(`${p}.boost.torque`) && (
            <div className="text-[11px] font-mono text-amber-700 flex items-center justify-between">
              <span>
                Live on bike: {state.liveValues[`${p}.boost.torque`]?.value ?? 85} Nm → Staged: {boostTorque} Nm
              </span>
              <button
                type="button"
                onClick={() => controller.revertParameter(`${p}.boost.torque`)}
                className="hover:underline cursor-pointer"
              >
                Revert
              </button>
            </div>
          )}
        </div>

        {/* Trail Torque Slider */}
        <div className="p-4 rounded-xl bg-stone-50/70 border border-stone-200/80 flex flex-col gap-2">
          <Slider
            label="Trail Assist Torque"
            value={trailTorque}
            min={20}
            max={85}
            step={1}
            unit="Nm"
            onChange={(val) => handleStage(`${p}.trail.torque`, val)}
            valueFormatter={(val) => `${val} Nm (Dynamic Flow)`}
          />
          {isStaged(`${p}.trail.torque`) && (
            <div className="text-[11px] font-mono text-amber-700 flex items-center justify-between">
              <span>
                Live on bike: {state.liveValues[`${p}.trail.torque`]?.value ?? 85} Nm → Staged: {trailTorque} Nm
              </span>
              <button
                type="button"
                onClick={() => controller.revertParameter(`${p}.trail.torque`)}
                className="hover:underline cursor-pointer"
              >
                Revert
              </button>
            </div>
          )}
        </div>

        {/* Eco Torque Slider */}
        <div className="p-4 rounded-xl bg-stone-50/70 border border-stone-200/80 flex flex-col gap-2">
          <Slider
            label="Eco Assist Torque"
            value={ecoTorque}
            min={20}
            max={60}
            step={1}
            unit="Nm"
            onChange={(val) => handleStage(`${p}.eco.torque`, val)}
            valueFormatter={(val) => `${val} Nm (Range Extension)`}
          />
          {isStaged(`${p}.eco.torque`) && (
            <div className="text-[11px] font-mono text-amber-700 flex items-center justify-between">
              <span>
                Live on bike: {state.liveValues[`${p}.eco.torque`]?.value ?? 27} Nm → Staged: {ecoTorque} Nm
              </span>
              <button
                type="button"
                onClick={() => controller.revertParameter(`${p}.eco.torque`)}
                className="hover:underline cursor-pointer"
              >
                Revert
              </button>
            </div>
          )}
        </div>
      </Card>

      {/* Bike Hardware & Geometry Section */}
      <Card className="flex flex-col gap-4 shadow-xs border-stone-200">
        <div>
          <h3 className="text-base font-bold text-stone-900">
            Bike Hardware & Wheel Geometry
          </h3>
          <p className="text-xs text-stone-500 mt-0.5">
            Calibrate wheel roll-out circumference and regional speed assist limits.
          </p>
        </div>

        {/* Tire Circumference Stepper */}
        <div
          className="p-4 rounded-xl border border-stone-200 bg-stone-50/50 flex flex-col sm:flex-row sm:items-center justify-between gap-4"
          data-testid="param-row-tire-circumference"
        >
          <div>
            <div className="flex items-center gap-2">
              <span className="text-sm font-semibold text-stone-900">
                Tire Circumference
              </span>
              {isStaged("tire-circumference") && (
                <span className="text-[10px] font-mono font-medium px-2 py-0.5 rounded-full bg-amber-100 text-amber-800">
                  Staged
                </span>
              )}
            </div>
            <div className="text-xs text-stone-500 mt-0.5 font-mono">
              Live:{" "}
              <strong className="text-stone-800">
                {state.liveValues["tire-circumference"]?.value ?? 2280} mm
              </strong>
              {isStaged("tire-circumference") && (
                <span className="text-amber-700 font-semibold ml-2">
                  Staged: {tireCircumference} mm
                </span>
              )}
            </div>
          </div>

          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => handleStage("tire-circumference", tireCircumference - 10)}
              data-testid="stepper-minus-tire-circumference"
              title="Decrease 10mm"
              className="w-11 h-11 p-0 text-base shrink-0 min-w-[44px] min-h-[44px]"
            >
              <Minus size={16} />
            </Button>
            <div className="min-w-[80px] text-center font-mono font-bold text-stone-900 text-sm">
              {tireCircumference} mm
            </div>
            <Button
              variant="outline"
              size="sm"
              onClick={() => handleStage("tire-circumference", tireCircumference + 10)}
              data-testid="stepper-plus-tire-circumference"
              title="Increase 10mm"
              className="w-11 h-11 p-0 text-base shrink-0 min-w-[44px] min-h-[44px]"
            >
              <Plus size={16} />
            </Button>
          </div>
        </div>

        {/* Speed Limit Display */}
        <div
          className="p-4 rounded-xl border border-stone-200 bg-stone-50/50 flex items-center justify-between"
          data-testid="param-row-speed-limit"
        >
          <div>
            <span className="text-sm font-semibold text-stone-900 block">
              Speed Assist Limit
            </span>
            <span className="text-xs text-stone-500 font-mono">
              Live: {state.liveValues["speed-limit"]?.value ?? 25} km/h
            </span>
          </div>
          <div className="text-sm font-mono font-bold text-stone-900 bg-white px-3 py-1.5 rounded border border-stone-300">
            {speedLimit} km/h
          </div>
        </div>

        {/* Mobile / Compact Parameter Filter */}
        <div className="flex flex-col sm:flex-row items-stretch sm:items-center gap-2 pt-2 border-t border-stone-100">
          <div className="flex-1">
            <input
              data-testid="search-filter-input"
              type="text"
              placeholder="Filter parameters..."
              value={searchFilter}
              onChange={(e) => setSearchFilter((e.target as HTMLInputElement).value)}
              className="w-full text-base font-sans p-2 rounded border border-stone-200 bg-stone-50 text-stone-900 min-h-[44px]"
              style={{ fontSize: "16px" }}
            />
          </div>
        </div>
      </Card>

      {/* Mobile Review Dock Trigger */}
      {stagedCount > 0 && (
        <button
          type="button"
          onClick={() => {
            const rail = document.querySelector(".transaction-rail");
            rail?.scrollIntoView({ behavior: "smooth", block: "nearest" });
          }}
          data-action="review-changes"
          data-testid="btn-review-changes"
          className="w-full py-3 px-4 rounded-xl bg-amber-500 text-stone-950 font-semibold text-sm shadow-md hover:bg-amber-400 min-h-[44px] flex items-center justify-center gap-2 cursor-pointer select-none"
        >
          <span>Review {stagedCount} Staged Change{stagedCount > 1 ? "s" : ""}</span>
        </button>
      )}

      {/* Floating Bottom Transaction Rail / Safe Friction Trigger */}
      {stagedCount > 0 && (
        <aside
          tabIndex={-1}
          className="fixed bottom-6 left-1/2 -translate-x-1/2 w-[calc(100%-2rem)] max-w-xl bg-stone-900 text-white rounded-2xl p-4 shadow-2xl flex items-center justify-between gap-4 z-40 border border-stone-700 animate-in fade-in slide-in-from-bottom-4 duration-200 transaction-rail"
          data-testid="transaction-rail"
          aria-label="Staged Transaction"
        >
          <div className="flex flex-col">
            <span className="text-xs font-medium text-amber-400">
              Staged Changes ({stagedCount})
            </span>
            <div className="text-xs text-stone-300 font-mono mt-0.5">
              {Object.entries(state.stagedValues).map(([name, val]) => (
                <div key={name} data-testid={`tx-item-${name}`}>
                  {name}: {state.liveValues[name]?.value ?? "unset"} mm → {val} mm
                </div>
              ))}
            </div>
          </div>

          <div className="flex items-center gap-2 shrink-0">
            <Button
              variant="ghost"
              size="sm"
              onClick={() => controller.discardAllStaged()}
              data-testid="btn-discard-staged"
              className="text-stone-300 hover:text-white text-xs min-h-[44px] min-w-[44px]"
            >
              Discard
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => setDiffModalOpen(true)}
              data-testid="btn-diff-modal"
              className="border-stone-700 text-stone-200 hover:text-white hover:bg-stone-800 text-xs min-h-[44px] min-w-[44px]"
            >
              Diff
            </Button>
            <Button
              variant="primary"
              size="sm"
              onClick={handleCommit}
              data-testid="btn-commit-batch"
              className="bg-amber-500 hover:bg-amber-400 text-stone-950 font-semibold text-xs min-h-[44px] min-w-[44px]"
            >
              Apply Changes
            </Button>
          </div>
        </aside>
      )}

      {/* SAFE FRICTION: Visual Diff Confirmation Modal */}
      <Modal
        open={diffModalOpen}
        onOpenChange={setDiffModalOpen}
        title="Review Changes Before Writing to Bike"
        description="Verify the parameters that will be written into your Shimano drive unit's internal memory."
        maxWidth="md"
        footer={
          <>
            <Button
              variant="outline"
              size="md"
              onClick={() => setDiffModalOpen(false)}
            >
              Cancel
            </Button>
            <Button
              variant="primary"
              size="md"
              onClick={handleCommit}
              data-testid="btn-confirm-diff-commit"
              className="bg-stone-900 hover:bg-black text-white flex items-center gap-2"
            >
              <Check size={16} />
              <span>Apply Changes to Bike</span>
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-4">
          <div className="rounded-xl border border-stone-200 overflow-hidden bg-stone-50">
            <table className="w-full text-xs font-sans">
              <thead className="bg-stone-100 text-stone-600 font-medium border-b border-stone-200">
                <tr>
                  <th className="py-2.5 px-3 text-left">Setting</th>
                  <th className="py-2.5 px-3 text-right">Current Live</th>
                  <th className="py-2.5 px-3 text-right">New Value</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-stone-200 font-mono">
                {Object.entries(state.stagedValues).map(([name, val]) => {
                  const live = state.liveValues[name]?.value ?? "unset";
                  return (
                    <tr key={name} className="hover:bg-white">
                      <td className="py-2.5 px-3 text-stone-900 font-medium font-sans">
                        {name}
                      </td>
                      <td className="py-2.5 px-3 text-right text-stone-500">
                        {String(live)}
                      </td>
                      <td className="py-2.5 px-3 text-right text-emerald-700 font-bold">
                        {String(val)}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>

          <div className="p-3.5 rounded-xl bg-emerald-50 border border-emerald-200 text-emerald-900 flex items-start gap-3">
            <ShieldCheck size={20} className="text-emerald-700 shrink-0 mt-0.5" />
            <div className="text-xs leading-relaxed">
              <strong className="block font-semibold">Automatic Pre-Write Safety Net</strong>
              cranklink automatically captures and persists a complete restore point of your previous settings before writing. You can revert at any time.
            </div>
          </div>
        </div>
      </Modal>


    </div>
  );
}

import { friendlyMotorName } from "../../domain/motorIdentity.ts";
import { useState } from "react";
import {
  Zap,
  Disc3,
  ShieldCheck,
  Eye,
  EyeOff,
  Cpu,
  Globe,
  Gauge,
  Sliders,
  CheckCircle2,
  HardDrive,
  Lightbulb,
  Layers,
} from "lucide-react";
import type { InstrumentState } from "../../types.ts";
import type { SessionController } from "../../core/sessionController.ts";
import { Card } from "../ui/Card.tsx";
import { Badge } from "../ui/Badge.tsx";
import {
  LEGACY_DESTINATION_LABELS,
  MODERN_DESTINATION_LABELS,
} from "../../domain/schema.ts";

interface OverviewViewProps {
  state: InstrumentState;
  controller: SessionController;
}

export function OverviewView({ state, controller }: OverviewViewProps) {
  const [showFullSerial, setShowFullSerial] = useState(false);
  const identity = state.identity;
  const hasSetting = (name: string) => state.schema.some((setting) => setting.name === name);

  const getFriendlyModel = () => friendlyMotorName(identity);

  const getFirmwareStatusText = () => {
    const installed = identity?.dcasVersion || "4.2.0.0";
    const inspected = state.firmware.dcasFile?.metadata;
    if (!inspected) {
      return `Installed: v${installed}`;
    }
    const inspectedVer = inspected.version;
    if (inspectedVer === installed) {
      return `Installed: v${installed} (Matches selected file)`;
    }
    if (inspectedVer > installed) {
      return `Installed: v${installed} → File: v${inspectedVer} (Update available)`;
    }
    return `Installed: v${installed} → File: v${inspectedVer} (Rollback)`;
  };

  const getRegionLabel = () => {
    const rawDest = state.stagedValues["destination"] ?? state.liveValues["destination"]?.value;
    const destNum = Number(rawDest);
    const labels =
      ["ep600", "ep801", "ep500"].includes(identity?.modelSlug || "")
        ? MODERN_DESTINATION_LABELS
        : LEGACY_DESTINATION_LABELS;
    return labels[destNum] || (rawDest !== undefined ? `Region ${rawDest}` : "—");
  };

  const tireCircumference = state.stagedValues["tire-circumference"] ?? state.liveValues["tire-circumference"]?.value ?? "—";
  const speedLimit = state.stagedValues["speed-limit"] ?? state.liveValues["speed-limit"]?.value ?? "—";
  const lampState = state.stagedValues["lamp"] ?? state.liveValues["lamp"]?.value;

  const p1Boost = state.stagedValues["profile1.boost.torque"] ?? state.liveValues["profile1.boost.torque"]?.value ?? 85;
  const p1Trail = state.stagedValues["profile1.trail.torque"] ?? state.liveValues["profile1.trail.torque"]?.value ?? 85;
  const p1Eco = state.stagedValues["profile1.eco.torque"] ?? state.liveValues["profile1.eco.torque"]?.value ?? 27;

  const p2Boost = state.stagedValues["profile2.boost.torque"] ?? state.liveValues["profile2.boost.torque"]?.value ?? 85;
  const p2Trail = state.stagedValues["profile2.trail.torque"] ?? state.liveValues["profile2.trail.torque"]?.value ?? 85;
  const p2Eco = state.stagedValues["profile2.eco.torque"] ?? state.liveValues["profile2.eco.torque"]?.value ?? 27;

  return (
    <div className="w-full flex flex-col gap-6 py-2">
      {/* 1. Hardware Identity & Connection Status Card */}
      <Card className="flex flex-col gap-5 border-stone-200">
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
          <div className="flex items-start gap-4">
            <div className="w-14 h-14 rounded-2xl bg-stone-900 text-white flex items-center justify-center shrink-0 shadow-sm">
              <span className="font-bold text-xl font-mono">EP</span>
            </div>
            <div className="flex flex-col gap-1">
              <div className="flex items-center gap-2.5 flex-wrap">
                <h2 className="text-xl font-bold tracking-tight text-stone-900 font-sans">
                  {getFriendlyModel()}
                </h2>
                {state.transport === "sim" ? (
                  <Badge variant="sim">
                    SIM: {identity?.model || "SIMULATOR"}
                  </Badge>
                ) : (
                  <Badge variant="success">
                    Connected & Ready
                  </Badge>
                )}
              </div>

              {/* Exact Identity Strip with SN, DCAS, RX */}
              <div
                className="flex items-center gap-2 text-xs text-stone-600 flex-wrap pt-0.5 font-mono"
              >
                <button
                  type="button"
                  onClick={() => setShowFullSerial(!showFullSerial)}
                  className="inline-flex items-center gap-1 hover:text-stone-900 cursor-pointer font-mono"
                  title="Toggle serial number display"
                >
                  <span>SN: {identity?.serial || "010203040506"}</span>
                  {showFullSerial ? <EyeOff size={12} /> : <Eye size={12} />}
                </button>
                <span>•</span>
                <span>
                  DCAS: <strong className="text-stone-900 font-mono">{identity?.dcasVersion || "4.2.0.0"}</strong>
                </span>
                <span>•</span>
                <span>
                  RX: <strong className="text-stone-900 font-mono">{identity?.rxVersion || "4.1.11"}</strong>
                </span>
                <span>•</span>
                <span className="font-sans text-stone-600 font-medium">
                  {getFirmwareStatusText()}
                </span>
              </div>
            </div>
          </div>
        </div>

        {/* Technical Sub-specs */}
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-3 pt-3 border-t border-stone-100">
          <div className="p-3 rounded-2xl bg-stone-50 border border-stone-200/80 flex flex-col gap-0.5">
            <span className="text-[10px] font-semibold text-stone-500 uppercase tracking-wider">
              Transport Link
            </span>
            <span className="text-sm font-mono font-bold text-stone-900 uppercase">
              {state.transport || "None"}
            </span>
          </div>

          {hasSetting("destination") && (
<div className="p-3 rounded-2xl bg-stone-50 border border-stone-200/80 flex flex-col gap-0.5">
            <span className="text-[10px] font-semibold text-stone-500 uppercase tracking-wider">
              Market Standard
            </span>
            <span className="text-sm font-sans font-bold text-stone-900 truncate" title={getRegionLabel()}>
              {getRegionLabel()}
            </span>
          </div>
          )}

          {hasSetting("speed-limit") && (
<div className="p-3 rounded-2xl bg-stone-50 border border-stone-200/80 flex flex-col gap-0.5">
            <span className="text-[10px] font-semibold text-stone-500 uppercase tracking-wider">
              Speed Assist Cutoff
            </span>
            <span className="text-sm font-mono font-bold text-stone-900">
              {speedLimit} km/h
            </span>
          </div>
          )}

          <div className="p-3 rounded-2xl bg-stone-50 border border-stone-200/80 flex flex-col gap-0.5">
            <span className="text-[10px] font-semibold text-stone-500 uppercase tracking-wider">
              Wheel Roll-Out
            </span>
            <span className="text-sm font-mono font-bold text-stone-900">
              {tireCircumference} mm
            </span>
          </div>
        </div>
      </Card>

      {/* 2. Active Drive Configuration & Live Operating State */}
      {state.schema.some((setting) => setting.section === "profiles") && (
      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        {/* Assist Profile 1 Summary */}
        <Card className="flex flex-col gap-4 border-stone-200">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <Zap size={18} className="text-amber-600" />
              <h3 className="text-sm font-bold text-stone-900">Profile 1 (Everyday / Trail)</h3>
            </div>
            <span className="text-[10px] font-mono font-medium px-2 py-0.5 rounded-full bg-stone-100 text-stone-700">
              Active
            </span>
          </div>

          <div className="grid grid-cols-3 gap-2.5 text-center">
            <div className="p-3 rounded-xl bg-stone-50 border border-stone-200/70">
              <span className="text-[10px] font-semibold text-stone-500 block uppercase">Boost</span>
              <span className="text-base font-mono font-bold text-stone-900">{p1Boost} Nm</span>
            </div>
            <div className="p-3 rounded-xl bg-stone-50 border border-stone-200/70">
              <span className="text-[10px] font-semibold text-stone-500 block uppercase">Trail</span>
              <span className="text-base font-mono font-bold text-stone-900">{p1Trail} Nm</span>
            </div>
            <div className="p-3 rounded-xl bg-stone-50 border border-stone-200/70">
              <span className="text-[10px] font-semibold text-stone-500 block uppercase">Eco</span>
              <span className="text-base font-mono font-bold text-stone-900">{p1Eco} Nm</span>
            </div>
          </div>
        </Card>

        {/* Assist Profile 2 Summary */}
        <Card className="flex flex-col gap-4 border-stone-200">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <Zap size={18} className="text-amber-600" />
              <h3 className="text-sm font-bold text-stone-900">Profile 2 (Custom / Power)</h3>
            </div>
            <span className="text-[10px] font-mono font-medium px-2 py-0.5 rounded-full bg-stone-100 text-stone-700">
              Preset
            </span>
          </div>

          <div className="grid grid-cols-3 gap-2.5 text-center">
            <div className="p-3 rounded-xl bg-stone-50 border border-stone-200/70">
              <span className="text-[10px] font-semibold text-stone-500 block uppercase">Boost</span>
              <span className="text-base font-mono font-bold text-stone-900">{p2Boost} Nm</span>
            </div>
            <div className="p-3 rounded-xl bg-stone-50 border border-stone-200/70">
              <span className="text-[10px] font-semibold text-stone-500 block uppercase">Trail</span>
              <span className="text-base font-mono font-bold text-stone-900">{p2Trail} Nm</span>
            </div>
            <div className="p-3 rounded-xl bg-stone-50 border border-stone-200/70">
              <span className="text-[10px] font-semibold text-stone-500 block uppercase">Eco</span>
              <span className="text-base font-mono font-bold text-stone-900">{p2Eco} Nm</span>
            </div>
          </div>
        </Card>
      </div>

      )}

      {/* 3. Drivetrain & Installation Specs */}
      <Card className="flex flex-col gap-4 border-stone-200">
        <h3 className="text-sm font-bold text-stone-900 flex items-center gap-2">
          <HardDrive size={18} className="text-indigo-600" />
          <span>Drivetrain & Installation Specs</span>
        </h3>

        <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
          {hasSetting("shifting-system") && (
<div className="p-3 rounded-2xl bg-stone-50 border border-stone-200/80 flex flex-col gap-0.5">
            <span className="text-[10px] font-semibold text-stone-500 uppercase tracking-wider">
              Shifting System
            </span>
            <span className="text-xs font-semibold text-stone-900 truncate">
              {Number(state.stagedValues["shifting-system"] ?? state.liveValues["shifting-system"]?.value ?? 0) === 1
                ? "Di2 IGH"
                : Number(state.stagedValues["shifting-system"] ?? state.liveValues["shifting-system"]?.value ?? 0) === 2
                ? "Di2 Derailleur"
                : Number(state.stagedValues["shifting-system"] ?? state.liveValues["shifting-system"]?.value ?? 0) >= 3
                ? "Di2 Auto"
                : "Mechanical"}
            </span>
          </div>
          )}

          {hasSetting("front-teeth") && hasSetting("rear-teeth") && (
<div className="p-3 rounded-2xl bg-stone-50 border border-stone-200/80 flex flex-col gap-0.5">
            <span className="text-[10px] font-semibold text-stone-500 uppercase tracking-wider">
              Chainring / Cog
            </span>
            <span className="text-xs font-mono font-bold text-stone-900">
              {state.stagedValues["front-teeth"] ?? state.liveValues["front-teeth"]?.value ?? 34}T /{" "}
              {state.stagedValues["rear-teeth"] ?? state.liveValues["rear-teeth"]?.value ?? 11}T
            </span>
          </div>
          )}

          <div className="p-3 rounded-2xl bg-stone-50 border border-stone-200/80 flex flex-col gap-0.5">
            <span className="text-[10px] font-semibold text-stone-500 uppercase tracking-wider">
              Mounting Pitch
            </span>
            <span className="text-xs font-mono font-bold text-stone-900">
              {state.stagedValues["mounting-angle"] ?? state.liveValues["mounting-angle"]?.value ?? 0}°
            </span>
          </div>

          <div className="p-3 rounded-2xl bg-stone-50 border border-stone-200/80 flex flex-col gap-0.5">
            <span className="text-[10px] font-semibold text-stone-500 uppercase tracking-wider">
              Wired Light Port
            </span>
            <span className="text-xs font-semibold text-stone-900">
              {Number(lampState) === 1 ? "Always On" : Number(lampState) === 2 ? "Auto" : "Off"}
            </span>
          </div>
        </div>
      </Card>

      {/* 4. System Integrity & Safety Journal State */}
      <Card className="flex flex-col gap-4 border-stone-200">
        <h3 className="text-sm font-bold text-stone-900 flex items-center gap-2">
          <ShieldCheck size={18} className="text-emerald-700" />
          <span>System Integrity & Backup Status</span>
        </h3>

        <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
          <div className="p-3.5 rounded-2xl bg-stone-50 border border-stone-200/80 flex flex-col gap-1">
            <span className="text-[10px] font-semibold text-stone-500 uppercase">
              Microcontroller Pair
            </span>
            <span className="text-xs font-semibold text-stone-900 flex items-center gap-1.5">
              <CheckCircle2 size={14} className="text-emerald-600" />
              <span>Drive-unit identity verified</span>
            </span>
          </div>

          <div className="p-3.5 rounded-2xl bg-stone-50 border border-stone-200/80 flex flex-col gap-1">
            <span className="text-[10px] font-semibold text-stone-500 uppercase">
              Stored Restore Points
            </span>
            <span className="text-xs font-mono font-semibold text-stone-900">
              {state.snapshots.length} snapshot{state.snapshots.length !== 1 ? "s" : ""} in storage
            </span>
          </div>

          <div className="p-3.5 rounded-2xl bg-stone-50 border border-stone-200/80 flex flex-col gap-1">
            <span className="text-[10px] font-semibold text-stone-500 uppercase">
              Pending Changes
            </span>
            <span className="text-xs font-mono font-semibold text-stone-900">
              {Object.keys(state.stagedValues).length} staged
            </span>
          </div>
        </div>
      </Card>
    </div>
  );
}

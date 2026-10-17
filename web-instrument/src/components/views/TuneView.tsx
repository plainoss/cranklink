import { useState } from "react";
import {
  Zap,
  Sliders,
  Check,
  RotateCcw,
  ShieldCheck,
  AlertTriangle,
  Plus,
  Minus,
  Compass,
  Lightbulb,
  Cog,
  Wrench,
  Layers,
  ChevronDown,
  X,
} from "lucide-react";
import type { InstrumentState } from "../../types.ts";
import type { SessionController } from "../../core/sessionController.ts";
import { Card } from "../ui/Card.tsx";
import { Button } from "../ui/Button.tsx";
import { Slider } from "../ui/Slider.tsx";
import { Modal } from "../ui/Dialog.tsx";
import { Tabs } from "../ui/Tabs.tsx";
import { Badge } from "../ui/Badge.tsx";
import {
  LEGACY_DESTINATION_LABELS,
  MODERN_DESTINATION_LABELS,
  TIRE_PRESETS,
} from "../../domain/schema.ts";

export type TuneCategory =
  | "all"
  | "geometry"
  | "profiles"
  | "drivetrain"
  | "maintenance"
  | "oem";

interface TuneViewProps {
  state: InstrumentState;
  controller: SessionController;
  category?: TuneCategory;
  searchFilterProp?: string;
  mobileReviewOpen: boolean;
  onMobileReviewOpenChange: (open: boolean) => void;
}

export function TuneView({
  state,
  controller,
  category,
  searchFilterProp = "",
  mobileReviewOpen,
  onMobileReviewOpenChange,
}: TuneViewProps) {
  const [profileTab, setProfileTab] = useState<"1" | "2">("1");
  const [diffModalOpen, setDiffModalOpen] = useState(false);

  const isDesktop = typeof window !== "undefined" ? window.innerWidth >= 1024 : true;

  const activeGroup = (category || state.activeParameterGroup || "geometry") as TuneCategory;
  const p = profileTab === "1" ? "profile1" : "profile2";

  const getVal = (name: string, fallback: number | string = 0): number => {
    if (state.stagedValues[name] !== undefined) {
      return Number(state.stagedValues[name]);
    }
    const live = state.liveValues[name]?.value;
    if (live !== undefined && live !== null && !isNaN(Number(live))) {
      return Number(live);
    }
    return Number(fallback);
  };

  const getStrVal = (name: string, fallback: string = ""): string => {
    if (state.stagedValues[name] !== undefined) {
      return String(state.stagedValues[name]);
    }
    const live = state.liveValues[name]?.value;
    if (live !== undefined && live !== null) {
      return String(live);
    }
    return fallback;
  };

  const isStaged = (name: string) => state.stagedValues[name] !== undefined;

  const handleStage = (name: string, val: number | string) => {
    controller.stageParameter(name, val);
  };

  const handleCommit = async () => {
    setDiffModalOpen(false);
    onMobileReviewOpenChange(false);
    try {
      await controller.commitStagedBatch();
    } catch {
      // Error captured in state.errorBanner
    }
  };

  // Profile parameter values
  const boostTorque = getVal(`${p}.boost.torque`, 85);
  const boostCharacter = getVal(`${p}.boost.character`, 5);
  const boostStart = getVal(`${p}.boost.start`, 3);

  const trailTorque = getVal(`${p}.trail.torque`, 85);
  const trailCharacter = getVal(`${p}.trail.character`, 5);
  const trailStart = getVal(`${p}.trail.start`, 3);

  const ecoTorque = getVal(`${p}.eco.torque`, 27);
  const ecoCharacter = getVal(`${p}.eco.character`, 5);
  const ecoStart = getVal(`${p}.eco.start`, 3);

  const profileDisplay = getVal(`${p}.display`, 1);
  const assistPattern = getVal("assist-pattern", 5);

  // Geometry parameter values
  const destinationVal = getVal("destination", 3);
  const speedLimit = getVal("speed-limit", 25);
  const tireCircumference = getVal("tire-circumference", 2280);
  const lampVal = getVal("lamp", 0);
  const mountingAngle = getVal("mounting-angle", 0);
  const speedAdjustment = getVal("display-speed-adjustment", 0);
  const lightingTime = getVal("lighting-time", 0);
  const walkAssist = getVal("walk-assist", 0);

  // Drivetrain parameter values
  const shiftingSystem = getVal("shifting-system", 0);
  const frontTeeth = getVal("front-teeth", 34);
  const rearTeeth = getVal("rear-teeth", 11);
  const rearMaxGear = getVal("rear-max-gear", 12);
  const autoGearMode = getVal("auto-gear-mode", 0);
  const autoGearCadence = getVal("auto-gear-cadence", 65);
  const startMode = getVal("start-mode", 0);
  const shiftingAdvice = getVal("shifting-advice", 1);
  const shiftLimitUp = getVal("shift-limit-up", 255);
  const shiftLimitDown = getVal("shift-limit-down", 255);

  // Service & Maintenance
  const maintenanceAlert = getVal("maintenance-alert", 0);
  const maintenanceDistance = getVal("maintenance-distance", 1000);
  const maintenanceDate = getStrVal("maintenance-date", "");

  // OEM limits
  const torqueMin = getVal("torque-minimum", 20);
  const torqueMax = getVal("torque-maximum", 85);
  const maxPowerBoost = getVal("max-power.boost", 500);
  const maxPowerTrail = getVal("max-power.trail", 500);
  const maxPowerEco = getVal("max-power.eco", 500);

  const destinationLabels =
    state.identity?.modelSlug === "ep800-crg"
      ? LEGACY_DESTINATION_LABELS
      : MODERN_DESTINATION_LABELS;

  const maxSpeedCeiling =
    destinationVal === 5 ? 45 : destinationVal === 1 ? 32 : 25;

  const stagedCount = Object.keys(state.stagedValues).length;

  const filterMatches = (text: string) => {
    if (!searchFilterProp.trim()) return true;
    return text.toLowerCase().includes(searchFilterProp.toLowerCase().trim());
  };

  const isFiltering = searchFilterProp.trim().length > 0;

  const showProfiles =
    (activeGroup === "all" || activeGroup === "profiles" || isFiltering) &&
    (filterMatches("boost") ||
      filterMatches("trail") ||
      filterMatches("eco") ||
      filterMatches("torque") ||
      filterMatches("character") ||
      filterMatches("start") ||
      filterMatches("assist") ||
      filterMatches("profile") ||
      filterMatches("pattern"));

  const showGeometry =
    (activeGroup === "all" || activeGroup === "geometry" || isFiltering) &&
    (filterMatches("tire") ||
      filterMatches("circumference") ||
      filterMatches("speed") ||
      filterMatches("region") ||
      filterMatches("destination") ||
      filterMatches("wheel") ||
      filterMatches("light") ||
      filterMatches("lamp") ||
      filterMatches("angle") ||
      filterMatches("walk") ||
      filterMatches("geometry"));

  const showDrivetrain =
    (activeGroup === "all" || activeGroup === "drivetrain" || isFiltering) &&
    (state.schema.length === 0 || state.schema.some((s) => s.section === "drivetrain")) &&
    (filterMatches("gear") ||
      filterMatches("shift") ||
      filterMatches("di2") ||
      filterMatches("cadence") ||
      filterMatches("teeth") ||
      filterMatches("chainring") ||
      filterMatches("cassette") ||
      filterMatches("drivetrain"));

  const showMaintenance =
    (activeGroup === "all" || activeGroup === "maintenance" || isFiltering) &&
    (state.schema.length === 0 || state.schema.some((s) => s.section === "maintenance")) &&
    (filterMatches("maintenance") ||
      filterMatches("service") ||
      filterMatches("alert") ||
      filterMatches("distance") ||
      filterMatches("date"));

  const showOem =
    (activeGroup === "all" || activeGroup === "oem" || isFiltering) &&
    (state.schema.length === 0 || state.schema.some((s) => s.section === "oem")) &&
    (filterMatches("oem") ||
      filterMatches("power") ||
      filterMatches("limit") ||
      filterMatches("bound"));

  return (
    <div className="w-full flex flex-col gap-6 py-2">
      {/* 1. Bike Hardware, Region & Geometry Section */}
      {showGeometry && (
        <Card className="p-0 flex flex-col divide-y divide-stone-200 shadow-xs border-stone-200">
          {/* Region / Destination Selector */}
          <div className="px-4 py-4 sm:px-5 flex flex-col gap-3">
            <div className="flex items-center justify-between">
              <div>
                <span className="text-sm font-semibold text-stone-900 block">
                  Market Jurisdiction (Destination)
                </span>
                <span className="text-xs text-stone-500 font-mono">
                  Live on bike: {destinationLabels[Number(state.liveValues["destination"]?.value ?? 3)] || "Europe (25 km/h)"}
                </span>
              </div>
              {isStaged("destination") && (
                <span className="text-[10px] font-mono font-medium px-2 py-0.5 rounded-full bg-amber-100 text-amber-800">
                  Staged: {destinationLabels[destinationVal]}
                </span>
              )}
            </div>

            <select
              value={destinationVal}
              onChange={(e) => handleStage("destination", Number(e.target.value))}
              className="w-full text-xs font-mono p-2.5 rounded-xl border border-stone-300 bg-white text-stone-900 min-h-[44px] cursor-pointer focus:outline-none focus:border-stone-500"
            >
              {Object.entries(destinationLabels).map(([code, label]) => (
                <option key={code} value={code}>
                  {label}
                </option>
              ))}
            </select>
          </div>

          {/* Speed Assist Limit Slider */}
          <div
            className="px-4 py-4 sm:px-5 flex flex-col gap-3"
            data-testid="param-row-speed-limit"
          >
            <div className="flex items-center justify-between">
              <div>
                <span className="text-sm font-semibold text-stone-900 block">
                  Speed Assist Limit
                </span>
                <span className="text-xs text-stone-500 font-mono">
                  Live: {state.liveValues["speed-limit"]?.value ?? 25} km/h
                  {isStaged("speed-limit") && (
                    <span className="text-amber-700 font-semibold ml-2">
                      Staged: {speedLimit} km/h
                    </span>
                  )}
                </span>
              </div>
              <div className="text-sm font-mono font-bold text-stone-900 bg-white px-3 py-1.5 rounded-lg border border-stone-200 shadow-2xs">
                {speedLimit} km/h
              </div>
            </div>

            <Slider
              label="Assist Cutoff Velocity"
              value={speedLimit}
              min={15}
              max={maxSpeedCeiling}
              step={1}
              unit="km/h"
              onChange={(val) => handleStage("speed-limit", val)}
              valueFormatter={(val) => `${val} km/h`}
            />

            {/* Quick Speed Presets */}
            <div className="flex items-center gap-2 flex-wrap pt-1">
              <span className="text-[11px] font-medium text-stone-500">Presets:</span>
              {[20, 25, 32].map((spd) => (
                <button
                  type="button"
                  key={spd}
                  onClick={() => handleStage("speed-limit", spd)}
                  className={`px-2.5 py-1 text-xs font-mono rounded-lg border transition-colors cursor-pointer ${
                    speedLimit === spd
                      ? "bg-stone-900 text-white border-stone-900"
                      : "bg-white text-stone-700 border-stone-200 hover:bg-stone-100"
                  }`}
                >
                  {spd} km/h
                </button>
              ))}
            </div>
          </div>

          {/* Tire Circumference Stepper & Presets */}
          <div
            className="px-4 py-4 sm:px-5 flex flex-col gap-3"
            data-testid="param-row-tire-circumference"
          >
            <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
              <div>
                <div className="flex items-center gap-2">
                  <span className="text-sm font-semibold text-stone-900">
                    Tire Roll-Out Circumference
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

            {/* Tire Presets */}
            <div className="flex items-center gap-1.5 flex-wrap pt-1 border-t border-stone-200/60">
              <span className="text-[11px] font-medium text-stone-500 mr-1">Common Sizes:</span>
              {TIRE_PRESETS.map((preset) => (
                <button
                  type="button"
                  key={preset.label}
                  onClick={() => handleStage("tire-circumference", Number(preset.value))}
                  className={`px-2 py-1 text-[11px] font-mono rounded-lg border transition-colors cursor-pointer ${
                    tireCircumference === Number(preset.value)
                      ? "bg-stone-900 text-white border-stone-900"
                      : "bg-white text-stone-700 border-stone-200 hover:bg-stone-100"
                  }`}
                >
                  {preset.label}
                </button>
              ))}
            </div>
          </div>

          {/* Wired Light Terminal (Lamp) */}
          <div className="px-4 py-4 sm:px-5 flex items-center justify-between gap-3">
            <div className="flex items-center gap-2.5">
              <Lightbulb size={18} className="text-amber-600" />
              <div>
                <span className="text-sm font-semibold text-stone-900 block">
                  12V Wired Light Port
                </span>
                <span className="text-xs text-stone-500 font-mono">
                  Current: {lampVal === 1 ? "Always On" : lampVal === 2 ? "Auto" : "Off"}
                </span>
              </div>
            </div>

            <div className="flex items-center gap-1 bg-stone-200/70 p-1 rounded-xl">
              {[
                { val: 0, label: "Off" },
                { val: 1, label: "On" },
                { val: 2, label: "Auto" },
              ].map((opt) => (
                <button
                  type="button"
                  key={opt.val}
                  onClick={() => handleStage("lamp", opt.val)}
                  className={`px-3 py-1.5 rounded-lg text-xs font-semibold transition-all cursor-pointer ${
                    lampVal === opt.val
                      ? "bg-white text-stone-900 shadow-2xs font-bold"
                      : "text-stone-600 hover:text-stone-900"
                  }`}
                >
                  {opt.label}
                </button>
              ))}
            </div>
          </div>

          {/* Advanced Geometry & Installation Details */}
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-x-5 gap-y-4 px-4 py-4 sm:px-5">
            {/* Motor Mounting Angle */}
            <div className="flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <div>
                  <span className="text-xs font-semibold text-stone-900 block">
                    Motor Mounting Angle
                  </span>
                  <span className="text-[11px] text-stone-500 font-mono">
                    Chassis frame pitch
                  </span>
                </div>
                <div className="text-xs font-mono font-bold text-stone-900 bg-white px-2 py-1 rounded-md border border-stone-200">
                  {mountingAngle}°
                </div>
              </div>
              <Slider
                label="Mounting Pitch"
                value={mountingAngle}
                min={0}
                max={60}
                step={1}
                unit="°"
                onChange={(val) => handleStage("mounting-angle", val)}
                valueFormatter={(val) => `${val}°`}
              />
            </div>

            {/* Speedometer Display Offset */}
            <div className="flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <div>
                  <span className="text-xs font-semibold text-stone-900 block">
                    Speedometer Offset
                  </span>
                  <span className="text-[11px] text-stone-500 font-mono">
                    Fine calibration
                  </span>
                </div>
                <div className="text-xs font-mono font-bold text-stone-900 bg-white px-2 py-1 rounded-md border border-stone-200">
                  {(speedAdjustment * 0.1).toFixed(1)}%
                </div>
              </div>
              <Slider
                label="Display Calibration"
                value={speedAdjustment}
                min={-50}
                max={50}
                step={5}
                unit="0.1%"
                onChange={(val) => handleStage("display-speed-adjustment", val)}
                valueFormatter={(val) => `${(val * 0.1).toFixed(1)}%`}
              />
            </div>

            {/* Lighting Reserve Time */}
            <div className="flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <div>
                  <span className="text-xs font-semibold text-stone-900 block">
                    Lighting Reserve Time
                  </span>
                  <span className="text-[11px] text-stone-500 font-mono">
                    Minutes after assist cutoff
                  </span>
                </div>
                <div className="text-xs font-mono font-bold text-stone-900 bg-white px-2 py-1 rounded-md border border-stone-200">
                  {lightingTime} min
                </div>
              </div>
              <Slider
                label="Reserve Duration"
                value={lightingTime}
                min={0}
                max={180}
                step={10}
                unit="min"
                onChange={(val) => handleStage("lighting-time", val)}
                valueFormatter={(val) => `${val} min`}
              />
            </div>

            {/* Walk Assist Toggle */}
            <div className="flex items-center justify-between gap-3">
              <div>
                <span className="text-xs font-semibold text-stone-900 block">
                  Walk Assist Mode
                </span>
                <span className="text-[11px] text-stone-500 font-mono">
                  Motor push when walking
                </span>
              </div>
              <div className="flex items-center gap-1 bg-stone-200/70 p-1 rounded-xl">
                {[
                  { val: 0, label: "Disabled" },
                  { val: 2, label: "Enabled" },
                ].map((opt) => (
                  <button
                    type="button"
                    key={opt.val}
                    onClick={() => handleStage("walk-assist", opt.val)}
                    className={`px-3 py-1.5 rounded-lg text-xs font-semibold transition-all cursor-pointer ${
                      walkAssist === opt.val
                        ? "bg-white text-stone-900 shadow-2xs font-bold"
                        : "text-stone-600 hover:text-stone-900"
                    }`}
                  >
                    {opt.label}
                  </button>
                ))}
              </div>
            </div>
          </div>
        </Card>
      )}

      {/* 2. Assist Tuning Sliders Card */}
      {showProfiles && (
        <Card className="flex flex-col gap-6 shadow-xs border-stone-200">
          <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
            <div>
              <h3 className="text-base font-bold text-stone-900">
                Assist Profiles & Dynamics (Profile {profileTab})
              </h3>
              <p className="text-xs text-stone-500 mt-0.5">
                Configure motor peak torque, pedal acceleration aggressiveness, and start responsiveness.
              </p>
            </div>

            {/* Profile Selector Tabs */}
            <div className="w-full sm:w-auto">
              <Tabs
                value={profileTab}
                onValueChange={(val) => {
                  setProfileTab(val as "1" | "2");
                  controller.setProfileTab(Number(val) as 1 | 2);
                }}
                items={[
                  { id: "1", label: "Profile 1" },
                  { id: "2", label: "Profile 2" },
                ]}
              />
            </div>
          </div>

          {/* Mode 1: Boost Mode Tuning */}
          <div className="p-4 rounded-2xl bg-stone-50/80 border border-stone-200 flex flex-col gap-4">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <span className="w-2.5 h-2.5 rounded-full bg-amber-500"></span>
                <span className="text-sm font-bold text-stone-900">Boost Mode</span>
              </div>
              <div className="text-xs font-mono font-bold text-stone-900 bg-white px-2.5 py-1 rounded-lg border border-stone-200 shadow-2xs">
                {boostTorque} Nm
              </div>
            </div>

            <Slider
              label="Peak Motor Torque"
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

            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 pt-1 border-t border-stone-200/60">
              <div>
                <Slider
                  label="Assist Character (Aggressiveness)"
                  value={boostCharacter}
                  min={1}
                  max={state.identity?.modelSlug === "ep800-crg" ? 9 : 14}
                  step={1}
                  unit="lvl"
                  onChange={(val) => handleStage(`${p}.boost.character`, val)}
                  valueFormatter={(val) => `Level ${val}`}
                />
              </div>
              <div>
                <Slider
                  label="Pedal Start Sensitivity"
                  value={boostStart}
                  min={0}
                  max={4}
                  step={1}
                  unit="lvl"
                  onChange={(val) => handleStage(`${p}.boost.start`, val)}
                  valueFormatter={(val) =>
                    val === 0 ? "Level 0 (Smooth)" : val === 4 ? "Level 4 (Instant)" : `Level ${val}`
                  }
                />
              </div>
            </div>
          </div>

          {/* Mode 2: Trail Mode Tuning */}
          <div className="p-4 rounded-2xl bg-stone-50/80 border border-stone-200 flex flex-col gap-4">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <span className="w-2.5 h-2.5 rounded-full bg-blue-500"></span>
                <span className="text-sm font-bold text-stone-900">Trail Mode</span>
              </div>
              <div className="text-xs font-mono font-bold text-stone-900 bg-white px-2.5 py-1 rounded-lg border border-stone-200 shadow-2xs">
                {trailTorque} Nm
              </div>
            </div>

            <Slider
              label="Peak Motor Torque"
              value={trailTorque}
              min={20}
              max={85}
              step={1}
              unit="Nm"
              onChange={(val) => handleStage(`${p}.trail.torque`, val)}
              valueFormatter={(val) => `${val} Nm (Dynamic Flow)`}
            />

            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 pt-1 border-t border-stone-200/60">
              <div>
                <Slider
                  label="Assist Character"
                  value={trailCharacter}
                  min={1}
                  max={state.identity?.modelSlug === "ep800-crg" ? 9 : 14}
                  step={1}
                  unit="lvl"
                  onChange={(val) => handleStage(`${p}.trail.character`, val)}
                  valueFormatter={(val) => `Level ${val}`}
                />
              </div>
              <div>
                <Slider
                  label="Pedal Start Sensitivity"
                  value={trailStart}
                  min={0}
                  max={4}
                  step={1}
                  unit="lvl"
                  onChange={(val) => handleStage(`${p}.trail.start`, val)}
                  valueFormatter={(val) =>
                    val === 0 ? "Level 0 (Smooth)" : val === 4 ? "Level 4 (Instant)" : `Level ${val}`
                  }
                />
              </div>
            </div>
          </div>

          {/* Mode 3: Eco Mode Tuning */}
          <div className="p-4 rounded-2xl bg-stone-50/80 border border-stone-200 flex flex-col gap-4">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <span className="w-2.5 h-2.5 rounded-full bg-emerald-500"></span>
                <span className="text-sm font-bold text-stone-900">Eco Mode</span>
              </div>
              <div className="text-xs font-mono font-bold text-stone-900 bg-white px-2.5 py-1 rounded-lg border border-stone-200 shadow-2xs">
                {ecoTorque} Nm
              </div>
            </div>

            <Slider
              label="Peak Motor Torque"
              value={ecoTorque}
              min={20}
              max={85}
              step={1}
              unit="Nm"
              onChange={(val) => handleStage(`${p}.eco.torque`, val)}
              valueFormatter={(val) => `${val} Nm (Range Extension)`}
            />

            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 pt-1 border-t border-stone-200/60">
              <div>
                <Slider
                  label="Assist Character"
                  value={ecoCharacter}
                  min={0}
                  max={9}
                  step={1}
                  unit="lvl"
                  onChange={(val) => handleStage(`${p}.eco.character`, val)}
                  valueFormatter={(val) => `Level ${val}`}
                />
              </div>
              <div>
                <Slider
                  label="Pedal Start Sensitivity"
                  value={ecoStart}
                  min={0}
                  max={4}
                  step={1}
                  unit="lvl"
                  onChange={(val) => handleStage(`${p}.eco.start`, val)}
                  valueFormatter={(val) =>
                    val === 0 ? "Level 0 (Smooth)" : val === 4 ? "Level 4 (Instant)" : `Level ${val}`
                  }
                />
              </div>
            </div>
          </div>

          {/* Profile Global Preferences: Nomenclature & Motor Curve Pattern */}
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4 pt-2">
            <div className="p-4 rounded-2xl border border-stone-200 bg-stone-50/50 flex flex-col gap-2">
              <span className="text-xs font-semibold text-stone-900">Display Nomenclature</span>
              <div className="flex items-center gap-1 bg-stone-200/70 p-1 rounded-xl">
                {[
                  { val: 0, label: "Eco / Normal / High" },
                  { val: 1, label: "Eco / Trail / Boost" },
                ].map((opt) => (
                  <button
                    type="button"
                    key={opt.val}
                    onClick={() => handleStage(`${p}.display`, opt.val)}
                    className={`flex-1 py-1.5 rounded-lg text-xs font-semibold transition-all cursor-pointer ${
                      profileDisplay === opt.val
                        ? "bg-white text-stone-900 shadow-2xs font-bold"
                        : "text-stone-600 hover:text-stone-900"
                    }`}
                  >
                    {opt.label}
                  </button>
                ))}
              </div>
            </div>

            <div className="p-4 rounded-2xl border border-stone-200 bg-stone-50/50 flex flex-col gap-2">
              <span className="text-xs font-semibold text-stone-900">Base Assist Pattern</span>
              <div className="flex items-center gap-1 bg-stone-200/70 p-1 rounded-xl">
                {[
                  { val: 5, label: "Standard (Pattern 5)" },
                  { val: 6, label: "Sport (Pattern 6)" },
                ].map((opt) => (
                  <button
                    type="button"
                    key={opt.val}
                    onClick={() => handleStage("assist-pattern", opt.val)}
                    className={`flex-1 py-1.5 rounded-lg text-xs font-semibold transition-all cursor-pointer ${
                      assistPattern === opt.val
                        ? "bg-white text-stone-900 shadow-2xs font-bold"
                        : "text-stone-600 hover:text-stone-900"
                    }`}
                  >
                    {opt.label}
                  </button>
                ))}
              </div>
            </div>
          </div>
        </Card>
      )}

      {/* 3. Drivetrain & Electronic Shifting Section */}
      {showDrivetrain && (
        <Card className="flex flex-col gap-6 shadow-xs border-stone-200">
          <div>
            <h3 className="text-base font-bold text-stone-900 flex items-center gap-2">
              <Cog size={18} className="text-indigo-600" />
              <span>Drivetrain & Electronic Shifting</span>
            </h3>
            <p className="text-xs text-stone-500 mt-0.5">
              Electronic gear shifting, chainring sizing, and Di2 automatic cadence.
            </p>
          </div>

          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
            {/* Shifting System */}
            <div className="p-4 rounded-2xl border border-stone-200 bg-stone-50/50 flex flex-col gap-2">
              <span className="text-xs font-semibold text-stone-900">Drivetrain Shifting System</span>
              <select
                value={shiftingSystem}
                onChange={(e) => handleStage("shifting-system", Number(e.target.value))}
                className="w-full text-xs font-mono p-2.5 rounded-xl border border-stone-300 bg-white text-stone-900 min-h-[44px] cursor-pointer"
              >
                <option value={0}>Mechanical Shifting</option>
                <option value={1}>Di2 Internal Gear Hub (IGH)</option>
                <option value={2}>Di2 Electronic Derailleur</option>
                <option value={3}>Di2 Auto Internal Gear Hub</option>
                <option value={4}>Di2 LinkGlide</option>
                <option value={5}>Di2 Full Automatic</option>
              </select>
            </div>

            {/* Rear Cassette Speeds */}
            <div className="p-4 rounded-2xl border border-stone-200 bg-stone-50/50 flex flex-col gap-2">
              <span className="text-xs font-semibold text-stone-900">Rear Cassette Speeds</span>
              <div className="flex items-center gap-1 bg-stone-200/70 p-1 rounded-xl flex-wrap">
                {[8, 9, 10, 11, 12].map((spd) => (
                  <button
                    type="button"
                    key={spd}
                    onClick={() => handleStage("rear-max-gear", spd)}
                    className={`flex-1 py-1.5 px-2 rounded-lg text-xs font-semibold transition-all cursor-pointer ${
                      rearMaxGear === spd
                        ? "bg-white text-stone-900 shadow-2xs font-bold"
                        : "text-stone-600 hover:text-stone-900"
                    }`}
                  >
                    {spd}s
                  </button>
                ))}
              </div>
            </div>

            {/* Front Chainring Teeth */}
            <div className="p-4 rounded-2xl border border-stone-200 bg-stone-50/50 flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <span className="text-xs font-semibold text-stone-900">Front Chainring</span>
                <span className="text-xs font-mono font-bold">{frontTeeth}T</span>
              </div>
              <Slider
                label="Chainring Tooth Count"
                value={frontTeeth}
                min={30}
                max={60}
                step={2}
                unit="T"
                onChange={(val) => handleStage("front-teeth", val)}
                valueFormatter={(val) => `${val}T`}
              />
            </div>

            {/* Smallest Rear Sprocket */}
            <div className="p-4 rounded-2xl border border-stone-200 bg-stone-50/50 flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <span className="text-xs font-semibold text-stone-900">Smallest Rear Sprocket</span>
                <span className="text-xs font-mono font-bold">{rearTeeth}T</span>
              </div>
              <Slider
                label="Cog Tooth Count"
                value={rearTeeth}
                min={10}
                max={14}
                step={1}
                unit="T"
                onChange={(val) => handleStage("rear-teeth", val)}
                valueFormatter={(val) => `${val}T`}
              />
            </div>

            {/* Auto Gear Cadence */}
            <div className="p-4 rounded-2xl border border-stone-200 bg-stone-50/50 flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <span className="text-xs font-semibold text-stone-900">Target Auto-Shift Cadence</span>
                <span className="text-xs font-mono font-bold">{autoGearCadence} rpm</span>
              </div>
              <Slider
                label="Pedaling Cadence Target"
                value={autoGearCadence}
                min={50}
                max={76}
                step={1}
                unit="rpm"
                onChange={(val) => handleStage("auto-gear-cadence", val)}
                valueFormatter={(val) => `${val} rpm`}
              />
            </div>

            {/* Stop Start Gear */}
            <div className="p-4 rounded-2xl border border-stone-200 bg-stone-50/50 flex flex-col gap-2">
              <span className="text-xs font-semibold text-stone-900">Stop Return Gear (Start Mode)</span>
              <div className="flex items-center gap-1 bg-stone-200/70 p-1 rounded-xl flex-wrap">
                {[
                  { val: 0, label: "Off" },
                  { val: 1, label: "1st" },
                  { val: 2, label: "2nd" },
                  { val: 3, label: "3rd" },
                  { val: 4, label: "4th" },
                  { val: 5, label: "5th" },
                ].map((opt) => (
                  <button
                    type="button"
                    key={opt.val}
                    onClick={() => handleStage("start-mode", opt.val)}
                    className={`flex-1 py-1.5 px-2 rounded-lg text-xs font-semibold transition-all cursor-pointer ${
                      startMode === opt.val
                        ? "bg-white text-stone-900 shadow-2xs font-bold"
                        : "text-stone-600 hover:text-stone-900"
                    }`}
                  >
                    {opt.label}
                  </button>
                ))}
              </div>
            </div>
          </div>
        </Card>
      )}

      {/* 4. Service & Maintenance Section */}
      {showMaintenance && (
        <Card className="flex flex-col gap-6 shadow-xs border-stone-200">
          <div>
            <h3 className="text-base font-bold text-stone-900 flex items-center gap-2">
              <Wrench size={18} className="text-emerald-600" />
              <span>Service & Inspection Reminders</span>
            </h3>
            <p className="text-xs text-stone-500 mt-0.5">
              Manage cycle computer service alerts and scheduled maintenance distance thresholds.
            </p>
          </div>

          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
            {/* Maintenance Alert Toggle */}
            <div className="p-4 rounded-2xl border border-stone-200 bg-stone-50/50 flex items-center justify-between">
              <div>
                <span className="text-xs font-semibold text-stone-900 block">
                  Service Reminder Wrench
                </span>
                <span className="text-[11px] text-stone-500 font-mono">
                  Display wrench icon when due
                </span>
              </div>
              <div className="flex items-center gap-1 bg-stone-200/70 p-1 rounded-xl">
                {[
                  { val: 0, label: "Off" },
                  { val: 1, label: "Active" },
                ].map((opt) => (
                  <button
                    type="button"
                    key={opt.val}
                    onClick={() => handleStage("maintenance-alert", opt.val)}
                    className={`px-3 py-1.5 rounded-lg text-xs font-semibold transition-all cursor-pointer ${
                      maintenanceAlert === opt.val
                        ? "bg-white text-stone-900 shadow-2xs font-bold"
                        : "text-stone-600 hover:text-stone-900"
                    }`}
                  >
                    {opt.label}
                  </button>
                ))}
              </div>
            </div>

            {/* Maintenance Distance */}
            <div className="p-4 rounded-2xl border border-stone-200 bg-stone-50/50 flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <span className="text-xs font-semibold text-stone-900">Service Distance Interval</span>
                <span className="text-xs font-mono font-bold">{maintenanceDistance} km</span>
              </div>
              <Slider
                label="Inspection Threshold"
                value={maintenanceDistance}
                min={100}
                max={5000}
                step={100}
                unit="km"
                onChange={(val) => handleStage("maintenance-distance", val)}
                valueFormatter={(val) => `${val} km`}
              />
            </div>
          </div>
        </Card>
      )}

      {/* 5. OEM Factory Bounds Section */}
      {showOem && (
        <Card className="flex flex-col gap-6 shadow-xs border-stone-200">
          <div>
            <h3 className="text-base font-bold text-stone-900 flex items-center gap-2">
              <ShieldCheck size={18} className="text-amber-600" />
              <span>OEM Factory Boundaries</span>
            </h3>
            <p className="text-xs text-stone-500 mt-0.5">
              Manufacturer power limits and minimum/maximum torque envelopes.
            </p>
          </div>

          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
            <div className="p-4 rounded-2xl border border-stone-200 bg-stone-50/50 flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <span className="text-xs font-semibold text-stone-900">Minimum Configurable Torque</span>
                <span className="text-xs font-mono font-bold">{torqueMin} Nm</span>
              </div>
              <Slider
                label="Lower Torque Clamp"
                value={torqueMin}
                min={20}
                max={40}
                step={5}
                unit="Nm"
                onChange={(val) => handleStage("torque-minimum", val)}
                valueFormatter={(val) => `${val} Nm`}
              />
            </div>

            <div className="p-4 rounded-2xl border border-stone-200 bg-stone-50/50 flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <span className="text-xs font-semibold text-stone-900">Maximum Configurable Torque</span>
                <span className="text-xs font-mono font-bold">{torqueMax} Nm</span>
              </div>
              <Slider
                label="Upper Torque Clamp"
                value={torqueMax}
                min={30}
                max={85}
                step={5}
                unit="Nm"
                onChange={(val) => handleStage("torque-maximum", val)}
                valueFormatter={(val) => `${val} Nm`}
              />
            </div>
          </div>
        </Card>
      )}

      {/* Floating Bottom Transaction Rail (always on desktop; on mobile when review is tapped) */}
      {stagedCount > 0 && (mobileReviewOpen || isDesktop) && (
        <aside
          tabIndex={-1}
          className={`fixed z-40 border border-stone-700 bg-stone-900 text-white shadow-2xl animate-in fade-in slide-in-from-bottom-4 duration-200 transaction-rail ${isDesktop ? "bottom-6 left-1/2 -translate-x-1/2 flex w-[calc(100%-2rem)] max-w-xl items-center justify-between gap-4 rounded-2xl p-4" : "inset-x-3 bottom-3 flex max-h-[min(60dvh,34rem)] flex-col gap-3 rounded-2xl p-4 pb-[calc(1rem+env(safe-area-inset-bottom))]"}`}
          data-testid="transaction-rail"
          aria-label="Changes to save"
        >
          <div className={`flex min-w-0 flex-col ${isDesktop ? "" : "min-h-0"}`}>
            <div className="flex items-center justify-between gap-3">
              <span className="text-sm font-semibold text-amber-300">
                Changes to save ({stagedCount})
              </span>
              {!isDesktop && (
                <button type="button" onClick={() => onMobileReviewOpenChange(false)} aria-label="Close save panel" className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg text-stone-300 hover:bg-white/10 hover:text-white">
                  <X size={18} />
                </button>
              )}
            </div>
            <div className={`mt-1 text-xs text-stone-300 ${isDesktop ? "font-mono" : "min-h-0 overflow-y-auto overscroll-contain"}`}>
              {Object.entries(state.stagedValues).map(([name, val]) => {
                const live = state.liveValues[name]?.value ?? "unset";
                const unit =
                  name.includes("tire") || name.includes("circumference")
                    ? " mm"
                    : name.includes("speed")
                    ? " km/h"
                    : name.includes("torque")
                    ? " Nm"
                    : "";
                return (
                  <div key={name} data-testid={`tx-item-${name}`} className={isDesktop ? "" : "border-t border-white/10 py-1.5 first:border-0"}>
                    <span className={isDesktop ? "" : "font-medium text-white"}>{name.replaceAll("-", " ")}: </span>{live}{unit} → {val}{unit}
                  </div>
                );
              })}
            </div>
          </div>

          <div className={`flex shrink-0 gap-2 ${isDesktop ? "items-center" : "items-center [&>*]:flex-1"}`}>
            <Button
              variant="custom"
              size="sm"
              onClick={() => {
                controller.discardAllStaged();
                onMobileReviewOpenChange(false);
              }}
              data-testid="btn-discard-staged"
              className="bg-transparent text-stone-300 hover:bg-white/10 hover:text-white text-xs min-h-[44px] min-w-[44px]"
            >
              Discard
            </Button>
            <Button
              variant="custom"
              size="sm"
              onClick={() => setDiffModalOpen(true)}
              className={`${isDesktop ? "bg-amber-400 text-stone-950 hover:bg-amber-300" : "hidden"} text-xs font-bold min-h-[44px] min-w-[44px]`}
            >
              Review changes
            </Button>
            <Button
              variant="custom"
              size="sm"
              onClick={async () => {
                await controller.commitStagedBatch();
                onMobileReviewOpenChange(false);
              }}
              data-testid="btn-commit-batch"
              className="bg-white text-stone-950 hover:bg-stone-200 text-xs font-bold min-h-[44px] min-w-[44px]"
            >
              Save to device
            </Button>
          </div>
        </aside>
      )}

      {/* Safety Audit & Diff Confirmation Modal */}
      <Modal
        open={diffModalOpen}
        onOpenChange={(open) => setDiffModalOpen(open)}
        title="Review changes before saving"
      >
        <div className="flex flex-col gap-4">
          <p className="text-xs text-stone-600">
            Check these changes before saving them to your device:
          </p>
          <div className="p-3 rounded-xl bg-stone-50 border border-stone-200 font-mono text-xs flex flex-col gap-1.5 max-h-60 overflow-y-auto">
            {Object.entries(state.stagedValues).map(([name, val]) => {
              const live = state.liveValues[name]?.value ?? "unset";
              return (
                <div key={name} className="flex items-center justify-between border-b border-stone-200/50 pb-1 last:border-none">
                  <span className="font-semibold text-stone-800">{name}</span>
                  <span className="text-stone-500">
                    <span className="line-through mr-1 text-stone-400">{live}</span>
                    <span className="text-amber-700 font-bold">{val}</span>
                  </span>
                </div>
              );
            })}
          </div>

          <div className="flex items-center justify-end gap-3 pt-2">
            <Button variant="outline" size="sm" onClick={() => setDiffModalOpen(false)} className="min-h-[44px]">
              Cancel
            </Button>
            <Button
              variant="primary"
              size="sm"
              onClick={handleCommit}
              className="bg-stone-900 text-white hover:bg-stone-800 min-h-[44px]"
            >
              Save to device
            </Button>
          </div>
        </div>
      </Modal>
    </div>
  );
}

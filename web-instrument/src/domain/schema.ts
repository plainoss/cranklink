/**
 * schema.ts - Classification, operator-facing labels, and device rules for settings.
 * Value formatting lives in settingFormat.ts.
 * Priority order:
 * 1. Geometry & Hardware (Tire Circumference, Destination, Speed Limit, Lighting, Walk Assist, Mounting Angle)
 * 2. Assist Profiles (Profile 1 & Profile 2 Eco/Trail/Boost Torque, Character, Start)
 * 3. Drivetrain & Shifting (Gear counts, Chainring/Cog teeth, Auto Gear Cadence, Di2 Start Mode)
 * 4. Service & Maintenance (Maintenance Alert, Distance, Scheduled Date)
 * 5. OEM Factory Bounds (OEM profile bounds & power caps)
 */

import type {
  SettingSpec,
  EnrichedSetting,
  SettingSectionId,
  SettingPreset,
  MotorModel,
} from "../types.ts";
import { isModernModel } from "./motorIdentity.ts";

export const TIRE_PRESETS: SettingPreset[] = [
  // Chart values are estimates; the loaded wheel's measured rollout is most accurate.
  { label: '29" × 2.1" (2288 mm)', value: 2288 },
  { label: '27.5" × 2.25" (2180 mm)', value: 2180 },
  { label: '26" × 2.0" (2055 mm)', value: 2055 },
  { label: '20" Cargo × 2.15" (1620 mm)', value: 1620 },
];

export function settingAcceptsValue(setting: EnrichedSetting, value: number): boolean {
  if (!Number.isFinite(value) || value < setting.userMin || value > setting.userMax) return false;
  if (setting.choices.length) return setting.choices.includes(value);
  const steps = (value - setting.userMin) / setting.userStep;
  return Math.abs(steps - Math.round(steps)) < 1e-7;
}

export function clampSettingValue(setting: EnrichedSetting, value: number): number {
  const bounded = Math.min(setting.userMax, Math.max(setting.userMin, value));
  const snapped = setting.userMin + Math.round((bounded - setting.userMin) / setting.userStep) * setting.userStep;
  return Math.min(setting.userMax, Math.max(setting.userMin, snapped));
}

const LEGACY_DESTINATION_LABELS: Record<number, string> = {
  0: "Region 0 (25 km/h)",
  1: "Region 1 / US Class 1 (32 km/h)",
  3: "Europe / Restricted (25 km/h)",
  4: "Australia / 25 km/h",
};

const MODERN_DESTINATION_LABELS: Record<number, string> = {
  0: "Standard (25 km/h)",
  1: "US Class 1 (32 km/h)",
  2: "Japan (24 km/h)",
  3: "Europe (25 km/h)",
  5: "US Class 3 (45 km/h)",
};

// Keep these ranges aligned with the destination validation in src/settings.rs.
export function destinationSpeedRange(model: string | undefined, destination: number): { min: number; max: number } | undefined {
  const modern = isModernModel(model);
  if (destination === 1) return { min: 19, max: 32 };
  if (destination === 0 || destination === 3 || (!modern && destination === 4)) {
    return { min: 15, max: 25 };
  }
  if (modern && destination === 2) return { min: 14, max: 24 };
  if (modern && destination === 5) return { min: 19, max: 45 };
  return undefined;
}

// Keep aligned with modern_torque_levels and validate_modern_torque in src/settings.rs.
/** Peak-torque values an EP600/EP801 accepts for a mode; undefined where any Nm value is allowed. */
export function modernTorqueLevels(
  model: string | undefined,
  mode: "eco" | "trail" | "boost",
  minimum: number,
  maximum: number,
): number[] | undefined {
  if (!isModernModel(model)) return undefined;
  if (!(minimum >= 20 && minimum <= maximum && maximum <= 85)) return undefined;
  const levels = minimum === 20 && maximum === 85
    ? [20, 27, 34, 42, 49, 56, 63, 70, 78, 85]
    : minimum === 20 && maximum === 50
      ? [20, 24, 27, 30, 33, 36, 40, 43, 46, 50]
      : Array.from({ length: 10 }, (_, i) => Math.round((i * maximum + (9 - i) * minimum) / 9));
  return mode === "eco" ? levels.slice(0, 8) : mode === "boost" ? levels.slice(2) : levels;
}

export function destinationForSpeed(model: string | undefined, speed: number, preferred: number, choices?: number[]): number | undefined {
  const candidates = [preferred, 0, 1, 2, 3, 4, 5];
  return candidates.find((destination) => {
    if (choices?.length && !choices.includes(destination)) return false;
    const range = destinationSpeedRange(model, destination);
    return range && Number.isFinite(speed) && speed >= range.min && speed <= range.max;
  });
}

interface SettingAnnotation {
  label: string;
  description: string;
  section: SettingSectionId;
  displayUnit?: string;
  presets?: SettingPreset[];
  choiceLabels?: Record<number, string>;
}

const ANNOTATIONS: Record<string, SettingAnnotation> = {
  "tire-circumference": {
    label: "Tire Circumference",
    description: "Distance per wheel revolution, used for speed and odometer.",
    section: "geometry",
    displayUnit: "mm",
    presets: TIRE_PRESETS,
  },
  "destination": {
    label: "Market Region",
    description: "Sets the legal assist-speed range for where the bike is ridden.",
    section: "geometry",
    displayUnit: "",
  },
  "speed-limit": {
    label: "Assist Speed Limit",
    description: "Motor assistance stops above this speed.",
    section: "geometry",
    displayUnit: "km/h",
    presets: [
      { label: "20 km/h", value: 20 },
      { label: "25 km/h (EU)", value: 25 },
      { label: "32 km/h (US Cl.1)", value: 32 },
    ],
  },
  "mounting-angle": {
    label: "Motor Mounting Angle",
    description: "Installation pitch of the drive unit in the frame.",
    section: "geometry",
    displayUnit: "deg",
  },
  "display-speed-adjustment": {
    label: "Speedometer Calibration",
    description: "Fine offset applied to the displayed speed.",
    section: "geometry",
    displayUnit: "0.1%",
  },
  "lamp": {
    label: "Wired Light Port",
    description: "Power to lights wired into the drive unit.",
    section: "geometry",
    displayUnit: "",
    choiceLabels: { 0: "Off", 1: "On", 2: "Auto" },
  },
  "light-output": {
    label: "Light Port Power Allocation",
    description: "Reserved power output budget for wired headlight/taillight",
    section: "geometry",
    displayUnit: "0.01 W",
  },
  "lighting-time": {
    label: "Lighting Reserve",
    description: "Minutes the lights stay on after assist cuts out on low battery.",
    section: "geometry",
    displayUnit: "min",
  },
  "walk-assist": {
    label: "Walk Assist",
    description: "Low-speed push while walking beside the bike.",
    section: "geometry",
    displayUnit: "",
    choiceLabels: { 0: "Disabled", 2: "Enabled" },
  },
  "assist-pattern": {
    label: "Assist Pattern",
    description: "Base motor response curve shared by both profiles.",
    section: "profiles",
    displayUnit: "",
    choiceLabels: { 5: "Standard", 6: "Sport" },
  },
  "torque-minimum": {
    label: "Minimum Profile Torque",
    description: "Lowest peak torque any assist mode may be set to.",
    section: "oem",
    displayUnit: "Nm",
  },
  "torque-maximum": {
    label: "Maximum Profile Torque",
    description: "Highest peak torque any assist mode may be set to.",
    section: "oem",
    displayUnit: "Nm",
  },
  "shifting-system": {
    label: "Shifting System",
    description: "Mechanical, or the Di2 derailleur or hub fitted to the bike.",
    section: "drivetrain",
    displayUnit: "",
    choiceLabels: {
      0: "Mechanical",
      1: "Di2 IGH",
      2: "Di2 Derailleur",
      3: "Di2 Auto IGH",
      4: "Di2 LinkGlide",
      5: "Di2 Auto",
    },
  },
  "rear-max-gear": {
    label: "Rear Gears",
    description: "Number of sprockets on the cassette or gears in the hub.",
    section: "drivetrain",
    displayUnit: "gears",
  },
  "front-teeth": {
    label: "Front Chainring",
    description: "Teeth on the front chainring.",
    section: "drivetrain",
    displayUnit: "T",
  },
  "rear-teeth": {
    label: "Smallest Rear Cog",
    description: "Teeth on the smallest rear sprocket.",
    section: "drivetrain",
    displayUnit: "T",
  },
  "rear-teeth-pattern": {
    label: "Cassette Progression Pattern",
    description: "Shimano cassette sprocket combination index",
    section: "drivetrain",
    displayUnit: "",
  },
  "auto-gear-mode": {
    label: "Automatic Shifting Mode",
    description: "Default electronic auto-shift state",
    section: "drivetrain",
    displayUnit: "",
    choiceLabels: { 0: "Manual", 1: "Auto" },
  },
  "auto-gear-cadence": {
    label: "Auto-Shift Target Cadence",
    description: "Pedaling cadence Di2 automatic shifting aims for.",
    section: "drivetrain",
    displayUnit: "rpm",
  },
  "shifting-advice": {
    label: "Shift Timing Indicator",
    description: "Display recommended gear shift prompts on cycle computer",
    section: "drivetrain",
    displayUnit: "",
    choiceLabels: { 0: "Off", 1: "On" },
  },
  "start-mode": {
    label: "Start Gear After Stopping",
    description: "Di2 shifts to this gear when you stop. Off keeps the current gear.",
    section: "drivetrain",
    displayUnit: "gear",
  },
  "shift-limit-up": {
    label: "Multi-Shift Upshift Limit",
    description: "Maximum gears shifted when holding upshift switch",
    section: "drivetrain",
    displayUnit: "",
    choiceLabels: { 2: "2 Gears", 3: "3 Gears", 255: "No Limit" },
  },
  "shift-limit-down": {
    label: "Multi-Shift Downshift Limit",
    description: "Maximum gears shifted when holding downshift switch",
    section: "drivetrain",
    displayUnit: "",
    choiceLabels: { 2: "2 Gears", 3: "3 Gears", 255: "No Limit" },
  },
  "maintenance-alert": {
    label: "Service Reminder",
    description: "Shows a wrench on the display when service is due.",
    section: "maintenance",
    displayUnit: "",
    choiceLabels: { 0: "Disabled", 1: "Enabled" },
  },
  "maintenance-distance": {
    label: "Distance Until Service",
    description: "Riding distance before the service reminder appears.",
    section: "maintenance",
    displayUnit: "km",
  },
  "maintenance-date": {
    label: "Scheduled Service Date",
    description: "Calendar date for next scheduled inspection (YYYY-MM-DD or unset)",
    section: "maintenance",
    displayUnit: "",
  },
  "maintenance-year": {
    label: "Service Year (2000+)",
    description: "Raw calendar year offset from 2000",
    section: "maintenance",
    displayUnit: "yr",
  },
  "maintenance-month": {
    label: "Service Month",
    description: "Calendar month (1–12)",
    section: "maintenance",
    displayUnit: "mo",
  },
  "maintenance-day": {
    label: "Service Day",
    description: "Calendar day of month (1–31)",
    section: "maintenance",
    displayUnit: "day",
  },
};

const GEOMETRY_SORT_ORDER = [
  "tire-circumference",
  "destination",
  "speed-limit",
  "mounting-angle",
  "display-speed-adjustment",
  "lamp",
  "light-output",
  "lighting-time",
  "walk-assist",
];

function formatProfileSetting(name: string): {
  label: string;
  description: string;
  section: SettingSectionId;
  displayUnit: string;
  choiceLabels?: Record<number, string>;
} | null {
  if (name === "profile1.name" || name === "profile2.name") {
    const p = name.startsWith("profile1") ? 1 : 2;
    return {
      label: `Profile ${p} Label`,
      description: `ASCII display name for Assist Profile ${p} (max 10 chars)`,
      section: "profiles",
      displayUnit: "ASCII",
    };
  }

  const displayMatch = name.match(/^profile([12])\.display(-oem)?$/);
  if (displayMatch) {
    const p = displayMatch[1];
    const isOem = !!displayMatch[2];
    return {
      label: `Profile ${p} Mode Names${isOem ? " (OEM)" : ""}`,
      description: "Display mode nomenclature on cycle computer",
      section: isOem ? "oem" : "profiles",
      displayUnit: "",
      choiceLabels: { 0: "Eco / Normal / High", 1: "Eco / Trail / Boost" },
    };
  }

  const modeMatch = name.match(/^profile([12])\.(eco|trail|boost)\.(torque|character|start)(-oem)?$/);
  if (modeMatch) {
    const [, p, mode, field, oemSuffix] = modeMatch;
    const isOem = !!oemSuffix;
    const modeTitle = mode.charAt(0).toUpperCase() + mode.slice(1);
    const fieldMap: Record<string, { title: string; desc: string; unit: string }> = {
      torque: {
        title: "Peak Torque",
        desc: `Peak motor torque ceiling in ${modeTitle} mode`,
        unit: "Nm",
      },
      character: {
        title: "Assist Character",
        desc: `Support curve aggressiveness in ${modeTitle} mode`,
        unit: "lvl",
      },
      start: {
        title: "Start Response",
        desc: `Initial pedal engagement responsiveness in ${modeTitle} mode`,
        unit: "lvl",
      },
    };
    const f = fieldMap[field];
    return {
      label: `Profile ${p} ${modeTitle} ${f.title}${isOem ? " (OEM)" : ""}`,
      description: f.desc,
      section: isOem ? "oem" : "profiles",
      displayUnit: f.unit,
    };
  }

  const maxPowerMatch = name.match(/^max-power\.(eco|trail|boost)$/);
  if (maxPowerMatch) {
    const mode = maxPowerMatch[1];
    const modeTitle = mode.charAt(0).toUpperCase() + mode.slice(1);
    return {
      label: `${modeTitle} Maximum Electrical Power`,
      description: `OEM peak power output cap in ${modeTitle} mode`,
      section: "oem",
      displayUnit: "W",
    };
  }

  return null;
}

function enrichSetting(
  spec: SettingSpec,
  modelSlug: MotorModel = "ep800-crg",
): EnrichedSetting {
  const scale = spec.wire_units_per_input || 1.0;
  const userMin = spec.min_raw / scale;
  const userMax = spec.max_raw / scale;
  const userStep = spec.step_raw / scale;

  const explicit = ANNOTATIONS[spec.name];
  const profileMeta = formatProfileSetting(spec.name);

  let section: SettingSectionId = "geometry";
  let label = spec.name;
  let description = "";
  let displayUnit = spec.unit === "enum" || spec.unit === "bool" || spec.unit === "raw index" ? "" : spec.unit;
  let choiceLabels: Record<number, string> | null = null;
  let presets: SettingPreset[] | null = null;

  if (explicit) {
    section = explicit.section;
    label = explicit.label;
    description = explicit.description;
    if (explicit.displayUnit !== undefined) displayUnit = explicit.displayUnit;
    if (explicit.choiceLabels) choiceLabels = explicit.choiceLabels;
    if (explicit.presets) presets = explicit.presets;
  } else if (profileMeta) {
    section = profileMeta.section;
    label = profileMeta.label;
    description = profileMeta.description;
    displayUnit = profileMeta.displayUnit;
    if (profileMeta.choiceLabels) choiceLabels = profileMeta.choiceLabels;
  } else if (spec.oem) {
    section = "oem";
  }

  if (spec.name === "destination") {
    choiceLabels =
      isModernModel(modelSlug)
        ? MODERN_DESTINATION_LABELS
        : LEGACY_DESTINATION_LABELS;
  }

  if (!choiceLabels && (spec.unit === "bool" || (spec.min_raw === 0 && spec.max_raw === 1 && spec.step_raw === 1 && spec.value_type === "number" && !spec.name.includes("display")))) {
    choiceLabels = { 0: "Off", 1: "On" };
  }

  return {
    ...spec,
    section,
    label,
    description,
    displayUnit,
    userMin,
    userMax,
    userStep,
    choiceLabels,
    presets,
  };
}

export function enrichSchema(
  specs: SettingSpec[],
  modelSlug: MotorModel = "ep800-crg",
): EnrichedSetting[] {
  const enriched = specs.map((s) => enrichSetting(s, modelSlug));
  return enriched.sort((a, b) => {
    if (a.section === "geometry" && b.section === "geometry") {
      const ia = GEOMETRY_SORT_ORDER.indexOf(a.name);
      const ib = GEOMETRY_SORT_ORDER.indexOf(b.name);
      if (ia !== -1 && ib !== -1) return ia - ib;
      if (ia !== -1) return -1;
      if (ib !== -1) return 1;
    }
    return 0;
  });
}

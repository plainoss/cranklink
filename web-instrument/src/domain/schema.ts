/**
 * schema.ts - Pure domain classification and human-readable metadata for SettingSpec.
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
  SimulatedModel,
} from "../types.ts";

export interface ParameterGroupInfo {
  id: SettingSectionId;
  title: string;
  subtitle: string;
}

export const PARAMETER_GROUPS: ParameterGroupInfo[] = [
  {
    id: "geometry",
    title: "Bike Hardware & Geometry",
    subtitle: "Wheel circumference, regional speed standard, mounting angle, and lighting",
  },
  {
    id: "profiles",
    title: "Assist Profiles & Motor Tuning",
    subtitle: "Eco, Trail, and Boost torque limits, assist character, and start sensitivity",
  },
  {
    id: "drivetrain",
    title: "Drivetrain & Electronic Shifting",
    subtitle: "Sprocket tooth counts, Di2 automatic shifting cadence, and multi-shift limits",
  },
  {
    id: "maintenance",
    title: "Service & Maintenance",
    subtitle: "Service interval alerts, distance reminders, and scheduled inspection dates",
  },
  {
    id: "oem",
    title: "OEM Factory Bounds",
    subtitle: "Manufacturer baseline profile envelopes and maximum power caps",
  },
];

export const TIRE_PRESETS: SettingPreset[] = [
  // Chart values are estimates; the loaded wheel's measured rollout is most accurate.
  { label: '29" × 2.1" (2288 mm)', value: 2288 },
  { label: '27.5" × 2.25" (2180 mm)', value: 2180 },
  { label: '26" × 2.0" (2055 mm)', value: 2055 },
  { label: '20" Cargo × 2.15" (1620 mm)', value: 1620 },
];

export const LEGACY_DESTINATION_LABELS: Record<number, string> = {
  0: "Region 0 (25 km/h)",
  1: "Region 1 / US Class 1 (32 km/h)",
  3: "Europe / Restricted (25 km/h)",
  4: "Australia / 25 km/h",
};

export const MODERN_DESTINATION_LABELS: Record<number, string> = {
  0: "Standard (25 km/h)",
  1: "US Class 1 (32 km/h)",
  2: "Japan (24 km/h)",
  3: "Europe (25 km/h)",
  5: "US Class 3 (45 km/h)",
};

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
    description: "Rolling distance per wheel revolution used for speed and odometer calculation",
    section: "geometry",
    displayUnit: "mm",
    presets: TIRE_PRESETS,
  },
  "destination": {
    label: "Market Region & Speed Standard",
    description: "Regulatory market jurisdiction governing assist speed ceiling",
    section: "geometry",
    displayUnit: "",
  },
  "speed-limit": {
    label: "Maximum Assist Speed",
    description: "Motor assist cutoff speed threshold (must fall within active region ceiling)",
    section: "geometry",
    displayUnit: "km/h",
    presets: [
      { label: "20 km/h", value: 20 },
      { label: "25 km/h (EU)", value: 25 },
      { label: "32 km/h (US Cl.1)", value: 32 },
    ],
  },
  "mounting-angle": {
    label: "Drive Unit Mounting Angle",
    description: "Physical installation pitch of the motor chassis in the frame",
    section: "geometry",
    displayUnit: "deg",
  },
  "display-speed-adjustment": {
    label: "Speedometer Calibration Offset",
    description: "Fine offset applied to displayed speed readout",
    section: "geometry",
    displayUnit: "0.1%",
  },
  "lamp": {
    label: "Wired Light Port",
    description: "12V auxiliary lighting terminal state",
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
    label: "Lighting Reserve Duration",
    description: "Minutes lights remain active after drive battery reaches assist cutoff",
    section: "geometry",
    displayUnit: "min",
  },
  "walk-assist": {
    label: "Walk Assist Mode",
    description: "Low-speed motor push assistance when walking beside the bike",
    section: "geometry",
    displayUnit: "",
    choiceLabels: { 0: "Disabled", 2: "Enabled" },
  },
  "assist-pattern": {
    label: "Active Assist Pattern",
    description: "Base motor response curve index",
    section: "profiles",
    displayUnit: "",
    choiceLabels: { 5: "Pattern 5 (Standard)", 6: "Pattern 6 (Sport)" },
  },
  "torque-minimum": {
    label: "OEM Minimum Torque Bound",
    description: "Lower bound for configurable profile torque",
    section: "oem",
    displayUnit: "Nm",
  },
  "torque-maximum": {
    label: "OEM Maximum Torque Bound",
    description: "Upper bound for configurable profile torque",
    section: "oem",
    displayUnit: "Nm",
  },
  "shifting-system": {
    label: "Drivetrain Shifting Type",
    description: "Mechanical or electronic Di2 derailleur / internal gear hub",
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
    label: "Rear Cassette Speeds",
    description: "Number of sprockets on rear cassette or internal hub",
    section: "drivetrain",
    displayUnit: "gears",
  },
  "front-teeth": {
    label: "Front Chainring Teeth",
    description: "Tooth count on front drive sprocket",
    section: "drivetrain",
    displayUnit: "T",
  },
  "rear-teeth": {
    label: "Smallest Rear Cog Teeth",
    description: "Tooth count on top-speed rear sprocket",
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
    description: "Preferred pedaling cadence for automatic gear transitions",
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
    label: "Start Gear Downshift",
    description: "Target gear automatically selected when coming to a stop (0 = Off)",
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
    label: "Service Reminder Alert",
    description: "Show maintenance wrench notification on display when threshold is reached",
    section: "maintenance",
    displayUnit: "",
    choiceLabels: { 0: "Disabled", 1: "Enabled" },
  },
  "maintenance-distance": {
    label: "Distance Until Service",
    description: "Remaining riding distance before scheduled inspection",
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
      label: `Profile ${p} Mode Naming${isOem ? " (OEM)" : ""}`,
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
        title: "Max Torque",
        desc: `Peak motor torque ceiling in ${modeTitle} mode`,
        unit: "Nm",
      },
      character: {
        title: "Assist Character",
        desc: `Support curve aggressiveness in ${modeTitle} mode`,
        unit: "lvl",
      },
      start: {
        title: "Assist Start",
        desc: `Initial pedal engagement responsiveness in ${modeTitle} mode`,
        unit: "lvl",
      },
    };
    const f = fieldMap[field];
    return {
      label: `Profile ${p} · ${modeTitle} ${f.title}${isOem ? " (OEM)" : ""}`,
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

export function enrichSetting(
  spec: SettingSpec,
  modelSlug: SimulatedModel = "ep800-crg",
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
      modelSlug === "ep800-crg"
        ? LEGACY_DESTINATION_LABELS
        : MODERN_DESTINATION_LABELS;
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
  modelSlug: SimulatedModel = "ep800-crg",
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

export function formatSettingValue(
  setting: EnrichedSetting,
  value: number | string | null | undefined,
): string {
  if (value === null || value === undefined) return "unset";
  if (setting.choiceLabels && typeof value === "number" && setting.choiceLabels[value] !== undefined) {
    return `${setting.choiceLabels[value]} (${value})`;
  }
  if (setting.displayUnit && setting.value_type === "number") {
    return `${value} ${setting.displayUnit}`;
  }
  return String(value);
}

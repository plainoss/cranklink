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
  SettingsModel,
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

export const E6002_DESTINATION_LABELS: Record<number, string> = {
  0: "Europe (EU)",
  1: "United States (US)",
  4: "Korea",
};

// Keep these ranges aligned with the destination validation in src/settings.rs.
export function destinationSpeedRange(model: string | undefined, destination: number): { min: number; max: number } | undefined {
  if (model === "e6000" || model === "e6001" || model === "e6002-pre33" || model === "e6002") return undefined;
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
/**
 * Peak-torque values an EP600/EP801 accepts for a mode; undefined where any Nm
 * value is allowed, and empty for OEM bounds without a verified vendor table,
 * whose torque writes the core refuses.
 */
export function modernTorqueLevels(
  model: string | undefined,
  mode: "eco" | "trail" | "boost",
  minimum: number,
  maximum: number,
): number[] | undefined {
  if (!isModernModel(model)) return undefined;
  const levels = minimum === 20 && maximum === 85
    ? [20, 27, 34, 42, 49, 56, 63, 70, 78, 85]
    : minimum === 20 && maximum === 50
      ? [20, 24, 27, 30, 33, 36, 40, 43, 46, 50]
      : [];
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
  ...Object.fromEntries(["a", "b", "c", "s"].map((channel) => [`switch-${channel}-function`, {
    label: `Switch ${channel.toUpperCase()} function`, description: "Button assignment for this control.", section: "drivetrain",
    displayUnit: "", choiceLabels: { 0: "Front shift up", 1: "Front shift down", 2: "Rear shift up", 3: "Rear shift down" },
  }])),
  "multi-shift-speed": {
    label: "Multi-shift speed", description: "How quickly repeated shifts are made while a button is held.", section: "drivetrain", displayUnit: "",
    choiceLabels: { 0: "Off", 1: "Very slow", 2: "Slow", 3: "Normal", 4: "Fast", 5: "Very fast", 6: "Super fast" },
  },
  "synchro-shift-interval": {
    label: "Synchronized shift interval", description: "Delay between coordinated front and rear shifts.", section: "drivetrain", displayUnit: "",
    choiceLabels: { 0: "Normal", 1: "Slow", 2: "Very slow" },
  },
  ...Object.fromEntries(["up", "down"].map((direction) => [`multi-shift-limit.${direction}`, {
    label: `Multi-shift ${direction} limit`, description: "Maximum gears shifted while holding the button.", section: "drivetrain", displayUnit: "",
    choiceLabels: { 2: "2 gears", 3: "3 gears", 255: "Unlimited" },
  }])),
  "derailleur-adjustment": { label: "Current derailleur adjustment", description: "Current adjustment reported by the derailleur.", section: "drivetrain", displayUnit: "steps" },
  "derailleur-adjustment-maximum": { label: "Maximum derailleur adjustment", description: "Adjustment limit reported by the derailleur.", section: "drivetrain", displayUnit: "steps" },
  "hub-adjustment": { label: "Hub adjustment target", description: "Target adjustment position. Applying the current position also cancels a different pending target.", section: "drivetrain", displayUnit: "steps" },
  "hub-gear-count": { label: "Hub gear count", description: "Number of gears supported by the connected hub.", section: "drivetrain", displayUnit: "gears" },
  "display-beep": { label: "Display beep", description: "Audible feedback from the cycle computer.", section: "geometry", displayUnit: "", choiceLabels: { 0: "Off", 1: "On" } },
  "display-distance-unit": { label: "Distance unit", description: "Units used by the cycle computer.", section: "geometry", displayUnit: "", choiceLabels: { 0: "Kilometres", 1: "Miles" } },
  "display-language": { label: "Display language", description: "Language used by the cycle computer.", section: "geometry", displayUnit: "", choiceLabels: { 0: "English", 1: "French", 2: "German", 3: "Dutch", 4: "Italian", 5: "Spanish" } },
  "display-backlight": { label: "Display backlight", description: "Backlight behavior on the cycle computer.", section: "geometry", displayUnit: "", choiceLabels: { 0: "Off", 1: "On", 2: "Manual" } },
  "display-brightness": { label: "Display brightness", description: "Cycle-computer screen brightness.", section: "geometry", displayUnit: "%" },
  "tire-circumference": {
    label: "Tire Circumference",
    description: "Distance the loaded tire travels in one full wheel turn. The motor uses this measurement for speed and distance readings.",
    section: "geometry",
    displayUnit: "mm",
    presets: TIRE_PRESETS,
  },
  "destination": {
    label: "Market Region",
    description: "Selects the market specification and the assist-speed range the motor accepts. Choose the region that applies where you ride.",
    section: "geometry",
    displayUnit: "",
  },
  "speed-limit": {
    label: "Assist Speed Limit",
    description: "Speed above which the motor stops helping. It does not limit how fast you can pedal or coast.",
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
    description: "Angle at which the motor is installed in the frame. Use the bike manufacturer's installation specification.",
    section: "geometry",
    displayUnit: "deg",
  },
  "display-speed-adjustment": {
    label: "Speedometer Calibration",
    description: "Percentage correction applied to the speedometer reading. This is separate from tire circumference and the assist speed limit.",
    section: "geometry",
    displayUnit: "0.1%",
  },
  "lamp": {
    label: "Wired Light Port",
    description: "Enables the motor's power connection for wired lights. Auto is available only on motors that report it as a supported option.",
    section: "geometry",
    displayUnit: "",
    choiceLabels: { 0: "Off", 1: "On", 2: "Auto" },
  },
  "light-output": {
    label: "Light Port Power Allocation",
    description: "Power budget reserved for a headlight and taillight connected to the motor, in watts. Match the bike's lighting specification.",
    section: "geometry",
    displayUnit: "0.01 W",
  },
  "lighting-time": {
    label: "Lighting Reserve",
    description: "Lighting time reserved after low battery stops motor assistance, in minutes. The motor reserves energy for the wired lights.",
    section: "geometry",
    displayUnit: "min",
  },
  "walk-assist": {
    label: "Walk Assist",
    description: "Allows motor support while you walk beside the bike and hold its walk-assist control. This does not set the normal riding assistance level.",
    section: "geometry",
    displayUnit: "",
    choiceLabels: { 0: "Disabled", 2: "Enabled" },
  },
  "assist-pattern": {
    label: "Assist Pattern",
    description: "Selects the motor's base assistance pattern. This affects how support is delivered alongside the individual profile settings.",
    section: "profiles",
    displayUnit: "",
    choiceLabels: { 5: "Standard", 6: "Sport" },
  },
  "torque-minimum": {
    label: "Minimum Profile Torque",
    description: "Factory lower bound for the peak torque you can select in an assist profile. Torque is the motor's turning force, measured in newton-metres.",
    section: "oem",
    displayUnit: "Nm",
  },
  "torque-maximum": {
    label: "Maximum Profile Torque",
    description: "Factory upper bound for the peak torque you can select in an assist profile. Each mode's torque setting must stay within this limit.",
    section: "oem",
    displayUnit: "Nm",
  },
  "shifting-system": {
    label: "Shifting System",
    description: "Identifies the mechanical or Di2 gear system installed on the bike. Select the system actually fitted so motor and shifting functions agree.",
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
    description: "Number of available rear gears: sprockets on a cassette or gears inside the hub. Match the installed drivetrain.",
    section: "drivetrain",
    displayUnit: "gears",
  },
  "front-teeth": {
    label: "Front Chainring",
    description: "Number of teeth on the front chainring. This describes the gearing fitted to the bike.",
    section: "drivetrain",
    displayUnit: "T",
  },
  "rear-teeth": {
    label: "Smallest Rear Cog",
    description: "Number of teeth on the smallest rear sprocket. Use the tooth count of the installed cassette.",
    section: "drivetrain",
    displayUnit: "T",
  },
  "rear-teeth-pattern": {
    label: "Cassette Progression Pattern",
    description: "Identifies the cassette's sequence of sprocket sizes. The numbered patterns are Shimano drivetrain options, not tooth counts.",
    section: "drivetrain",
    displayUnit: "",
  },
  "auto-gear-mode": {
    label: "Automatic Shifting Mode",
    description: "Selects whether a compatible electronic drivetrain starts in manual or automatic shifting mode.",
    section: "drivetrain",
    displayUnit: "",
    choiceLabels: { 0: "Manual", 1: "Auto" },
  },
  "auto-gear-cadence": {
    label: "Auto-Shift Target Cadence",
    description: "Pedaling rate that compatible Di2 automatic shifting aims to maintain, in crank revolutions per minute.",
    section: "drivetrain",
    displayUnit: "rpm",
  },
  "shifting-advice": {
    label: "Shift Timing Indicator",
    description: "Shows suggested gear changes on a compatible cycle computer. The prompts advise you when to shift; they do not shift the gears.",
    section: "drivetrain",
    displayUnit: "",
    choiceLabels: { 0: "Off", 1: "On" },
  },
  "start-mode": {
    label: "Start Gear After Stopping",
    description: "Gear a compatible Di2 system selects after you stop, ready for the next start. Off leaves the current gear selected.",
    section: "drivetrain",
    displayUnit: "gear",
  },
  "shift-limit-up": {
    label: "Multi-Shift Upshift Limit",
    description: "Maximum number of gear changes made while you hold the upshift switch. No Limit allows shifting to continue through the available gears.",
    section: "drivetrain",
    displayUnit: "",
    choiceLabels: { 2: "2 Gears", 3: "3 Gears", 255: "No Limit" },
  },
  "shift-limit-down": {
    label: "Multi-Shift Downshift Limit",
    description: "Maximum number of gear changes made while you hold the downshift switch. No Limit allows shifting to continue through the available gears.",
    section: "drivetrain",
    displayUnit: "",
    choiceLabels: { 2: "2 Gears", 3: "3 Gears", 255: "No Limit" },
  },
  "maintenance-alert": {
    label: "Service Reminder",
    description: "Enables the service reminder on the cycle computer. Its distance and date settings determine when the reminder is due.",
    section: "maintenance",
    displayUnit: "",
    choiceLabels: { 0: "Disabled", 1: "Enabled" },
  },
  "maintenance-distance": {
    label: "Distance Until Service",
    description: "Distance to the next service reminder, in kilometres. This sets a reminder; it does not perform or record a service.",
    section: "maintenance",
    displayUnit: "km",
  },
  "maintenance-date": {
    label: "Scheduled Service Date",
    description: "Date for the next service reminder. Enter YYYY-MM-DD, or unset to clear the scheduled date.",
    section: "maintenance",
    displayUnit: "",
  },
  "maintenance-year": {
    label: "Service Year",
    description: "Calendar year for the next service reminder. It forms the scheduled date together with the service month and day.",
    section: "maintenance",
    displayUnit: "",
  },
  "maintenance-month": {
    label: "Service Month",
    description: "Month for the next service reminder, from 1 for January to 12 for December. It is part of the scheduled service date.",
    section: "maintenance",
    displayUnit: "mo",
  },
  "maintenance-day": {
    label: "Service Day",
    description: "Day of the month for the next service reminder. Combine it with the service month and year to form a valid date.",
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
      description: `Name shown for assist profile ${p}. Use up to 10 basic Latin letters, numbers, or supported punctuation.`,
      section: "profiles",
      displayUnit: "",
    };
  }

  const displayMatch = name.match(/^profile([12])\.display(-oem)?$/);
  if (displayMatch) {
    const p = displayMatch[1];
    const isOem = !!displayMatch[2];
    return {
      label: `Profile ${p} Mode Names${isOem ? " (OEM)" : ""}`,
      description: isOem ? "Factory setting for the assist-mode names available on the cycle computer. This changes the labels, not the mode's torque or response." : "Names used for the assist modes on the cycle computer. This changes the labels, not the mode's torque or response.",
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
        desc: `Maximum turning force the motor may provide in ${modeTitle} mode, measured in newton-metres. Actual support also depends on your pedaling and the assist character.`,
        unit: "Nm",
      },
      character: {
        title: "Assist Character",
        desc: `How much assistance the motor provides for your pedaling effort in ${modeTitle} mode. This adjusts the support response within the mode's torque limit.`,
        unit: "lvl",
      },
      start: {
        title: "Start Response",
        desc: `Controls how quickly assistance builds when you start pedaling in ${modeTitle} mode. It affects the initial response rather than the mode's maximum torque.`,
        unit: "lvl",
      },
    };
    const f = fieldMap[field];
    return {
      label: `Profile ${p} ${modeTitle} ${f.title}${isOem ? " (OEM)" : ""}`,
      description: isOem ? `Factory setting for ${modeTitle} ${f.title.toLowerCase()}. ${f.desc}` : f.desc,
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
      description: `Factory limit for electrical power supplied to the motor in ${modeTitle} mode, in watts. This is separate from the mode's torque limit.`,
      section: "oem",
      displayUnit: "W",
    };
  }

  return null;
}

/** Known presentation metadata is also available when inspecting backups without a live schema. */
export function settingAnnotation(name: string): SettingAnnotation | null {
  return ANNOTATIONS[name] ?? formatProfileSetting(name);
}

function enrichSetting(
  spec: SettingSpec,
  modelSlug: SettingsModel = "ep800-crg",
): EnrichedSetting {
  const scale = spec.wire_units_per_input || 1.0;
  const userMin = spec.min_raw / scale;
  const userMax = spec.max_raw / scale;
  const userStep = spec.step_raw / scale;

  const explicit = ANNOTATIONS[spec.name];
  const profileMeta = formatProfileSetting(spec.name);

  let section: SettingSectionId = "geometry";
  let label = spec.name;
  const words = spec.name.replace(/[.-]/g, " ");
  let description = spec.read_only
    ? `Current ${words} reported by the connected component.`
    : `Vendor-specific ${words} setting. Refer to this component's documentation for its effect.`;
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
    if (modelSlug === "e6002") description = "Motor market selection. Choose the region that applies where the bike is ridden.";
    // E6002 exposes verified markets without a separate speed-limit setting.
    // Other motors use the range enforced by the core, as the CLI prints it.
    choiceLabels = Object.fromEntries(spec.choices.flatMap((choice) => {
      if (modelSlug === "e6002") return E6002_DESTINATION_LABELS[choice] ? [[choice, E6002_DESTINATION_LABELS[choice]]] : [];
      const range = destinationSpeedRange(modelSlug, choice);
      return range ? [[choice, `${choice} (${range.min}–${range.max} km/h)`]] : [];
    }));
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
  modelSlug: SettingsModel = "ep800-crg",
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

import type { DeviceIdentity, MotorModel, SettingsModel } from "../types.ts";

const MODELS: Record<number, { slug: MotorModel; name: string }> = {
  0x08: { slug: "e6000", name: "E6000 family" },
  0x0a: { slug: "e6001", name: "DU-E6001" },
  0x0c: { slug: "e6001", name: "E6001 family" },
  0x12: { slug: "e8000", name: "E8000 family" },
  0x15: { slug: "e6002", name: "E6002 family" },
  0x20: { slug: "e6100", name: "E6100 family" },
  0x21: { slug: "e7000", name: "DU-E7000" },
  0x23: { slug: "ep800-crg", name: "DU-EP800" },
  0x30: { slug: "ep600", name: "DU-EP600" },
  0x35: { slug: "ep801", name: "DU-EP801" },
  0x37: { slug: "ep500", name: "DU-EP500" },
  0x38: { slug: "e5100", name: "DU-E5100" },
};

export function isMotorStock(series: number, unit: number): boolean {
  return unit === 0 && MODELS[series] !== undefined;
}

/** Catalogued model names, e.g. ST-6870-L or SW-M8150-R/IR; never contain quotes. */
export const CATALOG_NAME = /^[A-Z0-9]+(?:[-/][A-Z0-9]+)+$/;

/** Component names come from the Rust live identity resolver, never a motor fallback. */
export function detectDeviceIdentity(parsed: {
  model_series?: number; model_unit?: number; cargo?: unknown;
  model_name?: unknown; settings_model?: unknown;
  dcas_version?: unknown;
}): Pick<DeviceIdentity, "modelSlug" | "model" | "isCargo" | "isMotor" | "modelUnit"> {
  if (!Number.isInteger(parsed.model_series) || parsed.model_series! < 0 || parsed.model_series! > 255
      || !Number.isInteger(parsed.model_unit) || parsed.model_unit! < 0 || parsed.model_unit! > 255) {
    throw new Error("Component response lacks a complete stock identity.");
  }
  if (isMotorStock(parsed.model_series!, parsed.model_unit!)) {
    return { ...detectMotorIdentity(parsed), isMotor: true, modelUnit: 0 };
  }
  const name = typeof parsed.model_name === "string" ? parsed.model_name : "";
  if (!Number.isInteger(parsed.model_series) || !Number.isInteger(parsed.model_unit)
      || !CATALOG_NAME.test(name) || name.startsWith("DU-")
      || parsed.settings_model !== name) {
    throw new Error("Component identity and settings schema do not match.");
  }
  return { modelSlug: name as SettingsModel, model: name, isMotor: false, isCargo: false, modelUnit: parsed.model_unit! };
}

/** Series selects the schema; verified backend metadata distinguishes shared-series variants. */
export function detectMotorIdentity(parsed: { model_series?: number; model_unit?: number; cargo?: unknown; model_name?: unknown; settings_model?: unknown; dcas_version?: unknown }) {
  const model = parsed.model_series === undefined ? undefined : MODELS[parsed.model_series];
  if (!model || (parsed.model_unit !== undefined && parsed.model_unit !== 0)) {
    throw new Error("Settings are not supported for this drive-unit identity.");
  }
  let modelSlug = model.slug;
  if (parsed.model_series === 0x15) {
    const version = typeof parsed.dcas_version === "string" && /^\d+\.\d+\.\d+\.\d+$/.test(parsed.dcas_version)
      ? parsed.dcas_version.split(".").map(Number) : null;
    if (!version || !version.every(Number.isSafeInteger)) {
      throw new Error("Cannot select E6002 settings without a valid drive-unit firmware version.");
    }
    modelSlug = version[0] > 3 || (version[0] === 3 && version[1] >= 3) ? "e6002" : "e6002-pre33";
  }
  if (parsed.settings_model !== undefined && parsed.settings_model !== modelSlug) {
    throw new Error("Drive-unit identity and settings schema do not match.");
  }
  const isCargo = parsed.cargo === true || /^(00|01)-CRG$/.test(String(parsed.cargo));
  const verifiedName = typeof parsed.model_name === "string" ? parsed.model_name.trim() : "";
  const name = verifiedName || `${model.name}${isCargo ? "-CRG" : ""}`;
  return { modelSlug, model: name, isCargo: isCargo || /-(CRG|TRK)$/.test(name) };
}

/** Models using the modern settings protocol: drive-unit authentication, modern destinations and torque levels. */
export function isModernModel(slug: string | undefined): boolean {
  return slug === "ep600" || slug === "ep801" || slug === "ep500";
}

export function friendlyMotorName(identity: DeviceIdentity | null): string {
  if (!identity) return "Shimano STEPS";
  if (identity.modelSlug === "ep800-crg") {
    return identity.isCargo ? "Shimano EP8 Cargo (DU-EP800-CRG)" : "Shimano EP8 (DU-EP800)";
  }
  return `Shimano ${identity.model}`;
}

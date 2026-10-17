import type { DeviceIdentity, MotorModel } from "../types.ts";

const MODELS: Record<number, { slug: MotorModel; name: string }> = {
  0x08: { slug: "e6000", name: "E6000 family" },
  0x12: { slug: "e8000", name: "E8000 family" },
  0x20: { slug: "e6100", name: "E6100 family" },
  0x21: { slug: "e7000", name: "DU-E7000" },
  0x23: { slug: "ep800-crg", name: "DU-EP800" },
  0x30: { slug: "ep600", name: "DU-EP600" },
  0x35: { slug: "ep801", name: "DU-EP801" },
  0x37: { slug: "ep500", name: "DU-EP500" },
  0x38: { slug: "e5100", name: "DU-E5100" },
};

/** Series selects the schema; verified backend metadata distinguishes shared-series variants. */
export function detectMotorIdentity(parsed: { model_series?: number; model_unit?: number; cargo?: unknown; model_name?: unknown; settings_model?: unknown }) {
  const model = parsed.model_series === undefined ? undefined : MODELS[parsed.model_series];
  if (!model || (parsed.model_unit !== undefined && parsed.model_unit !== 0)) {
    throw new Error("Settings are not supported for this drive-unit identity.");
  }
  if (parsed.settings_model !== undefined && parsed.settings_model !== model.slug) {
    throw new Error("Drive-unit identity and settings schema do not match.");
  }
  const isCargo = parsed.cargo === true || /^(00|01)-CRG$/.test(String(parsed.cargo));
  const verifiedName = typeof parsed.model_name === "string" ? parsed.model_name.trim() : "";
  const name = verifiedName || `${model.name}${isCargo ? "-CRG" : ""}`;
  return { modelSlug: model.slug, model: name, isCargo: isCargo || /-(CRG|TRK)$/.test(name) };
}

export function friendlyMotorName(identity: DeviceIdentity | null): string {
  if (!identity) return "Shimano STEPS";
  if (identity.modelSlug === "ep800-crg") {
    return identity.isCargo ? "Shimano EP8 Cargo (DU-EP800-CRG)" : "Shimano EP8 (DU-EP800)";
  }
  return `Shimano ${identity.model}`;
}

/**
 * Human-readable component types derived from Shimano model-name prefixes,
 * e.g. "DU-EP800-CRG" → "Motor". Keep in sync with ComponentType.kt in the Android app.
 */
const SUBTYPES: [prefix: string, type: string][] = [
  ["EW-WU", "Wireless unit"],
  ["EW-SW", "Shifter"],
  ["SM-EWW", "Wireless unit"],
  ["SM-EW", "Junction"],
  ["SM-BCR", "Charger"],
  ["SM-BMR", "Battery mount"],
  ["SM-PCE", "PC interface"],
];

const TYPES: Record<string, string> = {
  DU: "Motor",
  RD: "Rear derailleur",
  FD: "Front derailleur",
  SW: "Shifter",
  ST: "Shifter",
  SC: "Display",
  BT: "Battery",
  BM: "Battery mount",
  EW: "Junction",
  MU: "Hub motor unit",
  FH: "Freehub",
  FC: "Power meter",
};

/** The component type for a catalogued model name, or null when the prefix is unknown. */
export function componentType(model: string | null | undefined): string | null {
  if (!model) return null;
  const name = model.toUpperCase();
  const sub = SUBTYPES.find(([prefix]) => name.startsWith(prefix));
  if (sub) return sub[1];
  return TYPES[name.split("-")[0]] ?? null;
}

/** "Motor · DU-EP800-CRG"; candidates that disagree on the type keep only the names. */
export function componentLabel(models: readonly string[]): string {
  const names = models.join(" / ");
  const types = new Set(models.map(componentType));
  const [type] = types;
  return types.size === 1 && type ? `${type} · ${names}` : names;
}

/**
 * settingFormat.ts - Operator-facing names and values for settings.
 *
 * Every place that shows a setting (rows, review, backups, overview) uses
 * these helpers so a value reads the same way everywhere.
 */

import type { EnrichedSetting } from "../types.ts";
import { settingAcceptsValue } from "./schema.ts";

type Value = number | string | null | undefined;

const SCALED_UNIT = /^(0\.\d+)\s*(.+)$/;

export function settingLabel(schema: EnrichedSetting[], name: string): string {
  const spec = schema.find((setting) => setting.name === name);
  if (spec && spec.label !== spec.name) return spec.label;
  return name
    .split(/[.-]/)
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(" ");
}

export function formatValue(spec: EnrichedSetting | undefined, value: Value): string {
  if (value === null || value === undefined || value === "") return "—";
  const numeric = typeof value === "number" ? value : Number(value);
  const isNumeric = Number.isFinite(numeric) && String(value).trim() !== "";

  if (spec?.choiceLabels && isNumeric && spec.choiceLabels[numeric] !== undefined) {
    return spec.choiceLabels[numeric];
  }
  if (!isNumeric) return String(value).trim();

  const unit = spec?.displayUnit ?? "";
  if (spec?.name === "start-mode") return numeric === 0 ? "Off" : `Gear ${numeric}`;
  if (unit === "lvl") return `Level ${numeric}`;
  if (unit === "deg" || unit === "degrees") return `${numeric}°`;
  if (unit === "T" || unit === "teeth") return `${numeric}T`;
  const scaled = unit.match(SCALED_UNIT);
  if (scaled) {
    const factor = Number(scaled[1]);
    const decimals = scaled[1].length - 2;
    const suffix = scaled[2] === "percent" ? "%" : scaled[2];
    const separator = suffix === "%" ? "" : " ";
    return `${(numeric * factor).toFixed(decimals)}${separator}${suffix}`;
  }
  return unit ? `${numeric} ${unit}` : String(numeric);
}

export function formatSetting(schema: EnrichedSetting[], name: string, value: Value): string {
  return formatValue(schema.find((setting) => setting.name === name), value);
}

/**
 * Changes that alter legal or factory limits need an explicit acknowledgment.
 * The core's oem flag decides, since annotations file oem settings such as
 * tire circumference under geometry or drivetrain.
 */
export function isRegulatedSetting(spec: EnrichedSetting | undefined, name: string): boolean {
  return name === "destination" || spec?.oem === true || spec?.section === "oem";
}

/**
 * "Label (value)" for each value cranklink could not write back, as a restore
 * would: outside the setting's range, step or choices, or unset where the
 * setting cannot be unset. The core also checks region limits when writing.
 */
export function unwritableValues(schema: EnrichedSetting[], values: ReadonlyArray<readonly [string, Value]>): string[] {
  return values.flatMap(([name, value]) => {
    const spec = schema.find((setting) => setting.name === name);
    // Without a schema entry or a value read from the bike there is nothing to judge.
    if (!spec || value === undefined) return [];
    const writable = value === null || value === ""
      ? spec.nullable
      : (spec.value_type !== "number" && spec.value_type !== "enum") || settingAcceptsValue(spec, Number(value));
    return writable ? [] : [`${settingLabel(schema, name)} (${formatValue(spec, value)})`];
  });
}

/** The review's warning for current values in `unwritableValues`. */
export function unwritableNotice(values: string[]): string {
  return `cranklink cannot write back the current ${listText(values)}, so Backup & Restore cannot undo ${values.length === 1 ? "that change" : "those changes"}.`;
}

/** "A", "A and B", "A, B and C". */
export function listText(items: string[]): string {
  return items.length < 2 ? items.join("") : `${items.slice(0, -1).join(", ")} and ${items.at(-1)}`;
}

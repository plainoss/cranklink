/**
 * SettingRow.tsx - Schema-driven building blocks for every editable setting.
 *
 * A row owns the setting's name, its live and staged values, and an undo
 * control; the control inside it only edits the value. Search filtering
 * happens here so sections can hide themselves when no row matches.
 */

import { createContext, useContext, useId, type ReactNode } from "react";
import { RotateCcw, AlertTriangle } from "lucide-react";
import type { EnrichedSetting, InstrumentState } from "../../types.ts";
import type { SessionController } from "../../core/sessionController.ts";
import { formatValue, settingLabel } from "../../domain/settingFormat.ts";
import { settingAcceptsValue } from "../../domain/schema.ts";
import { Slider } from "../ui/Slider.tsx";
import { Segmented, type SegmentedOption } from "../ui/Segmented.tsx";

interface SettingsContextValue {
  state: InstrumentState;
  controller: SessionController;
  query: string;
}

const SettingsContext = createContext<SettingsContextValue | null>(null);

export function SettingsProvider({ value, children }: { value: SettingsContextValue; children: ReactNode }) {
  return <SettingsContext.Provider value={value}>{children}</SettingsContext.Provider>;
}

function useSettings(): SettingsContextValue {
  const context = useContext(SettingsContext);
  if (!context) throw new Error("Setting controls must render inside SettingsProvider.");
  return context;
}

export interface SettingAccess {
  name: string;
  spec: EnrichedSetting | undefined;
  label: string;
  live: number | string | null | undefined;
  value: number | string | null | undefined;
  numeric: number;
  isStaged: boolean;
  unavailable: string | undefined;
  format: (value: number | string | null | undefined) => string;
  stage: (value: number | string) => void;
  revert: () => void;
}

export function useSetting(name: string): SettingAccess {
  const { state, controller } = useSettings();
  const spec = state.schema.find((setting) => setting.name === name);
  const live = state.liveValues[name]?.value;
  const staged = state.stagedValues[name];
  const value = staged ?? live;
  return {
    name,
    spec,
    label: settingLabel(state.schema, name),
    live,
    value,
    numeric: Number(value),
    isStaged: staged !== undefined,
    unavailable: state.unavailableValues[name],
    format: (next) => formatValue(spec, next),
    stage: (next) => controller.stageParameter(name, next),
    revert: () => controller.revertParameter(name),
  };
}

export function useSearchQuery(): string {
  return useSettings().query;
}

export function matchesQuery(query: string, ...texts: Array<string | undefined>): boolean {
  const needle = query.trim().toLowerCase();
  if (!needle) return true;
  return texts.some((text) => text?.toLowerCase().includes(needle));
}

interface SettingRowProps {
  name: string;
  /** Overrides the schema label where the curated wording is clearer. */
  title?: string;
  hint?: string;
  /** Control placed beside the title instead of below it. */
  inlineControl?: ReactNode;
  /** Show the current value large beside the title (sliders, steppers). */
  showValue?: boolean;
  children?: ReactNode;
  footer?: ReactNode;
  "data-testid"?: string;
}

export function SettingRow({
  name,
  title,
  hint,
  inlineControl,
  showValue = false,
  children,
  footer,
  "data-testid": testId,
}: SettingRowProps) {
  const setting = useSetting(name);
  const query = useSearchQuery();
  const titleId = useId();
  const heading = title ?? setting.label;
  const description = hint ?? setting.spec?.description;
  // Older drive units lack some settings; their schema decides what is shown.
  if (!setting.spec) return null;
  if (!matchesQuery(query, heading, name, description, setting.label)) return null;

  return (
    <div
      className="px-4 py-4 sm:px-5 flex flex-col gap-3"
      data-setting-row={name}
      data-testid={testId ?? `param-row-${name}`}
      role="group"
      aria-labelledby={titleId}
    >
      <div className="flex flex-wrap items-start justify-between gap-x-4 gap-y-2">
        <div className="min-w-0 flex-1">
          <h3 id={titleId} className="text-sm font-semibold text-stone-900">
            {heading}
          </h3>
          {description && <p className="text-xs text-stone-500 mt-0.5">{description}</p>}
          <SettingStatus setting={setting} />
        </div>
        {showValue && (
          <output
            className="shrink-0 text-sm font-mono font-bold text-stone-900 bg-white px-3 py-1.5 rounded-lg border border-stone-200 tabular-nums"
            aria-hidden="true"
          >
            {setting.format(setting.value)}
          </output>
        )}
        {inlineControl && <div className="shrink-0 max-w-full">{inlineControl}</div>}
      </div>
      {children}
      {footer}
    </div>
  );
}

function SettingStatus({ setting }: { setting: SettingAccess }) {
  if (setting.unavailable) {
    return (
      <p className="mt-1 flex items-center gap-1.5 text-xs text-amber-800">
        <AlertTriangle size={12} aria-hidden="true" />
        <span>Not available on this unit: {setting.unavailable}</span>
      </p>
    );
  }
  return (
    <p className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs font-mono text-stone-500">
      <span>
        Live: <strong className="font-semibold text-stone-800">{setting.format(setting.live)}</strong>
      </span>
      {setting.isStaged && (
        <>
          <span className="rounded-full bg-amber-100 px-2 py-0.5 font-semibold text-amber-900">
            Staged: {setting.format(setting.value)}
          </span>
          <button
            type="button"
            onClick={setting.revert}
            className="inline-flex min-h-[28px] items-center gap-1 rounded-md px-1.5 font-sans font-medium text-stone-600 underline-offset-2 hover:text-stone-900 hover:underline"
            aria-label={`Undo change to ${setting.label}`}
          >
            <RotateCcw size={12} aria-hidden="true" />
            Undo
          </button>
        </>
      )}
    </p>
  );
}

interface SettingSliderProps {
  name: string;
  min?: number;
  max?: number;
  step?: number;
  /** Discrete values the device accepts; the slider then steps through them. */
  values?: number[];
  "data-testid"?: string;
}

export function SettingSlider({ name, min, max, step, values, "data-testid": testId }: SettingSliderProps) {
  const setting = useSetting(name);
  if (values?.length) {
    const nearest = values.reduce((best, value, index) =>
      Math.abs(value - setting.numeric) < Math.abs(values[best] - setting.numeric) ? index : best, 0);
    return (
      <Slider
        label={setting.label}
        value={nearest}
        min={0}
        max={values.length - 1}
        step={1}
        valueText={setting.format(values[nearest])}
        onChange={(index) => setting.stage(values[index])}
        disabled={Boolean(setting.unavailable)}
        data-testid={testId}
      />
    );
  }
  const lower = min ?? setting.spec?.userMin ?? 0;
  const upper = max ?? setting.spec?.userMax ?? 100;
  const value = Number.isFinite(setting.numeric) ? setting.numeric : lower;
  return (
    <Slider
      label={setting.label}
      value={value}
      min={lower}
      max={upper}
      step={step ?? setting.spec?.userStep ?? 1}
      valueText={setting.format(value)}
      onChange={setting.stage}
      disabled={Boolean(setting.unavailable)}
      data-testid={testId}
    />
  );
}

interface SettingChoiceProps {
  name: string;
  options?: SegmentedOption<number>[];
}

/** Options from the curated list or the schema's labelled choices, limited to values this unit accepts. */
function choiceOptions(setting: SettingAccess, options?: SegmentedOption<number>[]): SegmentedOption<number>[] {
  const spec = setting.spec;
  const candidates = options
    ?? (spec?.choices.length ? spec.choices : Object.keys(spec?.choiceLabels ?? {}).map(Number))
      .map((value) => ({ value, label: setting.format(value) }));
  return spec ? candidates.filter((option) => settingAcceptsValue(spec, option.value)) : candidates;
}

export function SettingSegmented({ name, options }: SettingChoiceProps) {
  const setting = useSetting(name);
  return (
    <Segmented
      label={setting.label}
      value={setting.numeric}
      options={choiceOptions(setting, options)}
      onChange={setting.stage}
    />
  );
}

export function SettingSelect({ name, options }: SettingChoiceProps) {
  const setting = useSetting(name);
  return (
    <select
      aria-label={setting.label}
      value={Number.isFinite(setting.numeric) ? setting.numeric : ""}
      onChange={(event) => setting.stage(Number(event.target.value))}
      disabled={Boolean(setting.unavailable)}
      className="w-full text-sm p-2.5 rounded-xl border border-stone-300 bg-white text-stone-900 min-h-[44px] cursor-pointer focus:outline-none focus:ring-2 focus:ring-amber-300"
    >
      {choiceOptions(setting, options).map((option) => (
        <option key={option.value} value={option.value}>
          {option.label}
        </option>
      ))}
    </select>
  );
}

interface SectionProps {
  title: string;
  description?: string;
  icon?: ReactNode;
  notice?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  "data-testid"?: string;
}

/** A card of setting rows; it hides itself when search leaves no rows. */
export function SettingsSection({ title, description, icon, notice, actions, children, "data-testid": testId }: SectionProps) {
  const headingId = useId();
  return (
    <section
      aria-labelledby={headingId}
      data-testid={testId}
      className="bg-white border border-stone-200 rounded-3xl shadow-xs overflow-hidden [&:not(:has([data-setting-row]))]:hidden"
    >
      <header className="flex flex-wrap items-start justify-between gap-3 px-4 pt-4 pb-3 sm:px-5 border-b border-stone-200">
        <div className="min-w-0">
          <h2 id={headingId} className="text-base font-bold text-stone-900 flex items-center gap-2">
            {icon}
            <span>{title}</span>
          </h2>
          {description && <p className="text-xs text-stone-500 mt-0.5">{description}</p>}
        </div>
        {actions}
      </header>
      {notice}
      <div className="flex flex-col divide-y divide-stone-200">{children}</div>
    </section>
  );
}

export function SectionNotice({ children }: { children: ReactNode }) {
  return (
    <div className="mx-4 mt-3 sm:mx-5 flex items-start gap-2.5 rounded-xl border border-amber-200 bg-amber-50 p-3 text-xs leading-relaxed text-amber-900">
      <AlertTriangle size={16} className="mt-0.5 shrink-0 text-amber-700" aria-hidden="true" />
      <div>{children}</div>
    </div>
  );
}

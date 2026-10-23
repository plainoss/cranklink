/**
 * SettingRow.tsx - Schema-driven building blocks for every editable setting.
 *
 * A row owns the setting's name, its live and staged values, and an undo
 * control; the control inside it only edits the value. Search filtering
 * happens here so sections can hide themselves when no row matches.
 */

import { createContext, useContext, useId, type ReactNode } from "react";
import { RotateCcw, AlertTriangle } from "lucide-react";
import { Alert, Button, Card, Chip, Slider, ToggleButton, ToggleButtonGroup } from "@heroui/react";
import { OptionSelect } from "../ui/OptionSelect.tsx";
import type { EnrichedSetting, InstrumentState } from "../../types.ts";
import type { SessionController } from "../../core/sessionController.ts";
import { formatValue, settingLabel } from "../../domain/settingFormat.ts";
import { settingAcceptsValue } from "../../domain/schema.ts";

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
  isReadOnly: boolean;
  isDisabled: boolean;
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
  const isReadOnly = Boolean(spec?.read_only);
  const isDisabled = isReadOnly || Boolean(state.unavailableValues[name]) || state.phase !== "ready" || state.linkLost;
  return {
    name,
    spec,
    label: settingLabel(state.schema, name),
    live,
    value,
    numeric: Number(value),
    isStaged: staged !== undefined,
    isReadOnly,
    isDisabled,
    unavailable: state.unavailableValues[name],
    format: (next) => formatValue(spec, next),
    stage: (next) => {
      if (isDisabled) return;
      try {
        controller.stageParameter(name, next);
      } catch {
        // The controller records validation failures for the shared banner.
      }
    },
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
        <div className="min-w-[12rem] flex-1">
          <h3 id={titleId} className="text-sm font-semibold text-stone-900">
            {heading}
          </h3>
          {description && <p className="text-xs text-stone-500 mt-0.5">{description}</p>}
          <SettingStatus setting={setting} />
        </div>
        {showValue && (
          <Chip variant="secondary" size="lg" aria-hidden="true" className="shrink-0 font-mono font-bold tabular-nums">
            {setting.format(setting.value)}
          </Chip>
        )}
        {inlineControl && !setting.isReadOnly && <div className="shrink-0 max-w-full">{inlineControl}</div>}
      </div>
      {!setting.isReadOnly && children}
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
      {setting.isReadOnly && <Chip size="sm" variant="soft">Read only</Chip>}
      {setting.isStaged && (
        <>
          <Chip color="warning" variant="soft" size="sm" className="font-semibold">
            Staged: {setting.format(setting.value)}
          </Chip>
          <Button variant="ghost" size="sm" onPress={setting.revert} aria-label={`Undo change to ${setting.label}`} className="font-sans">
            <RotateCcw size={12} aria-hidden="true" />
            Undo
          </Button>
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
  "data-testid"?: string;
}

/** Intl unit formatting where the platform knows the unit, so the thumb announces it. */
const INTL_UNITS: Record<string, string> = {
  "km/h": "kilometer-per-hour",
  mm: "millimeter",
  km: "kilometer",
  min: "minute",
  minutes: "minute",
  deg: "degree",
  degrees: "degree",
};

export function SettingSlider({ name, min, max, step, "data-testid": testId }: SettingSliderProps) {
  const setting = useSetting(name);
  const noteId = useId();
  const lower = min ?? setting.spec?.userMin ?? 0;
  const upper = max ?? setting.spec?.userMax ?? 100;
  const value = Number.isFinite(setting.numeric) ? Math.min(upper, Math.max(lower, setting.numeric)) : lower;
  // The thumb cannot show a value past either end, e.g. a speed set by other tools.
  const outside = Number.isFinite(setting.numeric) && value !== setting.numeric;
  const unit = setting.spec?.displayUnit ?? "";
  const intlUnit = INTL_UNITS[unit];
  const range = `${setting.format(lower)} to ${setting.format(upper)}`;
  return (
    <>
      <Slider
        aria-label={intlUnit || !unit ? setting.label : `${setting.label} (${unit})`}
        value={value}
        minValue={lower}
        maxValue={upper}
        step={step ?? setting.spec?.userStep ?? 1}
        formatOptions={intlUnit ? { style: "unit", unit: intlUnit } : undefined}
        onChange={(next) => typeof next === "number" && setting.stage(next)}
        isDisabled={setting.isDisabled}
        className="w-full"
        aria-describedby={outside ? noteId : undefined}
        data-testid={testId}
      >
        <Slider.Track>
          <Slider.Fill />
          <Slider.Thumb />
        </Slider.Track>
      </Slider>
      {outside && (
        <p id={noteId} className="flex items-start gap-1.5 text-xs text-amber-800" data-testid={`slider-outside-${name}`}>
          <AlertTriangle size={12} className="mt-0.5 shrink-0" aria-hidden="true" />
          <span>
            {`The current value, ${setting.format(setting.numeric)}, is outside `}
            {min === undefined && max === undefined ? `the range cranklink can write (${range})` : `this slider’s range (${range})`}
            {`, so the slider stops at ${setting.format(value)}.`}
          </span>
        </p>
      )}
    </>
  );
}

export interface ChoiceOption {
  value: number;
  label: string;
}

interface SettingChoiceProps {
  name: string;
  options?: ChoiceOption[];
  /** Discrete values the device accepts, e.g. modern torque levels. */
  values?: number[];
  "data-testid"?: string;
}

/** Curated options, explicit values, or the schema's labelled choices, limited to values this unit accepts. */
function choiceOptions(setting: SettingAccess, options?: ChoiceOption[], values?: number[]): ChoiceOption[] {
  const spec = setting.spec;
  const candidates = options
    ?? (values ?? (spec?.choices.length ? spec.choices : Object.keys(spec?.choiceLabels ?? {}).map(Number)))
      .map((value) => ({ value, label: setting.format(value) }));
  return spec && !values ? candidates.filter((option) => settingAcceptsValue(spec, option.value)) : candidates;
}

export function SettingSegmented({ name, options, values, "data-testid": testId }: SettingChoiceProps) {
  const setting = useSetting(name);
  return (
    <ToggleButtonGroup
      aria-label={setting.label}
      selectionMode="single"
      disallowEmptySelection
      selectedKeys={Number.isFinite(setting.numeric) ? [String(setting.numeric)] : []}
      onSelectionChange={(keys) => {
        const [key] = [...keys];
        if (key !== undefined) setting.stage(Number(key));
      }}
      isDisabled={setting.isDisabled}
      size="sm"
      className="flex-wrap"
      data-testid={testId}
    >
      {choiceOptions(setting, options, values).map((option) => (
        <ToggleButton key={option.value} id={String(option.value)}>
          {option.label}
        </ToggleButton>
      ))}
    </ToggleButtonGroup>
  );
}

export function SettingSelect({ name, options }: SettingChoiceProps) {
  const setting = useSetting(name);
  return (
    <OptionSelect
      aria-label={setting.label}
      value={Number.isFinite(setting.numeric) ? String(setting.numeric) : null}
      options={choiceOptions(setting, options).map((option) => ({ id: String(option.value), label: option.label }))}
      onChange={(id) => setting.stage(Number(id))}
      isDisabled={setting.isDisabled}
    />
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
    <Card
      aria-labelledby={headingId}
      data-testid={testId}
      className="p-0 gap-0 overflow-hidden [&:not(:has([data-setting-row]))]:hidden"
    >
      <Card.Header className="flex flex-row flex-wrap items-start justify-between gap-3 px-4 pt-4 pb-3 sm:px-5 border-b border-separator">
        <div className="min-w-0">
          <Card.Title id={headingId} className="text-base font-bold flex items-center gap-2">
            {icon}
            <span>{title}</span>
          </Card.Title>
          {description && <Card.Description className="text-xs mt-0.5">{description}</Card.Description>}
        </div>
        {actions}
      </Card.Header>
      {notice}
      <Card.Content className="flex flex-col divide-y divide-separator">{children}</Card.Content>
    </Card>
  );
}

export function SectionNotice({ children }: { children: ReactNode }) {
  return (
    <Alert status="warning" className="mx-4 mt-3 sm:mx-5 w-auto">
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Description className="text-xs leading-relaxed">{children}</Alert.Description>
      </Alert.Content>
    </Alert>
  );
}

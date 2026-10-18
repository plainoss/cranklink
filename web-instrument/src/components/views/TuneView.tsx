/**
 * TuneView.tsx - Curated setting groups plus the complete settings list.
 *
 * Rows come from SettingRow so every setting shows its name once, its live
 * value, any staged value, and an undo control in the same place.
 */

import { useState, type ReactNode } from "react";
import { Cog, Compass, ShieldCheck, Sliders, Wrench, Minus, Plus } from "lucide-react";
import type { InstrumentState } from "../../types.ts";
import type { SessionController } from "../../core/sessionController.ts";
import { TIRE_PRESETS, clampSettingValue, modernTorqueLevels, settingAcceptsValue } from "../../domain/schema.ts";
import { Button } from "../ui/Button.tsx";
import { Tabs } from "../ui/Tabs.tsx";
import {
  SettingsProvider,
  SettingsSection,
  SectionNotice,
  SettingRow,
  SettingSlider,
  SettingSegmented,
  SettingSelect,
  useSetting,
} from "../settings/SettingRow.tsx";
import { AllSettingsList, allSettingNames } from "../settings/AllSettingsList.tsx";

export type TuneCategory = "all" | "geometry" | "profiles" | "drivetrain" | "maintenance" | "oem";

/** Settings with a curated control; search lists the rest separately. */
const CURATED = new Set([
  "destination", "speed-limit", "tire-circumference", "lamp", "mounting-angle",
  "display-speed-adjustment", "lighting-time", "walk-assist",
  "assist-pattern", "profile1.display", "profile2.display",
  ...["profile1", "profile2"].flatMap((p) =>
    ["boost", "trail", "eco"].flatMap((mode) => ["torque", "character", "start"].map((field) => `${p}.${mode}.${field}`))),
  "shifting-system", "rear-max-gear", "front-teeth", "rear-teeth", "auto-gear-cadence", "start-mode",
  "maintenance-alert", "maintenance-distance",
  "torque-minimum", "torque-maximum",
]);

interface TuneViewProps {
  state: InstrumentState;
  controller: SessionController;
  category: TuneCategory;
  query: string;
}

export function TuneView({ state, controller, category, query }: TuneViewProps) {
  const searching = query.trim().length > 0;
  const show = (group: TuneCategory) => searching || category === group;

  return (
    <SettingsProvider value={{ state, controller, query }}>
      <div className="w-full flex flex-col gap-6">
        {category === "all" && !searching ? (
          <AllSettingsList
            state={state}
            title="All settings"
            description="Every setting reported by this drive unit, including ones without a dedicated control."
            data-testid="all-settings"
          />
        ) : (
          <>
            {show("geometry") && hasSection(state, "geometry") && <GeometrySection state={state} />}
            {show("profiles") && hasSection(state, "profiles") && <ProfilesSection state={state} />}
            {show("drivetrain") && hasSection(state, "drivetrain") && <DrivetrainSection />}
            {show("maintenance") && hasSection(state, "maintenance") && <MaintenanceSection />}
            {show("oem") && hasSection(state, "oem") && <OemSection />}
            {searching && (
              <AllSettingsList
                state={state}
                names={allSettingNames(state).filter((name) => !CURATED.has(name))}
                title="More matching settings"
                data-testid="search-more-settings"
              />
            )}
            {searching && <NoSearchResults query={query} />}
          </>
        )}
      </div>
    </SettingsProvider>
  );
}

function hasSection(state: InstrumentState, section: string): boolean {
  return state.schema.some((setting) => setting.section === section);
}

/** Shown by CSS only when no section or list rendered a row. */
function NoSearchResults({ query }: { query: string }) {
  return (
    <p className="hidden [div:not(:has([data-setting-row]))>&]:block rounded-2xl border border-dashed border-stone-300 bg-white/60 p-6 text-center text-sm text-stone-600">
      No settings match “{query.trim()}”.
    </p>
  );
}

function GeometrySection({ state }: { state: InstrumentState }) {
  return (
    <SettingsSection
      title="Bike & Geometry"
      description="Region and assist speed, wheel size, lighting, and installation."
      icon={<Compass size={18} className="text-stone-600" aria-hidden="true" />}
      data-testid="section-geometry"
    >
      <SettingRow
        name="destination"
        footer={
          <p className="text-xs text-amber-900 bg-amber-50 border border-amber-200 rounded-xl px-3 py-2">
            Only choose the region that applies where you ride. Changing it can make the bike non-compliant with local e-bike law.
          </p>
        }
      >
        <SettingSelect name="destination" />
      </SettingRow>

      <SettingRow name="speed-limit" showValue>
        <SettingSlider name="speed-limit" />
        <p className="text-xs text-stone-500" data-testid="speed-destination-hint">
          If the speed needs a different region, that region change is staged too. Both appear in the review before saving.
        </p>
        <SpeedPresets state={state} />
      </SettingRow>

      <SettingRow
        name="tire-circumference"
        inlineControl={<TireStepper />}
      >
        <TirePresets />
      </SettingRow>

      <SettingRow name="lamp" inlineControl={<SettingSegmented name="lamp" />} />
      <SettingRow name="walk-assist" inlineControl={<SettingSegmented name="walk-assist" />} />

      <SettingRow name="mounting-angle" showValue>
        <SettingSlider name="mounting-angle" />
      </SettingRow>
      <SettingRow name="display-speed-adjustment" showValue>
        <SettingSlider name="display-speed-adjustment" />
      </SettingRow>
      <SettingRow name="lighting-time" showValue>
        <SettingSlider name="lighting-time" step={10} />
      </SettingRow>
    </SettingsSection>
  );
}

function SpeedPresets({ state }: { state: InstrumentState }) {
  const speed = useSetting("speed-limit");
  const spec = speed.spec;
  const presets = spec ? [20, 25, 32, 45].filter((value) => settingAcceptsValue(spec, value)) : [];
  if (presets.length === 0) return null;
  return (
    <div className="flex items-center gap-2 flex-wrap" role="group" aria-label="Speed presets">
      <span className="text-xs text-stone-500">Presets:</span>
      {presets.map((value) => (
        <PresetChip key={value} selected={speed.numeric === value} onClick={() => speed.stage(value)}>
          {value} km/h
        </PresetChip>
      ))}
    </div>
  );
}

function TireStepper() {
  const tire = useSetting("tire-circumference");
  const spec = tire.spec;
  if (!spec) return null;
  const value = Number.isFinite(tire.numeric) ? tire.numeric : spec.userMin;
  // Step by about 10 mm, rounded up to the unit's own step.
  const nudge = Math.ceil(10 / spec.userStep) * spec.userStep;
  const step = (delta: number) => tire.stage(clampSettingValue(spec, value + delta));
  return (
    <div className="flex items-center gap-2">
      <Button
        variant="outline"
        size="sm"
        onClick={() => step(-nudge)}
        disabled={value <= spec.userMin}
        data-testid="stepper-minus-tire-circumference"
        aria-label={`Decrease tire circumference by ${nudge} mm`}
        className="w-11 h-11 p-0 shrink-0 min-w-[44px] min-h-[44px]"
      >
        <Minus size={16} aria-hidden="true" />
      </Button>
      <output className="min-w-[80px] text-center font-mono font-bold text-stone-900 text-sm tabular-nums" aria-live="polite">
        {tire.format(tire.value)}
      </output>
      <Button
        variant="outline"
        size="sm"
        onClick={() => step(nudge)}
        disabled={value >= spec.userMax}
        data-testid="stepper-plus-tire-circumference"
        aria-label={`Increase tire circumference by ${nudge} mm`}
        className="w-11 h-11 p-0 shrink-0 min-w-[44px] min-h-[44px]"
      >
        <Plus size={16} aria-hidden="true" />
      </Button>
    </div>
  );
}

function TirePresets() {
  const tire = useSetting("tire-circumference");
  const spec = tire.spec;
  const presets = spec ? TIRE_PRESETS.filter((preset) => settingAcceptsValue(spec, Number(preset.value))) : [];
  const selected = presets.find((preset) => Number(preset.value) === tire.numeric);
  if (presets.length === 0) return null;
  return (
    <div className="flex flex-col gap-2">
      {/* A compact picker on phones; chips where there is room. */}
      <label className="sm:hidden text-xs text-stone-500">
        Common sizes
        <select
          value={selected ? String(selected.value) : ""}
          onChange={(event) => event.target.value && tire.stage(Number(event.target.value))}
          className="mt-1 w-full text-sm p-2.5 rounded-xl border border-stone-300 bg-white text-stone-900 min-h-[44px]"
        >
          <option value="">Choose a common size…</option>
          {presets.map((preset) => (
            <option key={preset.label} value={String(preset.value)}>
              {preset.label}
            </option>
          ))}
        </select>
      </label>
      <div className="hidden sm:flex items-center gap-1.5 flex-wrap" role="group" aria-label="Common tire sizes">
        <span className="text-xs text-stone-500 mr-1">Common sizes:</span>
        {presets.map((preset) => (
          <PresetChip key={preset.label} selected={selected === preset} onClick={() => tire.stage(Number(preset.value))}>
            {preset.label}
          </PresetChip>
        ))}
      </div>
      <p className="text-[11px] text-stone-500">Chart sizes are estimates; a measured roll-out is most accurate.</p>
    </div>
  );
}

function PresetChip({ selected, onClick, children }: { selected: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={selected}
      className={`min-h-[32px] px-2.5 text-xs font-mono rounded-lg border transition-colors cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-amber-400 ${
        selected ? "bg-stone-900 text-white border-stone-900" : "bg-white text-stone-700 border-stone-200 hover:bg-stone-100"
      }`}
    >
      {children}
    </button>
  );
}

const MODES = [
  { id: "boost", title: "Boost", dot: "bg-amber-500" },
  { id: "trail", title: "Trail", dot: "bg-sky-600" },
  { id: "eco", title: "Eco", dot: "bg-emerald-600" },
] as const;

function ProfilesSection({ state }: { state: InstrumentState }) {
  const [profile, setProfile] = useState<"1" | "2">("1");
  const p = `profile${profile}`;
  const bound = (name: string, fallback: number) => {
    const value = Number(state.liveValues[name]?.value);
    return Number.isFinite(value) ? value : fallback;
  };
  const torqueLevels = (mode: "eco" | "trail" | "boost") =>
    modernTorqueLevels(state.identity?.modelSlug, mode, bound("torque-minimum", 20), bound("torque-maximum", 85));
  return (
    <SettingsSection
      title="Assist Profiles"
      description="Peak torque, assist character, and start response for each mode."
      icon={<Sliders size={18} className="text-stone-600" aria-hidden="true" />}
      data-testid="section-profiles"
      actions={
        <div className="w-full sm:w-auto">
          <Tabs
            value={profile}
            onValueChange={(next) => setProfile(next as "1" | "2")}
            items={[
              { id: "1", label: "Profile 1" },
              { id: "2", label: "Profile 2" },
            ]}
          />
        </div>
      }
    >
      {MODES.map((mode) => (
        <div key={mode.id} className="flex flex-col divide-y divide-stone-100 [&:not(:has([data-setting-row]))]:hidden">
          <h3 className="flex items-center gap-2 px-4 pt-4 sm:px-5 text-sm font-bold text-stone-900 border-0">
            <span className={`w-2.5 h-2.5 rounded-full ${mode.dot}`} aria-hidden="true" />
            {mode.title} mode
          </h3>
          <SettingRow name={`${p}.${mode.id}.torque`} title={`${mode.title} peak torque`} showValue>
            <SettingSlider name={`${p}.${mode.id}.torque`} values={torqueLevels(mode.id)} data-testid={`slider-${mode.id}-torque`} />
          </SettingRow>
          <SettingRow name={`${p}.${mode.id}.character`} title={`${mode.title} assist character`} showValue>
            <SettingSlider name={`${p}.${mode.id}.character`} />
          </SettingRow>
          <SettingRow name={`${p}.${mode.id}.start`} title={`${mode.title} start response`} showValue>
            <SettingSlider name={`${p}.${mode.id}.start`} />
          </SettingRow>
        </div>
      ))}
      <SettingRow
        name={`${p}.display`}
        title="Mode names on the display"
        inlineControl={<SettingSegmented name={`${p}.display`} />}
      />
      <SettingRow name="assist-pattern" inlineControl={<SettingSegmented name="assist-pattern" />} />
    </SettingsSection>
  );
}

function DrivetrainSection() {
  return (
    <SettingsSection
      title="Drivetrain & Di2"
      description="Shifting system, gearing, and Di2 automatic shifting."
      icon={<Cog size={18} className="text-stone-600" aria-hidden="true" />}
      data-testid="section-drivetrain"
    >
      <SettingRow name="shifting-system">
        <SettingSelect name="shifting-system" />
      </SettingRow>
      <SettingRow name="rear-max-gear">
        <SettingSegmented name="rear-max-gear" />
      </SettingRow>
      <SettingRow name="front-teeth" showValue>
        <SettingSlider name="front-teeth" />
      </SettingRow>
      <SettingRow name="rear-teeth" showValue>
        <SettingSlider name="rear-teeth" />
      </SettingRow>
      <SettingRow name="auto-gear-cadence" showValue>
        <SettingSlider name="auto-gear-cadence" />
      </SettingRow>
      <SettingRow name="start-mode" showValue>
        <SettingSlider name="start-mode" />
      </SettingRow>
    </SettingsSection>
  );
}

function MaintenanceSection() {
  return (
    <SettingsSection
      title="Service & Maintenance"
      description="Service reminders shown on the cycle computer."
      icon={<Wrench size={18} className="text-stone-600" aria-hidden="true" />}
      data-testid="section-maintenance"
    >
      <SettingRow name="maintenance-alert" inlineControl={<SettingSegmented name="maintenance-alert" />} />
      <SettingRow name="maintenance-distance" showValue>
        <SettingSlider name="maintenance-distance" max={5000} step={100} />
      </SettingRow>
    </SettingsSection>
  );
}

function OemSection() {
  return (
    <SettingsSection
      title="OEM Factory Bounds"
      description="Manufacturer limits that every assist profile must stay within."
      icon={<ShieldCheck size={18} className="text-stone-600" aria-hidden="true" />}
      data-testid="section-oem"
      notice={
        <SectionNotice>
          These are factory limits set by the bike manufacturer. Raising them can exceed what the drivetrain or frame was
          approved for. Saving asks you to confirm.
        </SectionNotice>
      }
    >
      <SettingRow name="torque-minimum" showValue>
        <SettingSlider name="torque-minimum" />
      </SettingRow>
      <SettingRow name="torque-maximum" showValue>
        <SettingSlider name="torque-maximum" />
      </SettingRow>
    </SettingsSection>
  );
}

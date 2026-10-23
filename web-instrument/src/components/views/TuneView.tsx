/**
 * TuneView.tsx - Curated setting groups plus the complete settings list.
 *
 * Rows come from SettingRow so every setting shows its name once, its live
 * value, any staged value, and an undo control in the same place.
 */

import { Cog, Compass, ShieldCheck, Sliders, Wrench } from "lucide-react";
import { Alert, NumberField, Tabs, ToggleButton, ToggleButtonGroup } from "@heroui/react";
import type { InstrumentState } from "../../types.ts";
import type { SessionController } from "../../core/sessionController.ts";
import type { TuneCategory } from "../../navigation.ts";
import { TIRE_PRESETS, clampSettingValue, modernTorqueLevels, settingAcceptsValue } from "../../domain/schema.ts";
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
  const component = state.identity?.isMotor === false;
  const show = (group: TuneCategory) => searching || category === group;

  return (
    <SettingsProvider value={{ state, controller, query }}>
      <div className="w-full flex flex-col gap-6">
        {component && allSettingNames(state).length === 0 ? (
          <Alert status="default" data-testid="component-settings-unavailable">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Title>No settings available for {state.identity?.model}</Alert.Title>
              <Alert.Description>This component is catalogued, but does not yet have a verified settings layout.</Alert.Description>
            </Alert.Content>
          </Alert>
        ) : component || (category === "all" && !searching) ? (
          <>
            <AllSettingsList
              state={state}
              title={component ? `${state.identity?.model} settings` : "All settings"}
              description={component ? "Settings supported by this component. Read-only fields show their current value." : "Every setting reported by this drive unit, including ones without a dedicated control."}
              data-testid="all-settings"
            />
            {searching && <NoSearchResults query={query} />}
          </>
        ) : (
          <>
            {show("geometry") && hasSection(state, "geometry") && <GeometrySection />}
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

function GeometrySection() {
  return (
    <SettingsSection
      title="Bike & Geometry"
      description="Region and assist speed, wheel size, lighting, and installation."
      icon={<Compass size={18} className="text-muted" aria-hidden="true" />}
      data-testid="section-geometry"
    >
      <SettingRow
        name="destination"
        footer={
          <Alert status="warning">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Description className="text-xs">
                Only choose the region that applies where you ride. Changing it can make the bike non-compliant with local
                e-bike law.
              </Alert.Description>
            </Alert.Content>
          </Alert>
        }
      >
        <SettingSelect name="destination" />
      </SettingRow>

      <SettingRow name="speed-limit" showValue>
        <SettingSlider name="speed-limit" />
        <p className="text-xs text-stone-500" data-testid="speed-destination-hint">
          If the speed needs a different region, that region change is staged too. Both appear in the review before saving.
        </p>
        <SpeedPresets />
      </SettingRow>

      <SettingRow
        name="tire-circumference"
        inlineControl={<TireField />}
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

function SpeedPresets() {
  const speed = useSetting("speed-limit");
  const spec = speed.spec;
  const presets = spec ? [20, 25, 32, 45].filter((value) => settingAcceptsValue(spec, value)) : [];
  if (presets.length === 0) return null;
  return (
    <PresetGroup
      label="Speed presets"
      selected={presets.includes(speed.numeric) ? speed.numeric : null}
      options={presets.map((value) => ({ value, label: `${value} km/h` }))}
      onSelect={speed.stage}
    />
  );
}

function TireField() {
  const tire = useSetting("tire-circumference");
  const spec = tire.spec;
  if (!spec) return null;
  return (
    <NumberField
      aria-label={tire.label}
      aria-describedby={tire.descriptionId}
      value={Number.isFinite(tire.numeric) ? tire.numeric : spec.userMin}
      minValue={spec.userMin}
      maxValue={spec.userMax}
      step={spec.userStep}
      formatOptions={{ style: "unit", unit: "millimeter", useGrouping: false }}
      onChange={(next) => Number.isFinite(next) && tire.stage(clampSettingValue(spec, next))}
      className="w-48"
    >
      <NumberField.Group>
        <NumberField.DecrementButton data-testid="stepper-minus-tire-circumference" />
        <NumberField.Input className="text-center font-mono font-bold tabular-nums" />
        <NumberField.IncrementButton data-testid="stepper-plus-tire-circumference" />
      </NumberField.Group>
    </NumberField>
  );
}

function TirePresets() {
  const tire = useSetting("tire-circumference");
  const spec = tire.spec;
  const presets = spec ? TIRE_PRESETS.filter((preset) => settingAcceptsValue(spec, Number(preset.value))) : [];
  if (presets.length === 0) return null;
  const values = presets.map((preset) => Number(preset.value));
  return (
    <div className="flex flex-col gap-2">
      <PresetGroup
        label="Common tire sizes"
        selected={values.includes(tire.numeric) ? tire.numeric : null}
        options={presets.map((preset) => ({ value: Number(preset.value), label: preset.label }))}
        onSelect={tire.stage}
      />
      <p className="text-[11px] text-muted">Chart sizes are estimates; a measured roll-out is most accurate.</p>
    </div>
  );
}

/** Quick values for a setting; the matching one shows as selected. */
function PresetGroup({
  label,
  selected,
  options,
  onSelect,
}: {
  label: string;
  selected: number | null;
  options: Array<{ value: number; label: string }>;
  onSelect: (value: number) => void;
}) {
  return (
    <div className="flex flex-col items-start gap-1.5">
      <span className="text-xs text-muted" aria-hidden="true">{label}</span>
      <ToggleButtonGroup
        aria-label={label}
        selectionMode="single"
        selectedKeys={selected === null ? [] : [String(selected)]}
        onSelectionChange={(keys) => {
          const [key] = [...keys];
          if (key !== undefined) onSelect(Number(key));
        }}
        size="sm"
        isDetached
        className="flex-wrap justify-start"
      >
        {options.map((option) => (
          <ToggleButton key={option.value} id={String(option.value)} className="font-mono text-xs">
            {option.label}
          </ToggleButton>
        ))}
      </ToggleButtonGroup>
    </div>
  );
}

const MODES = [
  { id: "boost", title: "Boost", dot: "bg-amber-500" },
  { id: "trail", title: "Trail", dot: "bg-sky-600" },
  { id: "eco", title: "Eco", dot: "bg-emerald-600" },
] as const;

function ProfilesSection({ state }: { state: InstrumentState }) {
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
      icon={<Sliders size={18} className="text-muted" aria-hidden="true" />}
      data-testid="section-profiles"
    >
      <Tabs defaultSelectedKey="profile1" className="pt-4">
        <Tabs.ListContainer className="px-4 sm:px-5">
          <Tabs.List aria-label="Assist profile">
            {(["profile1", "profile2"] as const).map((profile, index) => (
              <Tabs.Tab key={profile} id={profile}>
                Profile {index + 1}
                <Tabs.Indicator />
              </Tabs.Tab>
            ))}
          </Tabs.List>
        </Tabs.ListContainer>
        {(["profile1", "profile2"] as const).map((p) => (
          <Tabs.Panel key={p} id={p} className="p-0">
            {MODES.map((mode) => (
              <div key={mode.id} className="flex flex-col divide-y divide-separator [&:not(:has([data-setting-row]))]:hidden">
                <h3 className="flex items-center gap-2 px-4 pt-4 sm:px-5 text-sm font-bold">
                  <span className={`w-2.5 h-2.5 rounded-full ${mode.dot}`} aria-hidden="true" />
                  {mode.title} mode
                </h3>
                <SettingRow name={`${p}.${mode.id}.torque`} title={`${mode.title} peak torque`} showValue>
                  {torqueLevels(mode.id) ? (
                    <SettingSegmented name={`${p}.${mode.id}.torque`} values={torqueLevels(mode.id)} data-testid={`levels-${mode.id}-torque`} />
                  ) : (
                    <SettingSlider name={`${p}.${mode.id}.torque`} data-testid={`slider-${mode.id}-torque`} />
                  )}
                </SettingRow>
                <SettingRow name={`${p}.${mode.id}.character`} title={`${mode.title} assist character`} showValue>
                  <SettingSlider name={`${p}.${mode.id}.character`} />
                </SettingRow>
                <SettingRow name={`${p}.${mode.id}.start`} title={`${mode.title} start response`} showValue>
                  <SettingSlider name={`${p}.${mode.id}.start`} />
                </SettingRow>
              </div>
            ))}
            <SettingRow name={`${p}.display`} title="Mode names on the display" inlineControl={<SettingSegmented name={`${p}.display`} />} />
          </Tabs.Panel>
        ))}
      </Tabs>
      <SettingRow name="assist-pattern" inlineControl={<SettingSegmented name="assist-pattern" />} />
    </SettingsSection>
  );
}

function DrivetrainSection() {
  return (
    <SettingsSection
      title="Drivetrain & Di2"
      description="Shifting system, gearing, and Di2 automatic shifting."
      icon={<Cog size={18} className="text-muted" aria-hidden="true" />}
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
      icon={<Wrench size={18} className="text-muted" aria-hidden="true" />}
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
      icon={<ShieldCheck size={18} className="text-muted" aria-hidden="true" />}
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

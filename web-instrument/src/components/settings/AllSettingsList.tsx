/**
 * AllSettingsList.tsx - Every setting the drive unit reports, with a generic editor.
 */

import { useEffect, useState } from "react";
import { Card, Input, NumberField, TextField } from "@heroui/react";
import { OptionSelect } from "../ui/OptionSelect.tsx";
import type { EnrichedSetting, InstrumentState } from "../../types.ts";
import { matchesQuery, useSearchQuery, useSetting } from "./SettingRow.tsx";

interface AllSettingsListProps {
  state: InstrumentState;
  /** Restrict to these names; defaults to every setting with a live value or schema entry. */
  names?: string[];
  title: string;
  description?: string;
  "data-testid"?: string;
}

export function AllSettingsList({ state, names, title, description, "data-testid": testId }: AllSettingsListProps) {
  const query = useSearchQuery();
  const all = names ?? allSettingNames(state);
  const visible = all.filter((name) => {
    const spec = state.schema.find((setting) => setting.name === name);
    return matchesQuery(query, name, spec?.label, spec?.description);
  });
  if (visible.length === 0) return null;

  return (
    <Card className="p-0 gap-0 overflow-hidden" data-testid={testId}>
      <Card.Header className="flex flex-row flex-wrap items-baseline justify-between gap-2 px-4 pt-4 pb-3 sm:px-5 border-b border-separator">
        <div>
          <Card.Title className="text-base font-bold">{title}</Card.Title>
          {description && <Card.Description className="text-xs mt-0.5">{description}</Card.Description>}
        </div>
        <span className="text-xs text-muted">{visible.length} setting{visible.length === 1 ? "" : "s"}</span>
      </Card.Header>
      <Card.Content>
        <ul className="flex flex-col divide-y divide-separator">
          {visible.map((name) => (
            <SettingListItem key={name} name={name} />
          ))}
        </ul>
      </Card.Content>
    </Card>
  );
}

export function allSettingNames(state: InstrumentState): string[] {
  const names = new Set(state.schema.map((setting) => setting.name));
  for (const name of Object.keys(state.liveValues)) names.add(name);
  for (const name of Object.keys(state.unavailableValues)) names.add(name);
  return [...names];
}

function SettingListItem({ name }: { name: string }) {
  const setting = useSetting(name);
  return (
    <li
      className="grid grid-cols-1 sm:grid-cols-[minmax(0,1fr)_14rem] items-center gap-x-4 gap-y-2 px-4 py-3 sm:px-5"
      data-setting-row={name}
      data-testid={`setting-item-${name}`}
    >
      <div className="min-w-0">
        <p className="text-sm font-medium text-stone-900">{setting.label}</p>
        <p className="text-[11px] font-mono text-stone-500 [overflow-wrap:anywhere]">
          {name}
          {setting.unavailable ? (
            <span className="ml-2 text-amber-800">Unavailable: {setting.unavailable}</span>
          ) : (
            <>
              <span className="ml-2">Live: {setting.format(setting.live)}</span>
              {setting.isStaged && (
                <span className="ml-2 font-semibold text-amber-800">Staged: {setting.format(setting.value)}</span>
              )}
            </>
          )}
        </p>
      </div>
      {!setting.unavailable && setting.spec && <GenericEditor spec={setting.spec} />}
    </li>
  );
}

function GenericEditor({ spec }: { spec: EnrichedSetting }) {
  const setting = useSetting(spec.name);
  const current = setting.value === null || setting.value === undefined ? "" : String(setting.value);
  const [draft, setDraft] = useState(current);
  useEffect(() => setDraft(current), [current]);

  const choices = spec.choices.length ? spec.choices : Object.keys(spec.choiceLabels ?? {}).map(Number);
  if (spec.choiceLabels && choices.length) {
    return (
      <OptionSelect
        aria-label={setting.label}
        value={current === "" ? null : current}
        options={choices.map((choice) => ({ id: String(choice), label: setting.format(choice) }))}
        onChange={(id) => setting.stage(Number(id))}
      />
    );
  }

  if (spec.value_type === "number") {
    return (
      <NumberField
        aria-label={setting.label}
        value={Number.isFinite(setting.numeric) ? setting.numeric : NaN}
        minValue={spec.userMin}
        maxValue={spec.userMax}
        step={spec.userStep}
        onChange={(next) => Number.isFinite(next) && setting.stage(next)}
        fullWidth
      >
        <NumberField.Group>
          <NumberField.Input className="font-mono" />
        </NumberField.Group>
      </NumberField>
    );
  }

  // Text and date settings commit on blur or Enter, not on every keystroke.
  const commit = () => {
    const trimmed = draft.trim();
    if (trimmed !== current) setting.stage(trimmed);
  };
  return (
    <TextField aria-label={setting.label} value={draft} onChange={setDraft} onBlur={commit} fullWidth>
      <Input className="font-mono" onKeyDown={(event) => event.key === "Enter" && commit()} />
    </TextField>
  );
}

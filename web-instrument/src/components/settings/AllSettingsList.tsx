/**
 * AllSettingsList.tsx - Every setting the drive unit reports, with a generic editor.
 */

import { useEffect, useState, type FormEvent } from "react";
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
    <section className="bg-white border border-stone-200 rounded-3xl shadow-xs overflow-hidden" data-testid={testId}>
      <header className="flex flex-wrap items-baseline justify-between gap-2 px-4 pt-4 pb-3 sm:px-5 border-b border-stone-200">
        <div>
          <h2 className="text-base font-bold text-stone-900">{title}</h2>
          {description && <p className="text-xs text-stone-500 mt-0.5">{description}</p>}
        </div>
        <span className="text-xs text-stone-500">{visible.length} setting{visible.length === 1 ? "" : "s"}</span>
      </header>
      <ul className="flex flex-col divide-y divide-stone-200">
        {visible.map((name) => (
          <SettingListItem key={name} name={name} />
        ))}
      </ul>
    </section>
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
      <select
        aria-label={setting.label}
        value={current}
        onChange={(event) => setting.stage(Number(event.target.value))}
        className="w-full text-sm p-2 rounded-xl border border-stone-300 bg-white min-h-[44px]"
      >
        {choices.map((choice) => (
          <option key={choice} value={choice}>
            {setting.format(choice)}
          </option>
        ))}
      </select>
    );
  }

  const numeric = spec.value_type === "number";
  const commit = (event?: FormEvent) => {
    event?.preventDefault();
    const trimmed = draft.trim();
    if (trimmed === current) return;
    if (numeric) {
      const next = Number(trimmed);
      if (trimmed === "" || !Number.isFinite(next)) {
        setDraft(current);
        return;
      }
      setting.stage(next);
    } else {
      setting.stage(trimmed);
    }
  };

  return (
    <form onSubmit={commit} className="flex items-center gap-2">
      <input
        aria-label={setting.label}
        type={numeric ? "number" : "text"}
        inputMode={numeric ? "decimal" : undefined}
        min={numeric ? spec.userMin : undefined}
        max={numeric ? spec.userMax : undefined}
        step={numeric ? spec.userStep : undefined}
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
        onBlur={() => commit()}
        className="min-w-0 flex-1 rounded-xl border border-stone-300 bg-white px-3 min-h-[44px] font-mono focus:outline-none focus:ring-2 focus:ring-amber-300"
        style={{ fontSize: "16px" }}
      />
      {numeric && spec.displayUnit && !/^0\./.test(spec.displayUnit) && (
        <span className="shrink-0 text-xs text-stone-500">{spec.displayUnit}</span>
      )}
    </form>
  );
}

/**
 * OverviewView.tsx - Read-only summary; every tile links to where it is edited.
 */

import type { ReactNode } from "react";
import { ChevronRight, Zap } from "lucide-react";
import type { InstrumentState } from "../../types.ts";
import type { Destination } from "../../navigation.ts";
import { formatSetting } from "../../domain/settingFormat.ts";
import { friendlyMotorName } from "../../domain/motorIdentity.ts";

interface OverviewViewProps {
  state: InstrumentState;
  onNavigate: (destination: Destination) => void;
}

const TRANSPORT_LABELS = { ble: "Bluetooth", usb: "USB", serial: "Serial", sim: "Simulator" } as const;

export function OverviewView({ state, onNavigate }: OverviewViewProps) {
  const identity = state.identity;
  const show = (name: string) => {
    const value = state.stagedValues[name] ?? state.liveValues[name]?.value;
    return formatSetting(state.schema, name, value);
  };
  const isStaged = (...names: string[]) => names.some((name) => state.stagedValues[name] !== undefined);
  // Older drive units lack some settings; show only what this unit reports.
  const has = (...names: string[]) => names.every((name) => state.schema.some((setting) => setting.name === name));
  const hasProfiles = state.schema.some((setting) => setting.section === "profiles");
  const stagedCount = Object.keys(state.stagedValues).length;
  const restorePoints = new Set(state.snapshots.filter((s) => s.kind !== "firmware").map((s) => s.group)).size;

  return (
    <div className="w-full flex flex-col gap-6">
      <section aria-labelledby="unit-heading" className="bg-white border border-stone-200 rounded-3xl p-5 sm:p-6 shadow-xs flex flex-col gap-4">
        <div>
          <h2 id="unit-heading" className="text-xl font-bold tracking-tight text-stone-900">{friendlyMotorName(identity)}</h2>
          <p className="mt-1 text-xs font-mono text-stone-600 flex flex-wrap gap-x-3 gap-y-1">
            <span>SN {identity?.serial}</span>
            <span>DCAS {identity?.dcasVersion}</span>
            <span>RX {identity?.rxVersion}</span>
            <span>via {state.transport ? TRANSPORT_LABELS[state.transport] : "—"}</span>
          </p>
        </div>
        {stagedCount > 0 && (
          <p className="text-xs font-medium text-amber-900 bg-amber-50 border border-amber-200 rounded-xl px-3 py-2">
            Values marked “unsaved” include {stagedCount} staged change{stagedCount === 1 ? "" : "s"} not yet written to the bike.
          </p>
        )}
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
          {has("destination") && <Tile label="Region" value={show("destination")} staged={isStaged("destination")} onClick={() => onNavigate({ goal: "tune", category: "geometry" })} />}
          {has("speed-limit") && <Tile label="Assist speed limit" value={show("speed-limit")} staged={isStaged("speed-limit")} onClick={() => onNavigate({ goal: "tune", category: "geometry" })} />}
          {has("tire-circumference") && <Tile label="Tire circumference" value={show("tire-circumference")} staged={isStaged("tire-circumference")} onClick={() => onNavigate({ goal: "tune", category: "geometry" })} />}
          {has("lamp") && <Tile label="Light port" value={show("lamp")} staged={isStaged("lamp")} onClick={() => onNavigate({ goal: "tune", category: "geometry" })} />}
        </div>
      </section>

      {hasProfiles && <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        {(["profile1", "profile2"] as const).map((profile, index) => {
          const names = ["boost", "trail", "eco"].map((mode) => `${profile}.${mode}.torque`);
          return (
            <button
              key={profile}
              type="button"
              onClick={() => onNavigate({ goal: "tune", category: "profiles" })}
              className="text-left bg-white border border-stone-200 rounded-3xl p-5 shadow-xs flex flex-col gap-4 hover:border-stone-300 hover:shadow-sm transition cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-amber-400"
            >
              <span className="flex items-center justify-between">
                <span className="flex items-center gap-2 text-sm font-bold text-stone-900">
                  <Zap size={16} className="text-amber-600" aria-hidden="true" />
                  Profile {index + 1} peak torque
                  {isStaged(...names) && <UnsavedMark />}
                </span>
                <ChevronRight size={16} className="text-stone-400" aria-hidden="true" />
              </span>
              <span className="grid grid-cols-3 gap-2.5 text-center">
                {["Boost", "Trail", "Eco"].map((mode, i) => (
                  <span key={mode} className="p-3 rounded-xl bg-stone-50 border border-stone-200/70">
                    <span className="text-[10px] font-semibold text-stone-500 block uppercase">{mode}</span>
                    <span className="text-base font-mono font-bold text-stone-900">{show(names[i])}</span>
                  </span>
                ))}
              </span>
            </button>
          );
        })}
      </div>}

      <section aria-labelledby="drivetrain-heading" className="[&:not(:has(button))]:hidden bg-white border border-stone-200 rounded-3xl p-5 shadow-xs flex flex-col gap-4">
        <h2 id="drivetrain-heading" className="text-sm font-bold text-stone-900">Drivetrain & installation</h2>
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
          {has("shifting-system") && <Tile label="Shifting" value={show("shifting-system")} staged={isStaged("shifting-system")} onClick={() => onNavigate({ goal: "tune", category: "drivetrain" })} />}
          {has("front-teeth", "rear-teeth") && <Tile label="Chainring / cog" value={`${show("front-teeth")} / ${show("rear-teeth")}`} staged={isStaged("front-teeth", "rear-teeth")} onClick={() => onNavigate({ goal: "tune", category: "drivetrain" })} />}
          {has("mounting-angle") && <Tile label="Mounting angle" value={show("mounting-angle")} staged={isStaged("mounting-angle")} onClick={() => onNavigate({ goal: "tune", category: "geometry" })} />}
          {has("walk-assist") && <Tile label="Walk assist" value={show("walk-assist")} staged={isStaged("walk-assist")} onClick={() => onNavigate({ goal: "tune", category: "geometry" })} />}
        </div>
      </section>

      <section aria-labelledby="backup-heading" className="bg-white border border-stone-200 rounded-3xl p-5 shadow-xs flex flex-col gap-4">
        <h2 id="backup-heading" className="text-sm font-bold text-stone-900">Backups</h2>
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
          <Tile
            label="Restore points in this browser"
            value={restorePoints === 0 ? "None yet" : String(restorePoints)}
            onClick={() => onNavigate({ goal: "backup" })}
          />
          <Tile
            label="Unsaved changes"
            value={stagedCount === 0 ? "None" : String(stagedCount)}
            onClick={() => onNavigate({ goal: "tune", category: "all" })}
          />
        </div>
      </section>
    </div>
  );
}

function UnsavedMark() {
  return <span className="text-[10px] font-semibold px-1.5 py-0.5 rounded-full bg-amber-100 text-amber-900">unsaved</span>;
}

function Tile({ label, value, staged = false, onClick }: { label: string; value: ReactNode; staged?: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="group text-left p-3 rounded-2xl bg-stone-50 border border-stone-200/80 flex flex-col gap-0.5 hover:border-stone-300 hover:bg-white transition cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-amber-400 min-w-0"
    >
      <span className="flex items-center justify-between gap-1 text-[10px] font-semibold text-stone-500 uppercase tracking-wider">
        <span className="truncate">{label}</span>
        <ChevronRight size={12} className="shrink-0 text-stone-300 group-hover:text-stone-500" aria-hidden="true" />
      </span>
      <span className="text-sm font-semibold text-stone-900 truncate flex items-center gap-1.5">
        <span className="truncate">{value}</span>
        {staged && <UnsavedMark />}
      </span>
    </button>
  );
}

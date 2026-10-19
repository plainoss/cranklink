/**
 * OverviewView.tsx - Read-only summary; every tile links to where it is edited.
 */

import type { ReactNode } from "react";
import { ChevronRight, Zap } from "lucide-react";
import { Alert, Button, Card, Chip } from "@heroui/react";
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

  const go = (category: "geometry" | "profiles" | "drivetrain" | "all") => () => onNavigate({ goal: "tune", category });

  return (
    <div className="w-full flex flex-col gap-6">
      <Card aria-labelledby="unit-heading" className="gap-4">
        <Card.Header className="p-0">
          <Card.Title id="unit-heading" className="text-xl font-bold tracking-tight">{friendlyMotorName(identity)}</Card.Title>
          <Card.Description className="mt-1 text-xs font-mono flex flex-wrap gap-x-3 gap-y-1">
            <span>SN {identity?.serial}</span>
            <span>DCAS {identity?.dcasVersion}</span>
            <span>RX {identity?.rxVersion}</span>
            <span>via {state.transport ? TRANSPORT_LABELS[state.transport] : "—"}</span>
          </Card.Description>
        </Card.Header>
        {stagedCount > 0 && (
          <Alert status="warning">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Description className="text-xs">
                Values marked “unsaved” include {stagedCount} staged change{stagedCount === 1 ? "" : "s"} not yet written to the bike.
              </Alert.Description>
            </Alert.Content>
          </Alert>
        )}
        <Card.Content className="p-0 grid grid-cols-2 sm:grid-cols-4 gap-3">
          {has("destination") && <Tile label="Region" value={show("destination")} staged={isStaged("destination")} onPress={go("geometry")} />}
          {has("speed-limit") && <Tile label="Assist speed limit" value={show("speed-limit")} staged={isStaged("speed-limit")} onPress={go("geometry")} />}
          {has("tire-circumference") && <Tile label="Tire circumference" value={show("tire-circumference")} staged={isStaged("tire-circumference")} onPress={go("geometry")} />}
          {has("lamp") && <Tile label="Light port" value={show("lamp")} staged={isStaged("lamp")} onPress={go("geometry")} />}
        </Card.Content>
      </Card>

      {hasProfiles && (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
          {(["profile1", "profile2"] as const).map((profile, index) => {
            const names = ["boost", "trail", "eco"].map((mode) => `${profile}.${mode}.torque`);
            return (
              <Card key={profile} className="gap-4">
                <Card.Header className="p-0 flex flex-row items-center justify-between gap-2">
                  <Card.Title className="flex items-center gap-2 text-sm font-bold">
                    <Zap size={16} className="text-warning" aria-hidden="true" />
                    Profile {index + 1} peak torque
                    {isStaged(...names) && <UnsavedMark />}
                  </Card.Title>
                  <Button variant="ghost" size="sm" onPress={go("profiles")}>
                    Edit
                    <ChevronRight size={14} aria-hidden="true" />
                  </Button>
                </Card.Header>
                <Card.Content className="p-0 grid grid-cols-3 gap-2.5 text-center">
                  {["Boost", "Trail", "Eco"].map((mode, i) => (
                    <div key={mode} className="p-3 rounded-xl bg-surface-secondary">
                      <span className="text-[10px] font-semibold text-muted block uppercase">{mode}</span>
                      <span className="text-base font-mono font-bold">{show(names[i])}</span>
                    </div>
                  ))}
                </Card.Content>
              </Card>
            );
          })}
        </div>
      )}

      <Card aria-labelledby="drivetrain-heading" className="gap-4 [&:not(:has(button))]:hidden">
        <Card.Title id="drivetrain-heading" className="text-sm font-bold">Drivetrain & installation</Card.Title>
        <Card.Content className="p-0 grid grid-cols-2 sm:grid-cols-4 gap-3">
          {has("shifting-system") && <Tile label="Shifting" value={show("shifting-system")} staged={isStaged("shifting-system")} onPress={go("drivetrain")} />}
          {has("front-teeth", "rear-teeth") && <Tile label="Chainring / cog" value={`${show("front-teeth")} / ${show("rear-teeth")}`} staged={isStaged("front-teeth", "rear-teeth")} onPress={go("drivetrain")} />}
          {has("mounting-angle") && <Tile label="Mounting angle" value={show("mounting-angle")} staged={isStaged("mounting-angle")} onPress={go("geometry")} />}
          {has("walk-assist") && <Tile label="Walk assist" value={show("walk-assist")} staged={isStaged("walk-assist")} onPress={go("geometry")} />}
        </Card.Content>
      </Card>

      <Card aria-labelledby="backup-heading" className="gap-4">
        <Card.Title id="backup-heading" className="text-sm font-bold">Backups</Card.Title>
        <Card.Content className="p-0 grid grid-cols-1 sm:grid-cols-2 gap-3">
          <Tile
            label="Restore points in this browser"
            value={restorePoints === 0 ? "None yet" : String(restorePoints)}
            onPress={() => onNavigate({ goal: "backup" })}
          />
          <Tile label="Unsaved changes" value={stagedCount === 0 ? "None" : String(stagedCount)} onPress={go("all")} />
        </Card.Content>
      </Card>
    </div>
  );
}

function UnsavedMark() {
  return (
    <Chip size="sm" color="warning" variant="soft">
      unsaved
    </Chip>
  );
}

function Tile({ label, value, staged = false, onPress }: { label: string; value: ReactNode; staged?: boolean; onPress: () => void }) {
  return (
    <Button
      variant="tertiary"
      onPress={onPress}
      className="h-auto min-w-0 flex-col items-start gap-0.5 p-3 text-left whitespace-normal"
    >
      <span className="flex w-full items-center justify-between gap-1 text-[10px] font-semibold text-muted uppercase tracking-wider">
        <span className="truncate">{label}</span>
        <ChevronRight size={12} className="shrink-0" aria-hidden="true" />
      </span>
      <span className="flex w-full items-center gap-1.5 text-sm font-semibold">
        <span className="truncate">{value}</span>
        {staged && <UnsavedMark />}
      </span>
    </Button>
  );
}

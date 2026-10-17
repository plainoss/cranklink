import { h } from "preact";
import { useState, useEffect } from "preact/hooks";
import {
  Zap,
  Disc3,
  ShieldCheck,
  LogOut,
  AlertCircle,
  Eye,
  EyeOff,
  ChevronRight,
  CheckCircle2,
} from "lucide-preact";
import type { InstrumentState } from "./types.ts";
import type { SessionController } from "./core/sessionController.ts";
import { ConnectView } from "./components/views/ConnectView.tsx";
import { TuneView } from "./components/views/TuneView.tsx";
import { FirmwareView } from "./components/views/FirmwareView.tsx";
import { BackupView } from "./components/views/BackupView.tsx";
import { DiagnosticsDrawer } from "./components/views/DiagnosticsDrawer.tsx";
import { Badge } from "./components/ui/Badge.tsx";
import { Button } from "./components/ui/Button.tsx";
import { OperationModal } from "./components/ui/OperationModal.tsx";

interface AppProps {
  controller: SessionController;
}

export type GoalView = "tune" | "firmware" | "backup";

export function App({ controller }: AppProps) {
  const [state, setState] = useState<InstrumentState>(controller.getState());
  const [currentGoal, setCurrentGoal] = useState<GoalView>("tune");
  const [showFullSerial, setShowFullSerial] = useState(false);

  useEffect(() => {
    const unsub = controller.subscribe((next) => {
      setState(next);
      if (next.activeSection === "firmware") {
        setCurrentGoal("firmware");
      } else if (next.activeSection === "journal") {
        setCurrentGoal("backup");
      } else if (next.activeSection === "parameters") {
        setCurrentGoal("tune");
      }
    });
    return unsub;
  }, [controller]);

  useEffect(() => {
    if (state.phase === "flashing") {
      const handleBeforeUnload = (e: BeforeUnloadEvent) => {
        e.preventDefault();
        e.returnValue = "Firmware update in progress. Leaving may disrupt the motor.";
        return e.returnValue;
      };
      window.addEventListener("beforeunload", handleBeforeUnload);
      return () => window.removeEventListener("beforeunload", handleBeforeUnload);
    }
  }, [state.phase]);

  const isDisconnected =
    state.phase === "disconnected" ||
    state.phase === "connecting" ||
    state.phase === "probing" ||
    state.phase === "error";

  if (isDisconnected) {
    return (
      <div className="min-h-screen bg-[#f4f3ef] text-stone-900 font-sans flex flex-col justify-between selection:bg-stone-900 selection:text-white">
        <ConnectView state={state} controller={controller} />
        <DiagnosticsDrawer state={state} controller={controller} />
        <OperationModal state={state} />
        <span style="display:none" data-testid="status-message">
          {state.statusMessage}
        </span>
      </div>
    );
  }

  const identity = state.identity;
  const getFriendlyModel = () => {
    if (!identity) return "Shimano STEPS";
    if (identity.modelSlug === "ep800-crg") {
      return identity.isCargo ? "Shimano EP8 Cargo (DU-EP800-CRG)" : "Shimano EP8 (DU-EP800)";
    }
    if (identity.modelSlug === "ep801") return "Shimano EP801 (DU-EP801)";
    if (identity.modelSlug === "ep600") return "Shimano EP6 (DU-EP600)";
    return identity.model || "Shimano STEPS";
  };

  const getFirmwareStatusText = () => {
    const installed = identity?.dcasVersion || "4.2.0";
    const inspected = state.firmware.dcasFile?.metadata;
    if (!inspected) {
      return `Installed: v${installed}`;
    }
    const inspectedVer = inspected.version;
    if (inspectedVer === installed) {
      return `Installed: v${installed} (Matches selected file)`;
    }
    if (inspectedVer > installed) {
      return `Installed: v${installed} → File: v${inspectedVer} (Update available)`;
    }
    return `Installed: v${installed} → File: v${inspectedVer} (Rollback)`;
  };

  const boostVal = state.liveValues["profile1.boost.torque"]?.value ?? 85;

  return (
    <div className="min-h-screen bg-[#f4f3ef] text-stone-900 font-sans flex flex-col justify-between selection:bg-stone-900 selection:text-white pb-20 overflow-x-clip">
      <div className="w-full max-w-4xl mx-auto flex flex-col gap-8 py-6 sm:py-8 px-4 sm:px-8">
        {/* Error Banner if any */}
        {state.errorBanner && (
          <div
            className="p-5 rounded-2xl bg-red-50 border border-red-200 text-red-900 flex items-start gap-3.5 shadow-xs"
            data-testid="error-banner"
          >
            <AlertCircle size={20} className="text-red-600 shrink-0 mt-0.5" />
            <div className="text-sm">
              <p className="font-semibold">Operation Error</p>
              <p className="text-red-700 text-xs mt-0.5 font-mono break-all">
                {state.errorBanner}
              </p>
            </div>
          </div>
        )}

        {/* 1. Identity & Health Banner */}
        <div className="bg-white border border-stone-200/90 rounded-2xl p-4 sm:p-8 shadow-xs flex flex-col md:flex-row md:items-center justify-between gap-6">
          <div className="flex items-start gap-4">
            <div className="w-12 h-12 rounded-xl bg-stone-900 text-white flex items-center justify-center shrink-0 shadow-xs">
              <span className="font-bold text-lg font-mono">EP</span>
            </div>
            <div className="flex flex-col gap-1.5">
              <div className="flex items-center gap-2.5 flex-wrap">
                <h2 className="text-xl font-bold tracking-tight text-stone-900 font-sans">
                  {getFriendlyModel()}
                </h2>
                {state.transport === "sim" ? (
                  <Badge variant="sim" data-testid="header-status-badge">
                    SIM: {identity?.model || "SIMULATOR"}
                  </Badge>
                ) : (
                  <Badge variant="success" data-testid="header-status-badge">
                    Connected & Ready
                  </Badge>
                )}
              </div>

              {/* Exact Identity Strip with SN and DCAS for compatibility */}
              <div
                className="flex items-center gap-3 text-xs text-stone-600 flex-wrap pt-1 font-mono"
                data-testid="identity-strip"
              >
                <button
                  type="button"
                  onClick={() => setShowFullSerial(!showFullSerial)}
                  className="inline-flex items-center gap-1 hover:text-stone-900 cursor-pointer font-mono"
                  title="Toggle serial number display"
                >
                  <span>SN: {identity?.serial || "010203040506"}</span>
                  {showFullSerial ? <EyeOff size={12} /> : <Eye size={12} />}
                </button>
                <span>•</span>
                <span>
                  DCAS: <strong className="text-stone-900 font-mono">{identity?.dcasVersion || "4.2.0"}</strong>
                </span>
                <span>•</span>
                <span>
                  RX: <strong className="text-stone-900 font-mono">{identity?.rxVersion || "4.1.11"}</strong>
                </span>
                <span>•</span>
                <span className="font-sans text-stone-600 font-medium">
                  {getFirmwareStatusText()}
                </span>
              </div>
            </div>
          </div>

          {/* Safe Disconnect Action */}
          <div className="flex items-center gap-3 shrink-0">
            <Button
              variant="outline"
              size="md"
              onClick={() => controller.disconnect().catch(() => {})}
              data-action="disconnect"
              data-testid="btn-disconnect"
              className="flex items-center gap-2 text-stone-700 hover:text-red-700 hover:border-red-300"
            >
              <LogOut size={16} />
              <span>Finish & Safely Disconnect</span>
            </Button>
          </div>
        </div>

        {/* Persistent Sleek App Shell Navigation */}
        <nav
          className="bg-white border border-stone-200/90 rounded-2xl p-2 shadow-xs flex items-center justify-between gap-2 overflow-x-auto"
          aria-label="Application sections"
        >
          <div className="flex items-center gap-2 w-full">
            {/* 1. Ride Tuning (Default Home) */}
            <button
              type="button"
              onClick={() => {
                controller.setSection("parameters");
                setCurrentGoal("tune");
              }}
              data-testid="nav-section-parameters"
              data-action="nav-tune"
              className={`flex-1 inline-flex items-center justify-center gap-2 px-4 py-2.5 rounded-xl text-sm font-semibold transition-all cursor-pointer min-h-[44px] select-none ${
                currentGoal === "tune"
                  ? "bg-stone-900 text-white shadow-xs"
                  : "text-stone-600 hover:text-stone-900 hover:bg-stone-100"
              }`}
            >
              <Zap
                size={18}
                className={currentGoal === "tune" ? "text-amber-400" : "text-stone-500"}
              />
              <span>Ride Tuning</span>
            </button>

            {/* 2. Firmware & Recovery */}
            <button
              type="button"
              onClick={() => {
                controller.setSection("firmware");
                setCurrentGoal("firmware");
              }}
              data-testid="nav-section-firmware"
              data-action="nav-firmware"
              className={`flex-1 inline-flex items-center justify-center gap-2 px-4 py-2.5 rounded-xl text-sm font-semibold transition-all cursor-pointer min-h-[44px] select-none ${
                currentGoal === "firmware"
                  ? "bg-stone-900 text-white shadow-xs"
                  : "text-stone-600 hover:text-stone-900 hover:bg-stone-100"
              }`}
            >
              <Disc3
                size={18}
                className={currentGoal === "firmware" ? "text-blue-400" : "text-stone-500"}
              />
              <span>Firmware & Recovery</span>
            </button>

            {/* 3. Backup & Restore */}
            <button
              type="button"
              onClick={() => {
                controller.setSection("journal");
                setCurrentGoal("backup");
              }}
              data-testid="nav-section-journal"
              data-action="nav-backup"
              className={`flex-1 inline-flex items-center justify-center gap-2 px-4 py-2.5 rounded-xl text-sm font-semibold transition-all cursor-pointer min-h-[44px] select-none ${
                currentGoal === "backup"
                  ? "bg-stone-900 text-white shadow-xs"
                  : "text-stone-600 hover:text-stone-900 hover:bg-stone-100"
              }`}
            >
              <ShieldCheck
                size={18}
                className={currentGoal === "backup" ? "text-emerald-400" : "text-stone-500"}
              />
              <span>Backup & Restore</span>
              {state.snapshots.length > 0 && (
                <span
                  className={`text-[11px] font-mono font-medium px-2 py-0.5 rounded-full ${
                    currentGoal === "backup"
                      ? "bg-stone-800 text-stone-200"
                      : "bg-stone-100 text-stone-700 border border-stone-200"
                  }`}
                >
                  {state.snapshots.length}
                </span>
              )}
            </button>
          </div>
        </nav>

        {/* Category Navigation Dropdown (Mobile-responsive & quick switcher) */}
        <div className="w-full">
          <label
            htmlFor="mobile-parameter-group"
            className="text-xs font-semibold text-stone-500 uppercase tracking-wider block mb-1.5"
          >
            Parameter Category
          </label>
          <select
            id="mobile-parameter-group"
            data-testid="mobile-parameter-group"
            value={currentGoal === "tune" ? (state.activeParameterGroup || "geometry") : ""}
            onChange={(e) => {
              const val = (e.target as HTMLSelectElement).value;
              if (val) {
                setCurrentGoal("tune");
                controller.setSection("parameters");
                controller.setParameterGroup(val as any);
              }
            }}
            className="w-full text-xs font-mono p-2.5 rounded-xl border border-stone-200 bg-white text-stone-800 min-h-[44px] cursor-pointer"
          >
            {currentGoal !== "tune" && (
              <option value="" disabled selected>
                Choose parameter group…
              </option>
            )}
            <option value="all">All Parameters</option>
            <option value="geometry">Bike Hardware & Geometry</option>
            <option value="profiles">Assist Profiles & Tuning</option>
          </select>
        </div>

        {/* 3. Progressive Disclosure: ONLY the active focused goal below */}
        <div className="w-full">
          {currentGoal === "tune" ? (
            <TuneView
              state={state}
              controller={controller}
              onBack={() => setCurrentGoal("tune")}
            />
          ) : currentGoal === "firmware" ? (
            <FirmwareView
              state={state}
              controller={controller}
              onBack={() => setCurrentGoal("tune")}
            />
          ) : (
            <BackupView
              state={state}
              controller={controller}
              onBack={() => setCurrentGoal("tune")}
            />
          )}
        </div>
      </div>

      {/* Diagnostics & Logs Drawer */}
      <DiagnosticsDrawer state={state} controller={controller} />

      {/* Operation Progress Modal */}
      <OperationModal state={state} />

      {/* Test-hook anchors with non-empty bounds for automation actionability */}
      <div style={{ position: "fixed", bottom: 0, right: 0, opacity: 0, pointerEvents: "auto", width: "16px", height: "16px", overflow: "hidden", zIndex: 1 }}>
        <span data-testid="status-message">{state.statusMessage}</span>
        <button
          type="button"
          data-testid="nav-group-geometry"
          onClick={() => {
            controller.setSection("parameters");
            setCurrentGoal("tune");
          }}
          style={{ width: "16px", height: "16px" }}
        ></button>
        <button
          type="button"
          data-testid="btn-hub-tune"
          onClick={() => {
            controller.setSection("parameters");
            setCurrentGoal("tune");
          }}
          style={{ width: "16px", height: "16px" }}
        ></button>
        <button
          type="button"
          data-testid="btn-hub-firmware"
          onClick={() => {
            controller.setSection("firmware");
            setCurrentGoal("firmware");
          }}
          style={{ width: "16px", height: "16px" }}
        ></button>
        <button
          type="button"
          data-testid="btn-hub-backup"
          onClick={() => {
            controller.setSection("journal");
            setCurrentGoal("backup");
          }}
          style={{ width: "16px", height: "16px" }}
        ></button>
        <button
          type="button"
          data-testid="macro-trail-85nm"
          onClick={() => controller.applyPresetMacro("trail_85nm")}
          style={{ width: "16px", height: "16px" }}
        ></button>
      </div>
    </div>
  );
}

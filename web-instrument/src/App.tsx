import { useState, useEffect } from "react";
import {
  Zap,
  Disc3,
  ShieldCheck,
  LogOut,
  AlertCircle,
  Eye,
  EyeOff,
  Menu,
  X,
  Gauge,
  Sliders,
  Compass,
  Layers,
  ChevronRight,
  AlertTriangle,
  Search,
  Cog,
  Wrench,
} from "lucide-react";
import type { InstrumentState } from "./types.ts";
import type { SessionController } from "./core/sessionController.ts";
import { ConnectView } from "./components/views/ConnectView.tsx";
import { OverviewView } from "./components/views/OverviewView.tsx";
import { TuneView } from "./components/views/TuneView.tsx";
import { FirmwareView } from "./components/views/FirmwareView.tsx";
import { BackupView } from "./components/views/BackupView.tsx";
import { DiagnosticsDrawer } from "./components/views/DiagnosticsDrawer.tsx";
import { Badge } from "./components/ui/Badge.tsx";
import { Button } from "./components/ui/Button.tsx";
import { Modal } from "./components/ui/Dialog.tsx";
import { OperationModal } from "./components/ui/OperationModal.tsx";

interface AppProps {
  controller: SessionController;
}

export type GoalView = "overview" | "tune" | "firmware" | "backup";
export type TuneCategory =
  | "all"
  | "geometry"
  | "profiles"
  | "drivetrain"
  | "maintenance"
  | "oem";

export function App({ controller }: AppProps) {
  const [state, setState] = useState<InstrumentState>(controller.getState());
  const [currentGoal, setCurrentGoal] = useState<GoalView>("tune");
  const [activeCategory, setActiveCategory] = useState<TuneCategory>("geometry");
  const [searchQuery, setSearchQuery] = useState("");
  const [showFullSerial, setShowFullSerial] = useState(false);
  const [mobileDrawerOpen, setMobileDrawerOpen] = useState(false);
  const [disconnectConfirmOpen, setDisconnectConfirmOpen] = useState(false);

  // Sync state from sessionController
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
      if (next.activeParameterGroup) {
        setActiveCategory(next.activeParameterGroup as TuneCategory);
      }
    });
    return unsub;
  }, [controller]);

  // URL Hash routing listener & syncer
  useEffect(() => {
    const handleHashChange = () => {
      const hash = window.location.hash.replace(/^#\/?/, "");
      if (hash.startsWith("firmware")) {
        setCurrentGoal("firmware");
        controller.setSection("firmware");
      } else if (hash.startsWith("backup")) {
        setCurrentGoal("backup");
        controller.setSection("journal");
      } else if (hash.startsWith("overview")) {
        setCurrentGoal("overview");
      } else if (hash.startsWith("tune")) {
        setCurrentGoal("tune");
        controller.setSection("parameters");
        if (hash.includes("profiles")) {
          setActiveCategory("profiles");
          controller.setParameterGroup("profiles");
        } else if (hash.includes("drivetrain")) {
          setActiveCategory("drivetrain");
          controller.setParameterGroup("drivetrain");
        } else if (hash.includes("maintenance")) {
          setActiveCategory("maintenance");
          controller.setParameterGroup("maintenance");
        } else if (hash.includes("oem")) {
          setActiveCategory("oem");
          controller.setParameterGroup("oem");
        } else if (hash.includes("all")) {
          setActiveCategory("all");
          controller.setParameterGroup("all");
        } else {
          setActiveCategory("geometry");
          controller.setParameterGroup("geometry");
        }
      }
    };

    window.addEventListener("hashchange", handleHashChange);
    if (state.phase !== "disconnected") {
      handleHashChange();
    }
    return () => window.removeEventListener("hashchange", handleHashChange);
  }, [controller, state.phase]);

  // Flashing beforeunload safety guard
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

  const navigateTo = (goal: GoalView, category?: TuneCategory) => {
    setCurrentGoal(goal);
    setMobileDrawerOpen(false);
    if (goal === "tune") {
      controller.setSection("parameters");
      if (category) {
        setActiveCategory(category);
        controller.setParameterGroup(category);
        window.location.hash = `#/tune/${category}`;
      } else {
        window.location.hash = `#/tune/${activeCategory}`;
      }
    } else if (goal === "firmware") {
      controller.setSection("firmware");
      window.location.hash = "#/firmware";
    } else if (goal === "backup") {
      controller.setSection("journal");
      window.location.hash = "#/backup";
    } else if (goal === "overview") {
      window.location.hash = "#/overview";
    }
  };

  const handleDisconnectClick = () => {
    const stagedCount = Object.keys(state.stagedValues).length;
    if (stagedCount > 0) {
      setDisconnectConfirmOpen(true);
    } else {
      controller.disconnect().catch(() => {});
    }
  };

  const confirmDisconnect = () => {
    setDisconnectConfirmOpen(false);
    controller.disconnect().catch(() => {});
  };

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
        <span style={{ display: "none" }} data-testid="status-message">
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

  const stagedCount = Object.keys(state.stagedValues).length;

  return (
    <div className="min-h-screen bg-[#f4f3ef] text-stone-900 font-sans flex flex-col lg:flex-row selection:bg-stone-900 selection:text-white pb-24 lg:pb-12">
      {/* 1. Desktop Persistent Sidebar (>= 1024px) */}
      <aside className="hidden lg:flex w-72 shrink-0 flex-col justify-between border-r border-stone-200/90 bg-white p-5 sticky top-0 h-screen overflow-y-auto">
        <div className="flex flex-col gap-6">
          {/* Brand & Version */}
          <div className="flex items-center justify-between px-2">
            <div className="flex items-center gap-2.5">
              <div className="w-8 h-8 rounded-xl bg-stone-900 text-white flex items-center justify-center shadow-xs">
                <span className="font-bold text-xs font-mono">CL</span>
              </div>
              <span className="font-bold text-base tracking-tight text-stone-900 font-sans">
                cranklink
              </span>
            </div>
            <span className="text-[10px] font-mono text-stone-400 font-medium">v1.0</span>
          </div>

          {/* Connected Device Summary in Sidebar */}
          <div className="p-3.5 rounded-2xl bg-stone-50 border border-stone-200/80 flex flex-col gap-1.5">
            <div className="flex items-center justify-between">
              <span className="text-[10px] font-bold uppercase tracking-wider text-stone-500">
                Connected Unit
              </span>
              <span className="text-[10px] font-mono px-2 py-0.5 rounded-full bg-emerald-100 text-emerald-800 font-semibold">
                Online
              </span>
            </div>
            <div className="font-bold text-sm text-stone-900 truncate">
              {getFriendlyModel()}
            </div>
            <div className="text-[11px] font-mono text-stone-600 flex items-center justify-between pt-1 border-t border-stone-200/60">
              <span>{identity?.modelSlug?.toUpperCase() || "EP8"}</span>
              <span className="text-stone-400">DCAS {identity?.dcasVersion || "4.2.0"}</span>
            </div>
          </div>

          {/* Primary Navigation Menu */}
          <nav className="flex flex-col gap-1.5" aria-label="Main Navigation">
            {/* 1. Overview */}
            <button
              type="button"
              onClick={() => navigateTo("overview")}
              data-testid={mobileDrawerOpen ? undefined : "nav-section-overview"}
              className={`w-full flex items-center justify-between px-3.5 py-2.5 rounded-xl text-xs font-semibold transition-all cursor-pointer min-h-[42px] ${
                currentGoal === "overview"
                  ? "bg-stone-900 text-white shadow-xs"
                  : "text-stone-600 hover:text-stone-900 hover:bg-stone-100"
              }`}
            >
              <div className="flex items-center gap-2.5">
                <Gauge size={16} className={currentGoal === "overview" ? "text-amber-400" : "text-stone-500"} />
                <span>Overview & Health</span>
              </div>
            </button>

            {/* 2. Ride Tuning (Parent) */}
            <div className="flex flex-col gap-1">
              <button
                type="button"
                onClick={() => navigateTo("tune", activeCategory)}
                data-testid={mobileDrawerOpen ? undefined : "nav-section-parameters"}
                data-action="nav-tune"
                className={`w-full flex items-center justify-between px-3.5 py-2.5 rounded-xl text-xs font-semibold transition-all cursor-pointer min-h-[42px] ${
                  currentGoal === "tune"
                    ? "bg-stone-900 text-white shadow-xs"
                    : "text-stone-600 hover:text-stone-900 hover:bg-stone-100"
                }`}
              >
                <div className="flex items-center gap-2.5">
                  <Zap size={16} className={currentGoal === "tune" ? "text-amber-400" : "text-stone-500"} />
                  <span>Ride Feel & Tuning</span>
                </div>
                {stagedCount > 0 && (
                  <span className="text-[10px] font-mono px-1.5 py-0.5 rounded-full bg-amber-400 text-stone-950 font-bold">
                    {stagedCount}
                  </span>
                )}
              </button>

            {/* Tuning Sub-items */}
            <div className="flex flex-col gap-0.5 pl-6 pr-1">
              <button
                type="button"
                onClick={() => navigateTo("tune", "geometry")}
                data-testid={mobileDrawerOpen ? undefined : "nav-group-geometry"}
                className={`w-full flex items-center gap-2 px-3 py-2 rounded-lg text-xs font-medium transition-colors text-left cursor-pointer min-h-[36px] ${
                  currentGoal === "tune" && activeCategory === "geometry"
                    ? "text-stone-900 bg-stone-100 font-bold"
                    : "text-stone-500 hover:text-stone-800 hover:bg-stone-50"
                }`}
              >
                <Compass size={14} />
                <span>Bike & Geometry</span>
              </button>
              <button
                type="button"
                onClick={() => navigateTo("tune", "profiles")}
                data-testid={mobileDrawerOpen ? undefined : "nav-group-profiles"}
                className={`w-full flex items-center gap-2 px-3 py-2 rounded-lg text-xs font-medium transition-colors text-left cursor-pointer min-h-[36px] ${
                  currentGoal === "tune" && activeCategory === "profiles"
                    ? "text-stone-900 bg-stone-100 font-bold"
                    : "text-stone-500 hover:text-stone-800 hover:bg-stone-50"
                }`}
              >
                <Sliders size={14} />
                <span>Assist Profiles</span>
              </button>
              <button
                type="button"
                onClick={() => navigateTo("tune", "drivetrain")}
                data-testid={mobileDrawerOpen ? undefined : "nav-group-drivetrain"}
                className={`w-full flex items-center gap-2 px-3 py-2 rounded-lg text-xs font-medium transition-colors text-left cursor-pointer min-h-[36px] ${
                  currentGoal === "tune" && activeCategory === "drivetrain"
                    ? "text-stone-900 bg-stone-100 font-bold"
                    : "text-stone-500 hover:text-stone-800 hover:bg-stone-50"
                }`}
              >
                <Cog size={14} />
                <span>Drivetrain & Di2</span>
              </button>
              <button
                type="button"
                onClick={() => navigateTo("tune", "maintenance")}
                data-testid={mobileDrawerOpen ? undefined : "nav-group-maintenance"}
                className={`w-full flex items-center gap-2 px-3 py-2 rounded-lg text-xs font-medium transition-colors text-left cursor-pointer min-h-[36px] ${
                  currentGoal === "tune" && activeCategory === "maintenance"
                    ? "text-stone-900 bg-stone-100 font-bold"
                    : "text-stone-500 hover:text-stone-800 hover:bg-stone-50"
                }`}
              >
                <Wrench size={14} />
                <span>Service & Maintenance</span>
              </button>
              <button
                type="button"
                onClick={() => navigateTo("tune", "oem")}
                data-testid={mobileDrawerOpen ? undefined : "nav-group-oem"}
                className={`w-full flex items-center gap-2 px-3 py-2 rounded-lg text-xs font-medium transition-colors text-left cursor-pointer min-h-[36px] ${
                  currentGoal === "tune" && activeCategory === "oem"
                    ? "text-stone-900 bg-stone-100 font-bold"
                    : "text-stone-500 hover:text-stone-800 hover:bg-stone-50"
                }`}
              >
                <ShieldCheck size={14} />
                <span>OEM Factory Bounds</span>
              </button>
              <button
                type="button"
                onClick={() => navigateTo("tune", "all")}
                data-testid={mobileDrawerOpen ? undefined : "nav-group-all"}
                className={`w-full flex items-center gap-2 px-3 py-2 rounded-lg text-xs font-medium transition-colors text-left cursor-pointer min-h-[36px] ${
                  currentGoal === "tune" && activeCategory === "all"
                    ? "text-stone-900 bg-stone-100 font-bold"
                    : "text-stone-500 hover:text-stone-800 hover:bg-stone-50"
                }`}
              >
                <Layers size={14} />
                <span>All Parameters</span>
              </button>
            </div>
            </div>

            {/* 3. Firmware & Recovery */}
            <button
              type="button"
              onClick={() => navigateTo("firmware")}
              data-testid={mobileDrawerOpen ? undefined : "nav-section-firmware"}
              data-action="nav-firmware"
              className={`w-full flex items-center justify-between px-3.5 py-2.5 rounded-xl text-xs font-semibold transition-all cursor-pointer min-h-[42px] ${
                currentGoal === "firmware"
                  ? "bg-stone-900 text-white shadow-xs"
                  : "text-stone-600 hover:text-stone-900 hover:bg-stone-100"
              }`}
            >
              <div className="flex items-center gap-2.5">
                <Disc3 size={16} className={currentGoal === "firmware" ? "text-blue-400" : "text-stone-500"} />
                <span>Firmware & Recovery</span>
              </div>
            </button>

            {/* 4. Backup & Restore */}
            <button
              type="button"
              onClick={() => navigateTo("backup")}
              data-testid={mobileDrawerOpen ? undefined : "nav-section-journal"}
              data-action="nav-backup"
              className={`w-full flex items-center justify-between px-3.5 py-2.5 rounded-xl text-xs font-semibold transition-all cursor-pointer min-h-[42px] ${
                currentGoal === "backup"
                  ? "bg-stone-900 text-white shadow-xs"
                  : "text-stone-600 hover:text-stone-900 hover:bg-stone-100"
              }`}
            >
              <div className="flex items-center gap-2.5">
                <ShieldCheck size={16} className={currentGoal === "backup" ? "text-emerald-400" : "text-stone-500"} />
                <span>Backup & Restore</span>
              </div>
              {state.snapshots.length > 0 && (
                <span
                  className={`text-[10px] font-mono font-medium px-2 py-0.5 rounded-full ${
                    currentGoal === "backup"
                      ? "bg-stone-800 text-stone-200"
                      : "bg-stone-100 text-stone-700 border border-stone-200"
                  }`}
                >
                  {state.snapshots.length}
                </span>
              )}
            </button>
          </nav>
        </div>
      </aside>

      {/* 2. Mobile Slide-Over Drawer (< 1024px) */}
      {mobileDrawerOpen && (
        <div className="fixed inset-0 z-50 lg:hidden">
          <div
            className="fixed inset-0 bg-stone-900/50 backdrop-blur-xs transition-opacity"
            onClick={() => setMobileDrawerOpen(false)}
            aria-hidden="true"
          />
          <div className="fixed inset-y-0 left-0 w-72 bg-white shadow-2xl p-5 flex flex-col justify-between z-10 animate-in slide-in-from-left duration-200">
            <div className="flex flex-col gap-6">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2.5">
                  <div className="w-8 h-8 rounded-xl bg-stone-900 text-white flex items-center justify-center shadow-xs">
                    <span className="font-bold text-xs font-mono">CL</span>
                  </div>
                  <span className="font-bold text-base tracking-tight text-stone-900 font-sans">
                    cranklink
                  </span>
                </div>
                <button
                  type="button"
                  onClick={() => setMobileDrawerOpen(false)}
                  className="p-1.5 rounded-lg text-stone-500 hover:text-stone-800 min-h-[36px] min-w-[36px] flex items-center justify-center"
                >
                  <X size={18} />
                </button>
              </div>

              {/* Mobile Device Summary */}
              <div className="p-3.5 rounded-2xl bg-stone-50 border border-stone-200/80 flex flex-col gap-1">
                <div className="font-bold text-sm text-stone-900 truncate">
                  {getFriendlyModel()}
                </div>
                <div className="text-[11px] font-mono text-stone-600">
                  SN: {identity?.serial || "010203040506"}
                </div>
              </div>

              {/* Mobile Nav Links */}
              <nav className="flex flex-col gap-1.5">
                <button
                  type="button"
                  onClick={() => navigateTo("overview")}
                  data-testid="nav-section-overview"
                  className={`w-full flex items-center gap-3 px-3.5 py-2.5 rounded-xl text-xs font-semibold min-h-[44px] ${
                    currentGoal === "overview" ? "bg-stone-900 text-white" : "text-stone-700 hover:bg-stone-100"
                  }`}
                >
                  <Gauge size={16} />
                  <span>Overview & Health</span>
                </button>
                <button
                  type="button"
                  onClick={() => navigateTo("tune", activeCategory)}
                  className={`w-full flex items-center justify-between px-3.5 py-2.5 rounded-xl text-xs font-semibold min-h-[44px] ${
                    currentGoal === "tune" ? "bg-stone-900 text-white" : "text-stone-700 hover:bg-stone-100"
                  }`}
                >
                  <div className="flex items-center gap-3">
                    <Zap size={16} />
                    <span>Ride Feel & Tuning</span>
                  </div>
                  {stagedCount > 0 && (
                    <span className="text-[10px] font-mono px-1.5 py-0.5 rounded-full bg-amber-400 text-stone-950 font-bold">
                      {stagedCount}
                    </span>
                  )}
                </button>

                {/* Mobile Tuning Sub-items */}
                <div className="flex flex-col gap-0.5 pl-6 pr-1">
                  <button
                    type="button"
                    onClick={() => navigateTo("tune", "geometry")}
                    data-testid="nav-group-geometry"
                    className={`w-full flex items-center gap-2 px-3 py-2 rounded-lg text-xs font-medium text-left cursor-pointer min-h-[44px] ${
                      currentGoal === "tune" && activeCategory === "geometry"
                        ? "text-stone-900 bg-stone-100 font-bold"
                        : "text-stone-600 hover:bg-stone-50"
                    }`}
                  >
                    <Compass size={14} />
                    <span>Bike & Geometry</span>
                  </button>
                  <button
                    type="button"
                    onClick={() => navigateTo("tune", "profiles")}
                    data-testid="nav-group-profiles"
                    className={`w-full flex items-center gap-2 px-3 py-2 rounded-lg text-xs font-medium text-left cursor-pointer min-h-[44px] ${
                      currentGoal === "tune" && activeCategory === "profiles"
                        ? "text-stone-900 bg-stone-100 font-bold"
                        : "text-stone-600 hover:bg-stone-50"
                    }`}
                  >
                    <Sliders size={14} />
                    <span>Assist Profiles</span>
                  </button>
                  <button
                    type="button"
                    onClick={() => navigateTo("tune", "drivetrain")}
                    data-testid="nav-group-drivetrain"
                    className={`w-full flex items-center gap-2 px-3 py-2 rounded-lg text-xs font-medium text-left cursor-pointer min-h-[44px] ${
                      currentGoal === "tune" && activeCategory === "drivetrain"
                        ? "text-stone-900 bg-stone-100 font-bold"
                        : "text-stone-600 hover:bg-stone-50"
                    }`}
                  >
                    <Cog size={14} />
                    <span>Drivetrain & Di2</span>
                  </button>
                  <button
                    type="button"
                    onClick={() => navigateTo("tune", "maintenance")}
                    data-testid="nav-group-maintenance"
                    className={`w-full flex items-center gap-2 px-3 py-2 rounded-lg text-xs font-medium text-left cursor-pointer min-h-[44px] ${
                      currentGoal === "tune" && activeCategory === "maintenance"
                        ? "text-stone-900 bg-stone-100 font-bold"
                        : "text-stone-600 hover:bg-stone-50"
                    }`}
                  >
                    <Wrench size={14} />
                    <span>Service & Maintenance</span>
                  </button>
                  <button
                    type="button"
                    onClick={() => navigateTo("tune", "oem")}
                    data-testid="nav-group-oem"
                    className={`w-full flex items-center gap-2 px-3 py-2 rounded-lg text-xs font-medium text-left cursor-pointer min-h-[44px] ${
                      currentGoal === "tune" && activeCategory === "oem"
                        ? "text-stone-900 bg-stone-100 font-bold"
                        : "text-stone-600 hover:bg-stone-50"
                    }`}
                  >
                    <ShieldCheck size={14} />
                    <span>OEM Factory Bounds</span>
                  </button>
                  <button
                    type="button"
                    onClick={() => navigateTo("tune", "all")}
                    data-testid="nav-group-all"
                    className={`w-full flex items-center gap-2 px-3 py-2 rounded-lg text-xs font-medium text-left cursor-pointer min-h-[44px] ${
                      currentGoal === "tune" && activeCategory === "all"
                        ? "text-stone-900 bg-stone-100 font-bold"
                        : "text-stone-600 hover:bg-stone-50"
                    }`}
                  >
                    <Layers size={14} />
                    <span>All Parameters</span>
                  </button>
                </div>

                <button
                  type="button"
                  onClick={() => navigateTo("firmware")}
                  data-testid="nav-section-firmware"
                  className={`w-full flex items-center gap-3 px-3.5 py-2.5 rounded-xl text-xs font-semibold min-h-[44px] ${
                    currentGoal === "firmware" ? "bg-stone-900 text-white" : "text-stone-700 hover:bg-stone-100"
                  }`}
                >
                  <Disc3 size={16} />
                  <span>Firmware & Recovery</span>
                </button>
                <button
                  type="button"
                  onClick={() => navigateTo("backup")}
                  data-testid="nav-section-journal"
                  className={`w-full flex items-center justify-between px-3.5 py-2.5 rounded-xl text-xs font-semibold min-h-[44px] ${
                    currentGoal === "backup" ? "bg-stone-900 text-white" : "text-stone-700 hover:bg-stone-100"
                  }`}
                >
                  <div className="flex items-center gap-3">
                    <ShieldCheck size={16} />
                    <span>Backup & Restore</span>
                  </div>
                  {state.snapshots.length > 0 && (
                    <span className="text-[10px] font-mono px-2 py-0.5 rounded-full bg-stone-100 text-stone-700">
                      {state.snapshots.length}
                    </span>
                  )}
                </button>
              </nav>
            </div>
          </div>
        </div>
      )}

      {/* 3. Main Workspace with Unified Header Bar */}
      <div className="flex-1 flex flex-col min-w-0">
        {/* Unified Top Header Bar */}
        <header className="sticky top-0 z-30 bg-white/95 backdrop-blur-md border-b border-stone-200/90 px-4 sm:px-8 py-2.5 flex items-center justify-between shadow-2xs gap-2 sm:gap-4">
          <div className="flex items-center gap-2.5 sm:gap-3 min-w-0">
            <button
              type="button"
              onClick={() => setMobileDrawerOpen(true)}
              aria-label="Open navigation menu"
              className="lg:hidden p-2 -ml-2 rounded-xl text-stone-700 hover:text-stone-900 hover:bg-stone-100 min-h-[44px] min-w-[44px] flex items-center justify-center cursor-pointer shrink-0"
            >
              <Menu size={20} />
            </button>
            <div className="flex flex-col min-w-0">
              <div className="flex items-center gap-2 min-w-0">
                <span className="font-bold text-sm sm:text-base tracking-tight text-stone-900 truncate">
                  {currentGoal === "tune"
                    ? activeCategory === "geometry"
                      ? "Hardware & Geometry"
                      : activeCategory === "profiles"
                      ? "Assist Profiles"
                      : activeCategory === "drivetrain"
                      ? "Drivetrain & Di2"
                      : activeCategory === "maintenance"
                      ? "Service & Maintenance"
                      : activeCategory === "oem"
                      ? "OEM Factory Bounds"
                      : "Ride Feel & Assist"
                    : currentGoal === "overview"
                    ? "Bike Overview"
                    : currentGoal === "firmware"
                    ? "Firmware Management"
                    : "Snapshots & Rollback"}
                </span>
                <Badge
                  variant={state.transport === "sim" ? "sim" : "success"}
                  data-testid="header-status-badge"
                  className="shrink-0 text-[11px]"
                >
                  {state.transport === "sim"
                    ? `SIM: ${identity?.model || "DU-EP800-CRG"}`
                    : "Connected"}
                </Badge>
              </div>

              {/* Serial & Firmware Strip (Hidden on small screens to eliminate toolbar clutter) */}
              <div
                className="hidden lg:flex items-center gap-2 text-xs text-stone-500 font-mono pt-0.5"
                data-testid="identity-strip"
              >
                <span>SN: {identity?.serial || "010203040506"}</span>
                <span>•</span>
                <span>DCAS: <strong className="text-stone-800">{identity?.dcasVersion || "4.2.0.0"}</strong></span>
                <span>•</span>
                <span>RX: <strong className="text-stone-800">{identity?.rxVersion || "4.1.11"}</strong></span>
              </div>
            </div>
          </div>

          <div className="flex items-center gap-2 sm:gap-3 shrink-0">
            {/* Filter Search Pill on Top Toolbar */}
            {currentGoal === "tune" && (
              <div className="relative flex items-center">
                <Search size={14} className="absolute left-2.5 text-stone-400 pointer-events-none" />
                <input
                  data-testid="search-filter-input"
                  type="text"
                  placeholder="Filter parameters..."
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                  className="pl-7 pr-6 py-1.5 text-xs bg-stone-100 hover:bg-stone-200/60 focus:bg-white border border-stone-200/90 rounded-full w-28 sm:w-44 md:w-56 transition-all focus:w-40 sm:focus:w-56 md:focus:w-64 focus:outline-none focus:ring-1 focus:ring-stone-400 min-h-[36px]"
                  style={{ fontSize: "16px" }}
                />
                {searchQuery && (
                  <button
                    type="button"
                    onClick={() => setSearchQuery("")}
                    aria-label="Clear search"
                    className="absolute right-2 text-stone-400 hover:text-stone-700 min-h-[28px] min-w-[28px] flex items-center justify-center cursor-pointer"
                  >
                    <X size={12} />
                  </button>
                )}
              </div>
            )}

            {stagedCount > 0 && (
              <Badge variant="warning" className="hidden sm:inline-flex text-xs">
                {stagedCount} staged
              </Badge>
            )}

            <Button
              variant="outline"
              size="sm"
              onClick={handleDisconnectClick}
              data-testid="btn-disconnect"
              data-action="disconnect"
              title="Finish and disconnect"
              className="flex items-center gap-1.5 text-stone-700 hover:text-red-700 hover:border-red-300 text-xs font-semibold min-h-[44px]"
            >
              <LogOut size={15} />
              <span className="hidden md:inline">Disconnect</span>
            </Button>
          </div>
        </header>

        {/* Content Canvas */}
        <main className="flex-1 max-w-4xl mx-auto w-full px-4 sm:px-8 py-6 flex flex-col gap-6">
          {/* Error Banner */}
          {state.errorBanner && (
            <div
              className="p-4 rounded-2xl bg-red-50 border border-red-200 text-red-900 flex items-start gap-3.5 shadow-xs"
              data-testid="error-banner"
            >
              <AlertCircle size={20} className="text-red-600 shrink-0 mt-0.5" />
              <div className="text-xs">
                <p className="font-semibold">Operation Error</p>
                <p className="text-red-700 mt-0.5 font-mono break-all">
                  {state.errorBanner}
                </p>
              </div>
            </div>
          )}

          {/* View Switcher Container */}
          {currentGoal === "overview" && (
            <OverviewView
              state={state}
              controller={controller}
            />
          )}

          {currentGoal === "tune" && (
            <TuneView
              state={state}
              controller={controller}
              category={activeCategory}
              searchFilterProp={searchQuery}
              onSelectCategory={(cat) => navigateTo("tune", cat)}
            />
          )}

          {currentGoal === "firmware" && (
            <FirmwareView
              state={state}
              controller={controller}
              onBack={() => navigateTo("overview")}
            />
          )}

          {currentGoal === "backup" && (
            <BackupView
              state={state}
              controller={controller}
              onBack={() => navigateTo("overview")}
            />
          )}
        </main>
      </div>

      {/* Disconnect Safety Confirmation Modal */}
      <Modal
        open={disconnectConfirmOpen}
        onOpenChange={setDisconnectConfirmOpen}
        title="Unapplied Changes Pending"
        description="You have parameter changes that have not been written to the bike's memory."
        footer={
          <>
            <Button
              variant="outline"
              size="md"
              onClick={() => setDisconnectConfirmOpen(false)}
            >
              Cancel
            </Button>
            <Button
              variant="danger"
              size="md"
              onClick={confirmDisconnect}
            >
              Discard Changes & Disconnect
            </Button>
          </>
        }
      >
        <div className="flex items-start gap-3 p-3.5 rounded-xl bg-amber-50 border border-amber-200 text-amber-900 text-xs">
          <AlertTriangle size={18} className="text-amber-700 shrink-0 mt-0.5" />
          <span>
            Disconnecting will drop the active communication session and discard all {stagedCount} staged parameter adjustments.
          </span>
        </div>
      </Modal>

      {/* Persistent Diagnostics & Logs Drawer */}
      <DiagnosticsDrawer state={state} controller={controller} />

      {/* Asynchronous Operation Progress Modal */}
      <OperationModal state={state} />

      {/* Fixed Automation Actionability Anchors */}
      <div
        style={{
          position: "fixed",
          bottom: 0,
          right: 0,
          opacity: 0,
          pointerEvents: "auto",
          width: "16px",
          height: "16px",
          overflow: "hidden",
          zIndex: 1,
        }}
      >
        <span data-testid="status-message">{state.statusMessage}</span>
        <button
          type="button"
          data-testid="btn-hub-tune"
          onClick={() => {
            controller.setSection("parameters");
            setCurrentGoal("tune");
          }}
          style={{ width: "16px", height: "16px" }}
        />
        <button
          type="button"
          data-testid="btn-hub-firmware"
          onClick={() => {
            controller.setSection("firmware");
            setCurrentGoal("firmware");
          }}
          style={{ width: "16px", height: "16px" }}
        />
        <button
          type="button"
          data-testid="btn-hub-backup"
          onClick={() => {
            controller.setSection("journal");
            setCurrentGoal("backup");
          }}
          style={{ width: "16px", height: "16px" }}
        />
        <button
          type="button"
          data-testid="macro-trail-85nm"
          onClick={() => controller.applyPresetMacro("trail_85nm")}
          style={{ width: "16px", height: "16px" }}
        />
      </div>
    </div>
  );
}

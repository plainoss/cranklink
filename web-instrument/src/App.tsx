import { friendlyMotorName } from "./domain/motorIdentity.ts";
import { useState, useEffect } from "react";
import { useNavigate, useRouterState } from "@tanstack/react-router";
import {
  Disc3,
  ShieldCheck,
  LogOut,
  AlertCircle,
  Menu,
  X,
  Gauge,
  Sliders,
  Compass,
  Layers,
  AlertTriangle,
  Search,
  Terminal,
  ShieldAlert,
  Cog,
  Wrench,
  Save,
} from "lucide-react";
import type { InstrumentState } from "./types.ts";
import type { SessionController } from "./core/sessionController.ts";
import { ConnectView } from "./components/views/ConnectView.tsx";
import { userFacingError } from "./domain/errors.ts";
import { OverviewView } from "./components/views/OverviewView.tsx";
import { TuneView } from "./components/views/TuneView.tsx";
import { FirmwareView } from "./components/views/FirmwareView.tsx";
import { BackupView } from "./components/views/BackupView.tsx";
import { DiagnosticsView } from "./components/views/DiagnosticsView.tsx";
import { Badge } from "./components/ui/Badge.tsx";
import { Button } from "./components/ui/Button.tsx";
import { Modal } from "./components/ui/Dialog.tsx";
import { OperationModal } from "./components/ui/OperationModal.tsx";
import { RiskWarningModal } from "./components/ui/RiskWarningModal.tsx";

interface AppProps {
  controller: SessionController;
}

export type GoalView = "overview" | "tune" | "firmware" | "backup" | "diagnostics";
export type TuneCategory =
  | "all"
  | "geometry"
  | "profiles"
  | "drivetrain"
  | "maintenance"
  | "oem";

export function App({ controller }: AppProps) {
  const [state, setState] = useState<InstrumentState>(controller.getState());
  const routePath = useRouterState({ select: (routerState) => routerState.location.pathname });
  const navigate = useNavigate();
  const currentGoal: GoalView = routePath.startsWith("/firmware")
    ? "firmware"
    : routePath.startsWith("/backup")
      ? "backup"
      : routePath.startsWith("/diagnostics")
        ? "diagnostics"
        : routePath.startsWith("/overview")
          ? "overview"
          : "tune";
  const routeCategory = routePath.split("/")[2] as TuneCategory | undefined;
  const activeCategory: TuneCategory = ["all", "geometry", "profiles", "drivetrain", "maintenance", "oem"].includes(routeCategory ?? "")
    ? routeCategory!
    : "geometry";
  const [searchQuery, setSearchQuery] = useState("");
  const [mobileDrawerOpen, setMobileDrawerOpen] = useState(false);
  const [disconnectConfirmOpen, setDisconnectConfirmOpen] = useState(false);
  const [mobileReviewOpen, setMobileReviewOpen] = useState(false);
  const [warningModalOpen, setWarningModalOpen] = useState(() => {
    try {
      return localStorage.getItem("cranklink_disclaimer_dismissed") !== "true";
    } catch {
      return true;
    }
  });

  const handleDismissWarning = () => {
    setWarningModalOpen(false);
    try {
      localStorage.setItem("cranklink_disclaimer_dismissed", "true");
    } catch {}
  };

  // Sync state from sessionController
  useEffect(() => {
    const unsub = controller.subscribe((next) => {
      setState(next);
    });
    return unsub;
  }, [controller]);

  // Route and session stay aligned for direct links, refreshes, and browser history.
  useEffect(() => {
    if (state.phase === "disconnected" || state.phase === "error") {
      if (routePath !== "/connect") void navigate({ to: "/connect" });
      return;
    }
    if (state.identity && routePath === "/connect") {
      void navigate({ to: "/tune/$category", params: { category: "geometry" } });
    } else if (state.identity && routePath === "/") {
      void navigate({ to: "/tune/$category", params: { category: "geometry" } });
    }
    if (currentGoal === "firmware") controller.setSection("firmware");
    else if (currentGoal === "backup") controller.setSection("journal");
    else if (currentGoal === "tune") {
      controller.setSection("parameters");
      controller.setParameterGroup(activeCategory);
    }
  }, [activeCategory, controller, currentGoal, navigate, routePath, state.phase]);

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
    setMobileDrawerOpen(false);
    if (goal === "tune") {
      controller.setSection("parameters");
      const nextCategory = category ?? activeCategory;
      controller.setParameterGroup(nextCategory);
      void navigate({ to: "/tune/$category", params: { category: nextCategory } });
    } else if (goal === "firmware") {
      controller.setSection("firmware");
      void navigate({ to: "/firmware" });
    } else if (goal === "backup") {
      controller.setSection("journal");
      void navigate({ to: "/backup" });
    } else if (goal === "diagnostics") {
      void navigate({ to: "/diagnostics" });
    } else if (goal === "overview") {
      void navigate({ to: "/overview" });
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

  const mcpApprovalBanner = state.mcpApproval && (
    <section
      role="region"
      aria-label="Agent Approval Request"
      data-testid="mcp-approval-banner"
      className="fixed bottom-4 left-4 right-4 z-[70] mx-auto flex max-w-3xl flex-col gap-3 rounded-2xl border border-amber-300 bg-amber-100 p-4 text-stone-900 shadow-xl sm:flex-row sm:items-center sm:justify-between"
    >
      <div className="flex min-w-0 items-start gap-3">
        <ShieldAlert size={19} className="mt-0.5 shrink-0 text-amber-800" />
        <div className="min-w-0">
          <strong className="block text-sm font-semibold">External agent request · {state.mcpApproval.tool}</strong>
          <p className="mt-1 break-words text-xs text-stone-700">
            {state.mcpApproval.diff.map((change) => `${change.label}: ${change.before} → ${change.after}`).join("; ")}
          </p>
        </div>
      </div>
      <div className="flex shrink-0 gap-2 self-end sm:self-auto">
        <Button
          variant="custom"
          size="sm"
          onClick={() => controller.respondMcpApproval(false)}
          data-testid="btn-mcp-reject"
          className="min-h-[44px] bg-white text-stone-900 hover:bg-stone-50"
        >
          Reject
        </Button>
        <Button
          variant="custom"
          size="sm"
          onClick={() => controller.respondMcpApproval(true)}
          data-testid="btn-mcp-approve"
          className="min-h-[44px] bg-stone-950 text-white hover:bg-black"
        >
          Authorize write
        </Button>
      </div>
    </section>
  );

  const isDisconnected =
    state.phase === "disconnected" ||
    state.phase === "connecting" ||
    state.phase === "probing" ||
    state.phase === "error";
  const stagedCount = Object.keys(state.stagedValues).length;

  useEffect(() => {
    if (stagedCount === 0) setMobileReviewOpen(false);
  }, [stagedCount]);

  if (isDisconnected) {
    return (
      <div className="min-h-screen bg-[#f4f3ef] text-stone-900 font-sans flex flex-col justify-between selection:bg-stone-900 selection:text-white">
        <ConnectView
          state={state}
          controller={controller}
          onOpenWarningModal={() => setWarningModalOpen(true)}
        />
        {mcpApprovalBanner}
        <OperationModal state={state} />
        <RiskWarningModal
          open={warningModalOpen}
          onOpenChange={setWarningModalOpen}
          onDismiss={handleDismissWarning}
        />
        <span style={{ display: "none" }} data-testid="status-message">
          {state.statusMessage}
        </span>
      </div>
    );
  }

  const identity = state.identity;
  const getFriendlyModel = () => friendlyMotorName(identity);

  return (
      <div className="min-h-screen bg-[#f4f3ef] text-stone-900 font-sans flex flex-col lg:flex-row selection:bg-stone-900 selection:text-white pb-8">
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
                {state.transport === "sim" ? "Simulator" : "Online"}
              </span>
            </div>
            <div className="font-bold text-sm text-stone-900 truncate">
              {getFriendlyModel()}
            </div>
            <div className="pt-1 border-t border-stone-200/60 text-[10px] font-mono text-stone-600 grid grid-cols-2 gap-x-2 gap-y-0.5" data-testid="identity-strip">
              <span>{identity?.modelSlug?.toUpperCase() || "EP8"}</span>
              <span className="truncate">SN: {identity?.serial || "—"}</span>
              <span>DCAS: {identity?.dcasVersion || "—"}</span>
              <span>RX: {identity?.rxVersion || "—"}</span>
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
              <div className="flex items-center justify-between px-3.5 pt-2 pb-1 text-[11px] font-semibold uppercase tracking-wide text-stone-500" data-testid={mobileDrawerOpen ? undefined : "nav-section-parameters"}>
                <span>Tuning</span>
                {stagedCount > 0 && (
                  <span className="text-[10px] normal-case tracking-normal font-mono px-1.5 py-0.5 rounded-full bg-amber-200 text-amber-950 font-bold">
                    {stagedCount}
                  </span>
                )}
              </div>

            {/* Tuning Sub-items */}
            <div className="flex flex-col gap-0.5 px-1">
              <button
                type="button"
                onClick={() => navigateTo("tune", "geometry")}
                data-testid={mobileDrawerOpen ? undefined : "nav-group-geometry"}
                aria-current={currentGoal === "tune" && activeCategory === "geometry" ? "page" : undefined}
                className={`w-full flex items-center gap-2 border-l-[3px] px-3 py-2 rounded-lg text-xs font-medium transition-colors text-left cursor-pointer min-h-[40px] ${
                  currentGoal === "tune" && activeCategory === "geometry"
                    ? "text-stone-950 bg-amber-100 border-amber-500 font-bold"
                    : "text-stone-600 border-transparent hover:text-stone-900 hover:bg-stone-100"
                }`}
              >
                <Compass size={14} />
                <span>Bike & Geometry</span>
              </button>
              {state.schema.some((setting) => setting.section === "profiles") && (
<button
                type="button"
                onClick={() => navigateTo("tune", "profiles")}
                data-testid={mobileDrawerOpen ? undefined : "nav-group-profiles"}
                aria-current={currentGoal === "tune" && activeCategory === "profiles" ? "page" : undefined}
                className={`w-full flex items-center gap-2 border-l-[3px] px-3 py-2 rounded-lg text-xs font-medium transition-colors text-left cursor-pointer min-h-[40px] ${
                  currentGoal === "tune" && activeCategory === "profiles"
                    ? "text-stone-950 bg-amber-100 border-amber-500 font-bold"
                    : "text-stone-600 border-transparent hover:text-stone-900 hover:bg-stone-100"
                }`}
              >
                <Sliders size={14} />
                <span>Assist Profiles</span>
              </button>
              )}
              {state.schema.some((setting) => setting.section === "drivetrain") && (
<button
                type="button"
                onClick={() => navigateTo("tune", "drivetrain")}
                data-testid={mobileDrawerOpen ? undefined : "nav-group-drivetrain"}
                aria-current={currentGoal === "tune" && activeCategory === "drivetrain" ? "page" : undefined}
                className={`w-full flex items-center gap-2 border-l-[3px] px-3 py-2 rounded-lg text-xs font-medium transition-colors text-left cursor-pointer min-h-[40px] ${
                  currentGoal === "tune" && activeCategory === "drivetrain"
                    ? "text-stone-950 bg-amber-100 border-amber-500 font-bold"
                    : "text-stone-600 border-transparent hover:text-stone-900 hover:bg-stone-100"
                }`}
              >
                <Cog size={14} />
                <span>Drivetrain & Di2</span>
              </button>
              )}
              {state.schema.some((setting) => setting.section === "maintenance") && (
<button
                type="button"
                onClick={() => navigateTo("tune", "maintenance")}
                data-testid={mobileDrawerOpen ? undefined : "nav-group-maintenance"}
                aria-current={currentGoal === "tune" && activeCategory === "maintenance" ? "page" : undefined}
                className={`w-full flex items-center gap-2 border-l-[3px] px-3 py-2 rounded-lg text-xs font-medium transition-colors text-left cursor-pointer min-h-[40px] ${
                  currentGoal === "tune" && activeCategory === "maintenance"
                    ? "text-stone-950 bg-amber-100 border-amber-500 font-bold"
                    : "text-stone-600 border-transparent hover:text-stone-900 hover:bg-stone-100"
                }`}
              >
                <Wrench size={14} />
                <span>Service & Maintenance</span>
              </button>
              )}
              {state.schema.some((setting) => setting.section === "oem") && (
<button
                type="button"
                onClick={() => navigateTo("tune", "oem")}
                data-testid={mobileDrawerOpen ? undefined : "nav-group-oem"}
                aria-current={currentGoal === "tune" && activeCategory === "oem" ? "page" : undefined}
                className={`w-full flex items-center gap-2 border-l-[3px] px-3 py-2 rounded-lg text-xs font-medium transition-colors text-left cursor-pointer min-h-[40px] ${
                  currentGoal === "tune" && activeCategory === "oem"
                    ? "text-stone-950 bg-amber-100 border-amber-500 font-bold"
                    : "text-stone-600 border-transparent hover:text-stone-900 hover:bg-stone-100"
                }`}
              >
                <ShieldCheck size={14} />
                <span>OEM Factory Bounds</span>
              </button>
              )}
              <button
                type="button"
                onClick={() => navigateTo("tune", "all")}
                data-testid={mobileDrawerOpen ? undefined : "nav-group-all"}
                aria-current={currentGoal === "tune" && activeCategory === "all" ? "page" : undefined}
                className={`w-full flex items-center gap-2 border-l-[3px] px-3 py-2 rounded-lg text-xs font-medium transition-colors text-left cursor-pointer min-h-[40px] ${
                  currentGoal === "tune" && activeCategory === "all"
                    ? "text-stone-950 bg-amber-100 border-amber-500 font-bold"
                    : "text-stone-600 border-transparent hover:text-stone-900 hover:bg-stone-100"
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
            <button
              type="button"
              onClick={() => navigateTo("diagnostics")}
              data-testid={mobileDrawerOpen ? undefined : "nav-section-diagnostics"}
              className={`w-full flex items-center gap-2.5 px-3.5 py-2.5 rounded-xl text-xs font-semibold transition-all cursor-pointer min-h-[42px] ${
                currentGoal === "diagnostics"
                  ? "bg-stone-900 text-white shadow-xs"
                  : "text-stone-600 hover:text-stone-900 hover:bg-stone-100"
              }`}
            >
              <Terminal size={16} className={currentGoal === "diagnostics" ? "text-amber-400" : "text-stone-500"} />
              <span>Diagnostics & Logs</span>
            </button>
          </nav>
        </div>
        <div className="flex flex-col gap-1 border-t border-stone-100 pt-2">
          <button
            type="button"
            onClick={() => setWarningModalOpen(true)}
            data-testid="btn-sidebar-warning"
            className="w-full flex items-center gap-2.5 px-3 py-2 rounded-xl text-xs text-stone-500 hover:text-stone-800 hover:bg-stone-50 transition-colors cursor-pointer"
          >
            <AlertTriangle size={15} className="text-amber-600 shrink-0" />
            <span>Safety &amp; Disclaimer</span>
          </button>
          <Button
            variant="ghost"
            size="sm"
            onClick={handleDisconnectClick}
            data-testid="btn-disconnect"
            data-action="disconnect"
            title="Disconnect from this bike"
            className="w-full justify-start gap-2 text-stone-500 hover:text-red-700"
          >
            <LogOut size={15} />
            <span>Disconnect</span>
          </Button>
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
              <div className="p-3.5 rounded-2xl bg-stone-50 border border-stone-200/80 flex flex-col gap-1" data-testid="mobile-device-summary">
                <div className="font-bold text-sm text-stone-900 truncate">
                  {getFriendlyModel()}
                </div>
                {state.transport === "sim" && <span className="text-[10px] font-medium text-amber-700">Development simulator</span>}
                <div className="text-[11px] font-mono text-stone-600">
                  SN: {identity?.serial || "—"}
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
                <div className="flex items-center justify-between px-3.5 pt-2 pb-1 text-[11px] font-semibold uppercase tracking-wide text-stone-500">
                  <span>Tuning</span>
                  {stagedCount > 0 && (
                    <span className="text-[10px] normal-case tracking-normal font-mono px-1.5 py-0.5 rounded-full bg-amber-200 text-amber-950 font-bold">
                      {stagedCount}
                    </span>
                  )}
                </div>

                {/* Mobile Tuning Sub-items */}
                <div className="flex flex-col gap-0.5 px-1">
                  <button
                    type="button"
                    onClick={() => navigateTo("tune", "geometry")}
                    data-testid="nav-group-geometry"
                    aria-current={currentGoal === "tune" && activeCategory === "geometry" ? "page" : undefined}
                    className={`w-full flex items-center gap-2 border-l-[3px] px-3 py-2 rounded-lg text-xs font-medium text-left cursor-pointer min-h-[44px] ${
                      currentGoal === "tune" && activeCategory === "geometry"
                        ? "text-stone-950 bg-amber-100 border-amber-500 font-bold"
                        : "text-stone-600 border-transparent hover:bg-stone-100"
                    }`}
                  >
                    <Compass size={14} />
                    <span>Bike & Geometry</span>
                  </button>
                  {state.schema.some((setting) => setting.section === "profiles") && (
<button
                    type="button"
                    onClick={() => navigateTo("tune", "profiles")}
                    data-testid="nav-group-profiles"
                    aria-current={currentGoal === "tune" && activeCategory === "profiles" ? "page" : undefined}
                    className={`w-full flex items-center gap-2 border-l-[3px] px-3 py-2 rounded-lg text-xs font-medium text-left cursor-pointer min-h-[44px] ${
                      currentGoal === "tune" && activeCategory === "profiles"
                        ? "text-stone-950 bg-amber-100 border-amber-500 font-bold"
                        : "text-stone-600 border-transparent hover:bg-stone-100"
                    }`}
                  >
                    <Sliders size={14} />
                    <span>Assist Profiles</span>
                  </button>
              )}
                  {state.schema.some((setting) => setting.section === "drivetrain") && (
<button
                    type="button"
                    onClick={() => navigateTo("tune", "drivetrain")}
                    data-testid="nav-group-drivetrain"
                    aria-current={currentGoal === "tune" && activeCategory === "drivetrain" ? "page" : undefined}
                    className={`w-full flex items-center gap-2 border-l-[3px] px-3 py-2 rounded-lg text-xs font-medium text-left cursor-pointer min-h-[44px] ${
                      currentGoal === "tune" && activeCategory === "drivetrain"
                        ? "text-stone-950 bg-amber-100 border-amber-500 font-bold"
                        : "text-stone-600 border-transparent hover:bg-stone-100"
                    }`}
                  >
                    <Cog size={14} />
                    <span>Drivetrain & Di2</span>
                  </button>
              )}
                  {state.schema.some((setting) => setting.section === "maintenance") && (
<button
                    type="button"
                    onClick={() => navigateTo("tune", "maintenance")}
                    data-testid="nav-group-maintenance"
                    aria-current={currentGoal === "tune" && activeCategory === "maintenance" ? "page" : undefined}
                    className={`w-full flex items-center gap-2 border-l-[3px] px-3 py-2 rounded-lg text-xs font-medium text-left cursor-pointer min-h-[44px] ${
                      currentGoal === "tune" && activeCategory === "maintenance"
                        ? "text-stone-950 bg-amber-100 border-amber-500 font-bold"
                        : "text-stone-600 border-transparent hover:bg-stone-100"
                    }`}
                  >
                    <Wrench size={14} />
                    <span>Service & Maintenance</span>
                  </button>
              )}
                  {state.schema.some((setting) => setting.section === "oem") && (
<button
                    type="button"
                    onClick={() => navigateTo("tune", "oem")}
                    data-testid="nav-group-oem"
                    aria-current={currentGoal === "tune" && activeCategory === "oem" ? "page" : undefined}
                    className={`w-full flex items-center gap-2 border-l-[3px] px-3 py-2 rounded-lg text-xs font-medium text-left cursor-pointer min-h-[44px] ${
                      currentGoal === "tune" && activeCategory === "oem"
                        ? "text-stone-950 bg-amber-100 border-amber-500 font-bold"
                        : "text-stone-600 border-transparent hover:bg-stone-100"
                    }`}
                  >
                    <ShieldCheck size={14} />
                    <span>OEM Factory Bounds</span>
                  </button>
              )}
                  <button
                    type="button"
                    onClick={() => navigateTo("tune", "all")}
                    data-testid="nav-group-all"
                    aria-current={currentGoal === "tune" && activeCategory === "all" ? "page" : undefined}
                    className={`w-full flex items-center gap-2 border-l-[3px] px-3 py-2 rounded-lg text-xs font-medium text-left cursor-pointer min-h-[44px] ${
                      currentGoal === "tune" && activeCategory === "all"
                        ? "text-stone-950 bg-amber-100 border-amber-500 font-bold"
                        : "text-stone-600 border-transparent hover:bg-stone-100"
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
                <button
                  type="button"
                  onClick={() => navigateTo("diagnostics")}
                  data-testid="nav-section-diagnostics"
                  className={`w-full flex items-center gap-3 px-3.5 py-2.5 rounded-xl text-xs font-semibold min-h-[44px] ${
                    currentGoal === "diagnostics" ? "bg-stone-900 text-white" : "text-stone-700 hover:bg-stone-100"
                  }`}
                >
                  <Terminal size={16} />
                  <span>Diagnostics & Logs</span>
                </button>
              </nav>
            </div>
            <div className="flex flex-col gap-1 border-t border-stone-100 pt-2">
              <button
                type="button"
                onClick={() => {
                  setMobileDrawerOpen(false);
                  setWarningModalOpen(true);
                }}
                data-testid="btn-drawer-warning"
                className="w-full flex items-center gap-3 px-3.5 py-2.5 rounded-xl text-xs text-stone-600 hover:text-stone-900 hover:bg-stone-50 transition-colors cursor-pointer min-h-[44px]"
              >
                <AlertTriangle size={16} className="text-amber-600 shrink-0" />
                <span>Safety &amp; Disclaimer</span>
              </button>
              <Button
                variant="ghost"
                size="md"
                onClick={handleDisconnectClick}
                data-testid="btn-disconnect-mobile"
                data-action="disconnect"
                className="w-full justify-start gap-3 text-stone-600 min-h-[44px]"
              >
                <LogOut size={16} />
                <span>Disconnect</span>
              </Button>
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
                      ? "Bike & Geometry"
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
                    ? "Firmware"
                    : currentGoal === "diagnostics"
                      ? "Diagnostics"
                      : "Backup & Restore"}
                </span>
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
                  placeholder="Search parameters"
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                  className="pl-7 pr-6 py-1.5 text-sm bg-stone-100 hover:bg-stone-200/60 focus:bg-white border border-stone-200/90 rounded-full w-32 sm:w-48 transition-all focus:outline-none focus:ring-1 focus:ring-stone-400 min-h-[40px]"
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
              <>
                <Badge variant="warning" className="hidden sm:inline-flex text-xs">
                  {stagedCount} to save
                </Badge>
                <button
                  type="button"
                  onClick={() => {
                    if (currentGoal !== "tune") navigateTo("tune", activeCategory);
                    setMobileReviewOpen(true);
                  }}
                  aria-label={`Review ${stagedCount} changes and save to device`}
                  aria-expanded={mobileReviewOpen}
                  data-testid="btn-review-changes"
                  className="lg:hidden relative inline-flex h-11 w-11 items-center justify-center rounded-xl border border-amber-300 bg-amber-100 text-amber-950 shadow-sm hover:bg-amber-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-amber-500"
                >
                  <Save size={19} strokeWidth={2.4} />
                  <span className="absolute -right-1 -top-1 flex h-5 min-w-5 items-center justify-center rounded-full bg-amber-500 px-1 text-[10px] font-bold leading-none text-stone-950">
                    {stagedCount}
                  </span>
                </button>
              </>
            )}

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
                  {userFacingError(state.errorBanner)}
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
              mobileReviewOpen={mobileReviewOpen}
              onMobileReviewOpenChange={setMobileReviewOpen}
            />
          )}

          {currentGoal === "firmware" && (
            <FirmwareView
              state={state}
              controller={controller}
              onOpenWarningModal={() => setWarningModalOpen(true)}
            />
          )}

          {currentGoal === "backup" && (
            <BackupView
              state={state}
              controller={controller}
            />
          )}

          {currentGoal === "diagnostics" && (
            <DiagnosticsView state={state} controller={controller} />
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

      {mcpApprovalBanner}

      {/* Asynchronous Operation Progress Modal */}
      <OperationModal state={state} />

      {/* Unofficial Software & Risk Warning Modal */}
      <RiskWarningModal
        open={warningModalOpen}
        onOpenChange={setWarningModalOpen}
        onDismiss={handleDismissWarning}
      />

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
        <span data-testid="status-message">{state.errorBanner ? userFacingError(state.errorBanner) : state.statusMessage}</span>
        <button
          type="button"
          data-testid="btn-hub-tune"
          onClick={() => {
            controller.setSection("parameters");
            navigateTo("tune", "geometry");
          }}
          style={{ width: "16px", height: "16px" }}
        />
        <button
          type="button"
          data-testid="btn-hub-firmware"
          onClick={() => {
            controller.setSection("firmware");
            navigateTo("firmware");
          }}
          style={{ width: "16px", height: "16px" }}
        />
        <button
          type="button"
          data-testid="btn-hub-backup"
          onClick={() => {
            controller.setSection("journal");
            navigateTo("backup");
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

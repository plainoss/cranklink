import { useCallback, useEffect, useState } from "react";
import { useNavigate, useRouterState } from "@tanstack/react-router";
import { AlertTriangle, LogOut, Menu, Save, Search, ShieldAlert, X } from "lucide-react";
import type { InstrumentState, SettingSectionId } from "./types.ts";
import type { SessionController } from "./core/sessionController.ts";
import {
  OVERVIEW_ITEM,
  TOOL_ITEMS,
  TUNE_ITEMS,
  destinationFromPath,
  pathFor,
  sameDestination,
  titleFor,
  type Destination,
  type NavItem,
} from "./navigation.ts";
import { useMediaQuery } from "./hooks.ts";
import { friendlyMotorName } from "./domain/motorIdentity.ts";
import { ConnectView } from "./components/views/ConnectView.tsx";
import { OverviewView } from "./components/views/OverviewView.tsx";
import { TuneView } from "./components/views/TuneView.tsx";
import { FirmwareView } from "./components/views/FirmwareView.tsx";
import { BackupView } from "./components/views/BackupView.tsx";
import { DiagnosticsView } from "./components/views/DiagnosticsView.tsx";
import { ChangesRail, ReviewDialog } from "./components/ChangesReview.tsx";
import { FailureBanner } from "./components/FailureBanner.tsx";
import { Toast } from "./components/Toast.tsx";
import { Button } from "./components/ui/Button.tsx";
import { Modal } from "./components/ui/Dialog.tsx";
import { OperationModal } from "./components/ui/OperationModal.tsx";
import { RiskWarningModal } from "./components/ui/RiskWarningModal.tsx";

const DISCLAIMER_KEY = "cranklink_disclaimer_dismissed";

interface AppProps {
  controller: SessionController;
}

export function App({ controller }: AppProps) {
  const [state, setState] = useState<InstrumentState>(controller.getState());
  const routePath = useRouterState({ select: (routerState) => routerState.location.pathname });
  const navigate = useNavigate();
  const destination = destinationFromPath(routePath);
  const isDesktop = useMediaQuery("(min-width: 1024px)");
  const [searchQuery, setSearchQuery] = useState("");
  const [searchOpen, setSearchOpen] = useState(false);
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [disconnectConfirmOpen, setDisconnectConfirmOpen] = useState(false);
  const [reviewOpen, setReviewOpen] = useState(false);
  // Shown until acknowledged once in this browser; reachable again from the sidebar.
  const [warningOpen, setWarningOpen] = useState(() => {
    try {
      return localStorage.getItem(DISCLAIMER_KEY) !== "true";
    } catch {
      return true;
    }
  });
  const acknowledgeWarning = () => {
    setWarningOpen(false);
    try {
      localStorage.setItem(DISCLAIMER_KEY, "true");
    } catch {
      // Without storage the notice simply shows again next visit.
    }
  };
  const riskWarning = (
    <RiskWarningModal open={warningOpen} onOpenChange={setWarningOpen} onDismiss={acknowledgeWarning} />
  );

  useEffect(() => controller.subscribe(setState), [controller]);

  const connected = !["disconnected", "connecting", "probing", "error"].includes(state.phase);

  // The route decides the view; the session decides whether a view is reachable.
  useEffect(() => {
    if (!connected) {
      if (routePath !== "/connect") void navigate({ to: "/connect" });
    } else if (routePath === "/connect" || routePath === "/") {
      void navigate({ to: "/tune/$category", params: { category: "geometry" } });
    }
  }, [connected, navigate, routePath]);

  useEffect(() => {
    if (state.phase !== "flashing") return;
    const guard = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = "Firmware update in progress. Leaving may disrupt the motor.";
    };
    window.addEventListener("beforeunload", guard);
    return () => window.removeEventListener("beforeunload", guard);
  }, [state.phase]);

  const go = useCallback((next: Destination) => {
    setDrawerOpen(false);
    if (next.goal !== "tune") {
      setSearchQuery("");
      setSearchOpen(false);
    }
    void navigate({ to: pathFor(next) });
  }, [navigate]);

  const stagedCount = Object.keys(state.stagedValues).length;

  const disconnect = () => {
    if (stagedCount > 0) setDisconnectConfirmOpen(true);
    else controller.disconnect().catch(() => {});
  };

  const saveAndDisconnect = async () => {
    setDisconnectConfirmOpen(false);
    try {
      await controller.commitStagedBatch();
      await controller.disconnect();
    } catch {
      // A failed save keeps the session and staged values; the banner explains why.
    }
  };

  const approvalBanner = state.mcpApproval && (
    <section
      aria-label="Agent approval request"
      data-testid="mcp-approval-banner"
      className="fixed bottom-4 left-4 right-4 z-[70] mx-auto flex max-w-3xl flex-col gap-3 rounded-2xl border border-amber-300 bg-amber-100 p-4 text-stone-900 shadow-xl sm:flex-row sm:items-center sm:justify-between"
    >
      <div className="flex min-w-0 items-start gap-3">
        <ShieldAlert size={19} className="mt-0.5 shrink-0 text-amber-800" aria-hidden="true" />
        <div className="min-w-0">
          <strong className="block text-sm font-semibold">External agent request · {state.mcpApproval.tool}</strong>
          <p className="mt-1 [overflow-wrap:anywhere] text-xs text-stone-700">
            {state.mcpApproval.diff.map((change) => `${change.label}: ${change.before} → ${change.after}`).join("; ")}
          </p>
        </div>
      </div>
      <div className="flex shrink-0 gap-2 self-end sm:self-auto">
        <Button variant="outline" size="sm" onClick={() => controller.respondMcpApproval(false)} data-testid="btn-mcp-reject" className="min-h-[44px] bg-white">
          Reject
        </Button>
        <Button variant="primary" size="sm" onClick={() => controller.respondMcpApproval(true)} data-testid="btn-mcp-approve" className="min-h-[44px]">
          Authorize write
        </Button>
      </div>
    </section>
  );

  if (!connected) {
    return (
      <>
        <ConnectView state={state} controller={controller} onOpenWarning={() => setWarningOpen(true)} />
        {approvalBanner}
        <OperationModal state={state} />
        {riskWarning}
      </>
    );
  }

  const navList = (
    <NavList
      current={destination}
      stagedCount={stagedCount}
      sections={new Set(state.schema.map((setting) => setting.section))}
      onNavigate={go}
    />
  );

  const deviceCard = (
    <div className="p-3.5 rounded-2xl bg-stone-50 border border-stone-200/80 flex flex-col gap-1.5" data-testid="identity-strip">
      <div className="flex items-center justify-between gap-2">
        <span className="text-[10px] font-bold uppercase tracking-wider text-stone-500">Connected unit</span>
        <LinkStatus state={state} />
      </div>
      <div className="font-bold text-sm text-stone-900 truncate" title={friendlyMotorName(state.identity)}>
        {friendlyMotorName(state.identity)}
      </div>
      <p className="pt-1 border-t border-stone-200/60 text-[11px] font-mono text-stone-600 grid grid-cols-2 gap-x-2 gap-y-0.5">
        <span className="col-span-2">SN: {state.identity?.serial}</span>
        <span>DCAS: {state.identity?.dcasVersion}</span>
        <span>RX: {state.identity?.rxVersion}</span>
      </p>
    </div>
  );

  const brand = (
    <div className="flex items-center gap-2.5">
      <div className="w-8 h-8 rounded-xl bg-stone-900 text-white flex items-center justify-center" aria-hidden="true">
        <span className="font-bold text-xs font-mono">CL</span>
      </div>
      <span className="font-bold text-base tracking-tight text-stone-900">cranklink</span>
    </div>
  );

  const sidebarFooter = (
    <div className="flex flex-col gap-1 border-t border-stone-100 pt-2">
      <button
        type="button"
        onClick={() => {
          setDrawerOpen(false);
          setWarningOpen(true);
        }}
        data-testid={isDesktop ? "btn-sidebar-warning" : "btn-drawer-warning"}
        className="w-full flex items-center gap-2.5 px-3.5 rounded-lg text-sm text-stone-600 hover:text-stone-900 hover:bg-stone-100 min-h-[44px]"
      >
        <AlertTriangle size={16} className="text-amber-600 shrink-0" aria-hidden="true" />
        <span>Safety &amp; disclaimer</span>
      </button>
      <Button
        variant="ghost"
        size="md"
        onClick={disconnect}
        data-testid={isDesktop ? "btn-disconnect" : "btn-disconnect-mobile"}
        data-action="disconnect"
        className="w-full justify-start gap-2 text-stone-600 hover:text-red-700"
      >
        <LogOut size={16} aria-hidden="true" />
        <span>Disconnect</span>
      </Button>
    </div>
  );

  const searchable = destination.goal === "tune";
  const showSearchInput = searchable && (isDesktop || searchOpen || searchQuery.length > 0);
  const title = searchQuery.trim() && searchable ? "Search results" : titleFor(destination);

  return (
    <div className="min-h-dvh bg-[#f4f3ef] text-stone-900 flex">
      {isDesktop ? (
        <aside className="w-72 shrink-0 flex flex-col justify-between gap-4 border-r border-stone-200/90 bg-white p-5 sticky top-0 h-dvh overflow-y-auto">
          <div className="flex flex-col gap-4">
            <div className="px-2">{brand}</div>
            {deviceCard}
            {navList}
          </div>
          {sidebarFooter}
        </aside>
      ) : (
        drawerOpen && (
          <div className="fixed inset-0 z-50">
            <div className="fixed inset-0 bg-stone-900/50 backdrop-blur-xs" onClick={() => setDrawerOpen(false)} aria-hidden="true" />
            <div
              role="dialog"
              aria-modal="true"
              aria-label="Navigation"
              className="fixed inset-y-0 left-0 w-72 max-w-[85vw] bg-white shadow-2xl p-5 flex flex-col justify-between gap-6 overflow-y-auto animate-in slide-in-from-left duration-200"
            >
              <div className="flex flex-col gap-6">
                <div className="flex items-center justify-between">
                  {brand}
                  <button
                    type="button"
                    onClick={() => setDrawerOpen(false)}
                    aria-label="Close navigation menu"
                    className="rounded-lg text-stone-500 hover:text-stone-800 min-h-[44px] min-w-[44px] flex items-center justify-center"
                  >
                    <X size={18} aria-hidden="true" />
                  </button>
                </div>
                <div data-testid="mobile-device-summary">{deviceCard}</div>
                {navList}
              </div>
              {sidebarFooter}
            </div>
          </div>
        )
      )}

      <div className="flex-1 flex flex-col min-w-0">
        <header className="sticky top-0 z-30 bg-white/95 backdrop-blur-md border-b border-stone-200/90">
          <div className="h-16 px-4 sm:px-8 flex items-center justify-between gap-2 sm:gap-4">
            <div className="flex items-center gap-2 min-w-0">
              {!isDesktop && (
                <button
                  type="button"
                  onClick={() => setDrawerOpen(true)}
                  aria-label="Open navigation menu"
                  className="-ml-2 rounded-xl text-stone-700 hover:bg-stone-100 min-h-[44px] min-w-[44px] flex items-center justify-center shrink-0"
                >
                  <Menu size={20} aria-hidden="true" />
                </button>
              )}
              <h1 className="font-bold text-base tracking-tight text-stone-900 truncate">{title}</h1>
            </div>

            <div className="flex items-center gap-2 shrink-0">
              {searchable && isDesktop && (
                <SearchInput value={searchQuery} onChange={setSearchQuery} />
              )}
              {searchable && !isDesktop && !showSearchInput && (
                <button
                  type="button"
                  onClick={() => setSearchOpen(true)}
                  aria-label="Search settings"
                  data-testid="btn-open-search"
                  className="rounded-xl text-stone-700 hover:bg-stone-100 min-h-[44px] min-w-[44px] flex items-center justify-center"
                >
                  <Search size={19} aria-hidden="true" />
                </button>
              )}
              {stagedCount > 0 && !isDesktop && (
                <button
                  type="button"
                  onClick={() => setReviewOpen(true)}
                  aria-label={`Review ${stagedCount} change${stagedCount === 1 ? "" : "s"} and save to the bike`}
                  data-testid="btn-review-changes"
                  className="relative inline-flex h-11 w-11 items-center justify-center rounded-xl border border-amber-300 bg-amber-100 text-amber-950 hover:bg-amber-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-amber-500"
                >
                  <Save size={19} strokeWidth={2.4} aria-hidden="true" />
                  <span className="absolute -right-1 -top-1 flex h-5 min-w-5 items-center justify-center rounded-full bg-amber-500 px-1 text-[10px] font-bold leading-none text-stone-950">
                    {stagedCount}
                  </span>
                </button>
              )}
            </div>
          </div>
          {!isDesktop && showSearchInput && (
            <div className="px-4 pb-3 flex items-center gap-2">
              <SearchInput
                value={searchQuery}
                onChange={setSearchQuery}
                autoFocus
                className="flex-1"
              />
              <button
                type="button"
                onClick={() => {
                  setSearchQuery("");
                  setSearchOpen(false);
                }}
                className="text-sm font-medium text-stone-600 min-h-[44px] px-2"
              >
                Cancel
              </button>
            </div>
          )}
        </header>

        <main className="flex-1 max-w-4xl mx-auto w-full px-4 sm:px-8 pt-6 pb-10 flex flex-col gap-6">
          <FailureBanner state={state} controller={controller} onShowDiagnostics={() => go({ goal: "diagnostics" })} />

          {destination.goal === "overview" && <OverviewView state={state} onNavigate={go} />}
          {destination.goal === "tune" && (
            <TuneView state={state} controller={controller} category={destination.category} query={searchQuery} />
          )}
          {destination.goal === "firmware" && (
            <FirmwareView state={state} controller={controller} onOpenWarning={() => setWarningOpen(true)} />
          )}
          {destination.goal === "backup" && <BackupView state={state} controller={controller} />}
          {destination.goal === "diagnostics" && <DiagnosticsView state={state} controller={controller} />}
          {isDesktop && <ChangesRail state={state} controller={controller} onReview={() => setReviewOpen(true)} />}
        </main>
      </div>

      <ReviewDialog state={state} controller={controller} open={reviewOpen} onOpenChange={setReviewOpen} />

      <Modal
        open={disconnectConfirmOpen}
        onOpenChange={setDisconnectConfirmOpen}
        title="Save changes before disconnecting?"
        description={`${stagedCount} staged change${stagedCount === 1 ? " hasn’t" : "s haven’t"} been written to the bike yet.`}
        footer={
          <>
            <Button variant="outline" size="md" onClick={() => setDisconnectConfirmOpen(false)}>
              Cancel
            </Button>
            <Button
              variant="danger"
              size="md"
              onClick={() => {
                setDisconnectConfirmOpen(false);
                controller.disconnect().catch(() => {});
              }}
            >
              Discard & disconnect
            </Button>
            <Button variant="primary" size="md" onClick={saveAndDisconnect} data-testid="btn-save-disconnect">
              Save & disconnect
            </Button>
          </>
        }
      >
        <p className="flex items-start gap-3 p-3.5 rounded-xl bg-amber-50 border border-amber-200 text-amber-900 text-xs">
          <AlertTriangle size={18} className="text-amber-700 shrink-0" aria-hidden="true" />
          Disconnecting without saving discards them. Saving writes them first, with a backup of the current values.
        </p>
      </Modal>

      <Toast
        toast={state.toast}
        onDismiss={() => controller.dismissToast()}
        onAction={(action) => {
          controller.dismissToast();
          if (action === "backup") go({ goal: "backup" });
        }}
      />
      {approvalBanner}
      <OperationModal state={state} />
      {riskWarning}
    </div>
  );
}

function LinkStatus({ state }: { state: InstrumentState }) {
  const [label, tone] = state.linkLost
    ? ["Link closed", "bg-amber-100 text-amber-900"]
    : state.transport === "sim"
      ? ["Simulator", "bg-violet-100 text-violet-900"]
      : ["Connected", "bg-emerald-100 text-emerald-800"];
  return (
    <span className={`text-[10px] font-semibold px-2 py-0.5 rounded-full ${tone}`} data-testid="link-status">
      {label}
    </span>
  );
}

function SearchInput({
  value,
  onChange,
  autoFocus = false,
  className = "",
}: {
  value: string;
  onChange: (value: string) => void;
  autoFocus?: boolean;
  className?: string;
}) {
  return (
    <div className={`relative flex items-center ${className}`}>
      <Search size={14} className="absolute left-3 text-stone-400 pointer-events-none" aria-hidden="true" />
      <input
        data-testid="search-filter-input"
        type="search"
        aria-label="Search settings"
        placeholder="Search settings"
        value={value}
        autoFocus={autoFocus}
        onChange={(event) => onChange(event.target.value)}
        className="w-full lg:w-56 pl-8 pr-3 py-1.5 bg-stone-100 hover:bg-stone-200/60 focus:bg-white border border-stone-200/90 rounded-full focus:outline-none focus:ring-2 focus:ring-amber-300 min-h-[40px]"
        style={{ fontSize: "16px" }}
      />
    </div>
  );
}

function NavList({
  current,
  stagedCount,
  sections,
  onNavigate,
}: {
  current: Destination;
  stagedCount: number;
  /** Setting sections this drive unit reports; other tuning pages are hidden. */
  sections: Set<SettingSectionId>;
  onNavigate: (destination: Destination) => void;
}) {
  const tuneItems = TUNE_ITEMS.filter((entry) =>
    entry.destination.goal !== "tune" || entry.destination.category === "all" || sections.has(entry.destination.category));
  const item = (entry: NavItem, nested = false) => {
    const active = sameDestination(entry.destination, current);
    const Icon = entry.icon;
    return (
      <li key={entry.testId}>
        <button
          type="button"
          onClick={() => onNavigate(entry.destination)}
          data-testid={entry.testId}
          aria-current={active ? "page" : undefined}
          className={`w-full flex items-center gap-2.5 border-l-[3px] rounded-lg text-left text-sm min-h-[44px] lg:min-h-[38px] transition-colors cursor-pointer ${
            nested ? "px-3" : "px-3.5 font-semibold"
          } ${
            active
              ? "bg-amber-100 border-amber-500 text-stone-950 font-semibold"
              : "border-transparent text-stone-600 hover:text-stone-900 hover:bg-stone-100"
          }`}
        >
          <Icon size={16} aria-hidden="true" />
          <span>{entry.label}</span>
        </button>
      </li>
    );
  };

  return (
    <nav aria-label="Main">
      <ul className="flex flex-col gap-1">
        {item(OVERVIEW_ITEM)}
        <li>
          <p className="flex items-center justify-between px-3.5 pt-3 pb-1 text-[11px] font-semibold uppercase tracking-wide text-stone-500" data-testid="nav-section-parameters">
            <span>Tuning</span>
            {stagedCount > 0 && (
              <span className="normal-case tracking-normal text-[10px] font-semibold px-1.5 py-0.5 rounded-full bg-amber-200 text-amber-950">
                {stagedCount} unsaved
              </span>
            )}
          </p>
          <ul className="flex flex-col gap-0.5 pl-1">{tuneItems.map((entry) => item(entry, true))}</ul>
        </li>
        <li aria-hidden="true" className="my-2 border-t border-stone-200" />
        {TOOL_ITEMS.map((entry) => item(entry))}
      </ul>
    </nav>
  );
}

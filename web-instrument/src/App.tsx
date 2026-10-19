import { useCallback, useEffect, useState } from "react";
import { useNavigate, useRouterState } from "@tanstack/react-router";
import { AlertTriangle, LogOut, Menu, Save, Search, ShieldAlert } from "lucide-react";
import { Alert, Button, Card, Chip, Drawer, Link, SearchField, Separator, Toast, toast } from "@heroui/react";
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
import { ConfirmDialog } from "./components/ui/ConfirmDialog.tsx";
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

  // Completed writes confirm through HeroUI's toast queue.
  useEffect(() => {
    const notice = state.toast;
    if (!notice) return;
    controller.dismissToast();
    toast.success(notice.message, {
      timeout: 8000,
      ...(notice.action === "backup"
        ? { actionProps: { children: "View backups", onPress: () => void navigate({ to: pathFor({ goal: "backup" }) }) } }
        : {}),
    });
  }, [state.toast, controller, navigate]);

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
    <Alert
      status="warning"
      aria-label="Agent approval request"
      data-testid="mcp-approval-banner"
      className="fixed bottom-4 left-4 right-4 z-[70] mx-auto max-w-3xl shadow-xl flex-col sm:flex-row sm:items-center"
    >
      <Alert.Indicator>
        <ShieldAlert size={19} aria-hidden="true" />
      </Alert.Indicator>
      <Alert.Content className="min-w-0">
        <Alert.Title>External agent request · {state.mcpApproval.tool}</Alert.Title>
        <Alert.Description className="[overflow-wrap:anywhere] text-xs">
          {state.mcpApproval.diff.map((change) => `${change.label}: ${change.before} → ${change.after}`).join("; ")}
        </Alert.Description>
      </Alert.Content>
      <div className="flex shrink-0 gap-2 self-end sm:self-auto">
        <Button variant="tertiary" onPress={() => controller.respondMcpApproval(false)} data-testid="btn-mcp-reject">
          Reject
        </Button>
        <Button onPress={() => controller.respondMcpApproval(true)} data-testid="btn-mcp-approve">
          Authorize write
        </Button>
      </div>
    </Alert>
  );

  if (!connected) {
    return (
      <>
        <ConnectView state={state} controller={controller} onOpenWarning={() => setWarningOpen(true)} />
        {approvalBanner}
        <OperationModal state={state} />
        {riskWarning}
        <Toast.Provider />
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
    <Card variant="secondary" className="gap-1.5 p-3.5" data-testid="identity-strip">
      <Card.Header className="flex flex-row items-center justify-between gap-2 p-0">
        <span className="text-[10px] font-bold uppercase tracking-wider text-muted">Connected unit</span>
        <LinkStatus state={state} />
      </Card.Header>
      <Card.Title className="text-sm font-bold truncate" title={friendlyMotorName(state.identity)}>
        {friendlyMotorName(state.identity)}
      </Card.Title>
      <Card.Content className="p-0">
        <Separator />
        <p className="pt-1 text-[11px] font-mono text-muted grid grid-cols-2 gap-x-2 gap-y-0.5">
          <span className="col-span-2">SN: {state.identity?.serial}</span>
          <span>DCAS: {state.identity?.dcasVersion}</span>
          <span>RX: {state.identity?.rxVersion}</span>
        </p>
      </Card.Content>
    </Card>
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
    <div className="flex flex-col gap-1">
      <Separator className="mb-1" />
      <Button
        variant="ghost"
        fullWidth
        onPress={() => {
          setDrawerOpen(false);
          setWarningOpen(true);
        }}
        data-testid={isDesktop ? "btn-sidebar-warning" : "btn-drawer-warning"}
        className="justify-start"
      >
        <AlertTriangle size={16} className="text-warning shrink-0" aria-hidden="true" />
        Safety &amp; disclaimer
      </Button>
      <Button
        variant="ghost"
        fullWidth
        onPress={disconnect}
        data-testid={isDesktop ? "btn-disconnect" : "btn-disconnect-mobile"}
        data-action="disconnect"
        className="justify-start"
      >
        <LogOut size={16} aria-hidden="true" />
        Disconnect
      </Button>
    </div>
  );

  const searchable = destination.goal === "tune";
  const showSearchInput = searchable && (isDesktop || searchOpen || searchQuery.length > 0);
  const title = searchQuery.trim() && searchable ? "Search results" : titleFor(destination);

  return (
    <div className="min-h-dvh bg-background text-foreground flex">
      {isDesktop ? (
        <aside className="w-72 shrink-0 flex flex-col justify-between gap-4 border-r border-separator bg-surface p-5 sticky top-0 h-dvh overflow-y-auto">
          <div className="flex flex-col gap-4">
            <div className="px-2">{brand}</div>
            {deviceCard}
            {navList}
          </div>
          {sidebarFooter}
        </aside>
      ) : (
        <Drawer>
          <Drawer.Backdrop isOpen={drawerOpen} onOpenChange={setDrawerOpen}>
            <Drawer.Content placement="left" className="w-72 max-w-[85vw]">
              <Drawer.Dialog aria-label="Navigation">
                <Drawer.Header className="flex flex-row items-center justify-between">
                  <Drawer.Heading>{brand}</Drawer.Heading>
                  <Drawer.CloseTrigger aria-label="Close navigation menu" />
                </Drawer.Header>
                <Drawer.Body className="flex flex-col gap-6">
                  <div data-testid="mobile-device-summary">{deviceCard}</div>
                  {navList}
                </Drawer.Body>
                <Drawer.Footer className="flex-col items-stretch">{sidebarFooter}</Drawer.Footer>
              </Drawer.Dialog>
            </Drawer.Content>
          </Drawer.Backdrop>
        </Drawer>
      )}

      <div className="flex-1 flex flex-col min-w-0">
        <header className="sticky top-0 z-30 bg-surface/95 backdrop-blur-md border-b border-separator">
          <div className="h-16 px-4 sm:px-8 flex items-center justify-between gap-2 sm:gap-4">
            <div className="flex items-center gap-2 min-w-0">
              {!isDesktop && (
                <Button variant="ghost" isIconOnly onPress={() => setDrawerOpen(true)} aria-label="Open navigation menu" className="-ml-2 shrink-0">
                  <Menu size={20} aria-hidden="true" />
                </Button>
              )}
              <h1 className="font-bold text-base tracking-tight truncate">{title}</h1>
            </div>

            <div className="flex items-center gap-2 shrink-0">
              {searchable && isDesktop && (
                <SearchInput value={searchQuery} onChange={setSearchQuery} />
              )}
              {searchable && !isDesktop && !showSearchInput && (
                <Button variant="ghost" isIconOnly onPress={() => setSearchOpen(true)} aria-label="Search settings" data-testid="btn-open-search">
                  <Search size={19} aria-hidden="true" />
                </Button>
              )}
              {stagedCount > 0 && !isDesktop && (
                <Button
                  onPress={() => setReviewOpen(true)}
                  aria-label={`Review ${stagedCount} change${stagedCount === 1 ? "" : "s"} and save to the bike`}
                  data-testid="btn-review-changes"
                  className="bg-amber-400 text-stone-950 hover:bg-amber-300"
                >
                  <Save size={18} strokeWidth={2.4} aria-hidden="true" />
                  {stagedCount}
                </Button>
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
              <Button
                variant="ghost"
                onPress={() => {
                  setSearchQuery("");
                  setSearchOpen(false);
                }}
              >
                Cancel
              </Button>
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

      <ConfirmDialog
        isOpen={disconnectConfirmOpen}
        onOpenChange={setDisconnectConfirmOpen}
        title="Save changes before disconnecting?"
        status="warning"
        actions={[
          {
            label: "Discard & disconnect",
            variant: "danger",
            onPress: () => {
              setDisconnectConfirmOpen(false);
              controller.disconnect().catch(() => {});
            },
          },
          { label: "Save & disconnect", onPress: saveAndDisconnect, "data-testid": "btn-save-disconnect" },
        ]}
      >
        <p className="text-sm text-muted">
          {stagedCount} staged change{stagedCount === 1 ? " hasn’t" : "s haven’t"} been written to the bike yet. Disconnecting
          without saving discards them. Saving writes them first, with a backup of the current values.
        </p>
      </ConfirmDialog>

      {approvalBanner}
      <OperationModal state={state} />
      {riskWarning}
      <Toast.Provider />
    </div>
  );
}

function LinkStatus({ state }: { state: InstrumentState }) {
  const [label, color] = state.linkLost
    ? (["Link closed", "warning"] as const)
    : state.transport === "sim"
      ? (["Simulator", "accent"] as const)
      : (["Connected", "success"] as const);
  return (
    <Chip size="sm" variant="soft" color={color} data-testid="link-status">
      {label}
    </Chip>
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
    <SearchField aria-label="Search settings" value={value} onChange={onChange} autoFocus={autoFocus} className={`min-w-0 lg:w-56 ${className}`}>
      <SearchField.Group className="min-w-0">
        <SearchField.SearchIcon />
        <SearchField.Input placeholder="Search settings" data-testid="search-filter-input" style={{ fontSize: "16px" }} />
        <SearchField.ClearButton />
      </SearchField.Group>
    </SearchField>
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
        <Link
          href={`#${pathFor(entry.destination)}`}
          onPress={() => onNavigate(entry.destination)}
          data-testid={entry.testId}
          aria-current={active ? "page" : undefined}
          className={`w-full flex items-center gap-2.5 border-l-[3px] rounded-lg text-sm min-h-[44px] lg:min-h-[38px] no-underline transition-colors ${
            nested ? "px-3" : "px-3.5 font-semibold"
          } ${
            active
              ? "bg-warning-soft border-warning text-foreground font-semibold"
              : "border-transparent text-muted hover:text-foreground hover:bg-surface-secondary"
          }`}
        >
          <Icon size={16} aria-hidden="true" />
          <span>{entry.label}</span>
        </Link>
      </li>
    );
  };

  return (
    <nav aria-label="Main">
      <ul className="flex flex-col gap-1">
        {item(OVERVIEW_ITEM)}
        <li>
          <p className="flex items-center justify-between px-3.5 pt-3 pb-1 text-[11px] font-semibold uppercase tracking-wide text-muted" data-testid="nav-section-parameters">
            <span>Tuning</span>
            {stagedCount > 0 && (
              <Chip size="sm" color="warning" variant="soft" className="normal-case tracking-normal">
                {stagedCount} unsaved
              </Chip>
            )}
          </p>
          <ul className="flex flex-col gap-0.5 pl-1">{tuneItems.map((entry) => item(entry, true))}</ul>
        </li>
        <li aria-hidden="true">
          <Separator className="my-2" />
        </li>
        {TOOL_ITEMS.map((entry) => item(entry))}
      </ul>
    </nav>
  );
}

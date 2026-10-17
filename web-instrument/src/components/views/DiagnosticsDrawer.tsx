import { h } from "preact";
import { useState } from "preact/hooks";
import { Terminal, ChevronUp, ChevronDown, Check, X, ShieldAlert } from "lucide-preact";
import type { InstrumentState } from "../../types.ts";
import type { SessionController } from "../../core/sessionController.ts";
import { Button } from "../ui/Button.tsx";

interface DiagnosticsDrawerProps {
  state: InstrumentState;
  controller: SessionController;
}

export function DiagnosticsDrawer({ state, controller }: DiagnosticsDrawerProps) {
  const [cmdDraft, setCmdDraft] = useState("");
  const expanded = state.consoleExpanded;

  const handleSubmit = (e: Event) => {
    e.preventDefault();
    if (!cmdDraft.trim()) return;
    const cmd = cmdDraft;
    setCmdDraft("");
    controller.runRawCommand(cmd).catch(() => {});
  };

  return (
    <div className="fixed bottom-0 left-0 right-0 z-30 flex flex-col pointer-events-none">
      <div className="max-w-4xl mx-auto w-full px-4 flex flex-col pointer-events-auto">
        {/* MCP Approval Banner (Human-In-The-Loop) */}
        {state.mcpApproval && (
          <div
            role="region"
            aria-label="Agent Approval Request"
            data-testid="mcp-approval-banner"
            className="mb-3 p-4 rounded-xl bg-amber-500 text-stone-950 shadow-xl border border-amber-400 flex flex-col sm:flex-row sm:items-center justify-between gap-4 animate-in slide-in-from-bottom-2 duration-150 banner banner-mcp"
          >
            <div className="flex items-start gap-3">
              <ShieldAlert size={20} className="shrink-0 mt-0.5 text-stone-950" />
              <div>
                <strong className="block text-xs font-bold uppercase tracking-wider">
                  External Agent Request ({state.mcpApproval.tool}):
                </strong>
                <p className="text-xs font-mono font-semibold mt-0.5">
                  {state.mcpApproval.diff
                    .map((d) => `${d.label}: ${d.before} → ${d.after}`)
                    .join("; ")}
                </p>
              </div>
            </div>

            <div className="flex items-center gap-2 self-end sm:self-center shrink-0">
              <Button
                variant="secondary"
                size="sm"
                onClick={() => controller.respondMcpApproval(false)}
                data-testid="btn-mcp-reject"
                className="bg-white/80 hover:bg-white text-stone-900 text-xs"
              >
                Reject
              </Button>
              <Button
                variant="primary"
                size="sm"
                onClick={() => controller.respondMcpApproval(true)}
                data-testid="btn-mcp-approve"
                className="bg-stone-950 hover:bg-black text-white text-xs font-semibold"
              >
                Authorize Write
              </Button>
            </div>
          </div>
        )}

        {/* Console Toggle Bar */}
        <div className="flex justify-end mb-2">
          <button
            type="button"
            onClick={() => controller.toggleConsole()}
            className="px-3 py-2 rounded-t-lg bg-stone-800 text-stone-200 text-xs font-mono font-medium hover:bg-stone-900 flex items-center gap-2 shadow-md transition-colors cursor-pointer select-none min-h-[44px] min-w-[44px]"
            data-testid="console-toggle"
          >
            <Terminal size={14} />
            <span>Diagnostics & Logs</span>
            {expanded ? <ChevronDown size={14} /> : <ChevronUp size={14} />}
          </button>
        </div>

        {/* Collapsible Console Body */}
        {expanded && (
          <div
            className="bg-stone-950 text-stone-100 rounded-t-xl border border-stone-800 shadow-2xl p-4 flex flex-col gap-3 font-mono text-xs max-h-[350px] overflow-hidden"
            data-testid="console-body"
          >
            {/* Log Stream */}
            <div className="flex-1 overflow-y-auto max-h-[250px] flex flex-col gap-1 pr-2 no-scrollbar">
              {state.logs.map((log) => {
                const color = {
                  stdout: "text-stone-300",
                  stderr: "text-red-400 font-semibold",
                  notice: "text-amber-300",
                  cmd: "text-emerald-400 font-semibold",
                }[log.stream];

                return (
                  <div
                    key={log.id}
                    className={`console-log-line ${log.stream} ${color} leading-relaxed break-all`}
                  >
                    <span className="text-stone-600 mr-2 select-none">
                      [{log.timestamp}]
                    </span>
                    <span>{log.text}</span>
                  </div>
                );
              })}
            </div>

            {/* Command Input Form */}
            <form onSubmit={handleSubmit} className="flex items-center gap-2 pt-2 border-t border-stone-800">
              <span className="text-emerald-500 font-bold select-none">cranklink&gt;</span>
              <input
                type="text"
                value={cmdDraft}
                onChange={(e) => setCmdDraft((e.target as HTMLInputElement).value)}
                placeholder="probe, units, settings read [name], firmware inspect..."
                className="flex-1 bg-stone-900 text-stone-100 px-3 py-1.5 rounded border border-stone-700 text-xs focus:outline-none focus:border-stone-500"
                style="font-size: 16px;"
                data-testid="console-input"
              />
              <Button type="submit" variant="secondary" size="sm" className="bg-stone-800 text-white text-xs">
                Run
              </Button>
            </form>
          </div>
        )}
      </div>
    </div>
  );
}

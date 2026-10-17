import { useState, type FormEvent } from "react";
import { Terminal } from "lucide-react";
import type { InstrumentState } from "../../types.ts";
import type { SessionController } from "../../core/sessionController.ts";
import { Button } from "../ui/Button.tsx";
import { Card } from "../ui/Card.tsx";

interface DiagnosticsViewProps {
  state: InstrumentState;
  controller: SessionController;
}

export function DiagnosticsView({ state, controller }: DiagnosticsViewProps) {
  const [cmdDraft, setCmdDraft] = useState("");

  const handleSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const command = cmdDraft.trim();
    if (!command) return;
    setCmdDraft("");
    controller.runRawCommand(command).catch(() => {});
  };

  return (
    <main className="mx-auto flex w-full max-w-5xl flex-col gap-5 py-2" data-testid="diagnostics-page">
      <header className="flex items-center gap-3">
        <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-2xl bg-stone-100 text-stone-700">
          <Terminal size={19} />
        </div>
        <div>
          <h1 className="text-xl font-semibold tracking-tight text-stone-900">Diagnostics</h1>
          <p className="text-sm text-stone-500">Session logs and device commands</p>
        </div>
      </header>

      <Card className="flex min-h-[24rem] flex-col gap-0 overflow-hidden p-0" data-testid="console-body">
        <div className="flex items-center justify-between border-b border-stone-200 px-4 py-3 sm:px-5">
          <h2 className="text-sm font-medium text-stone-800">Session log</h2>
          <span className="text-xs text-stone-500">{state.logs.length} entries</span>
        </div>

        <div
          className="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto bg-stone-50 px-4 py-4 font-mono text-xs sm:px-5"
          role="log"
          aria-label="Session logs"
          aria-live="polite"
        >
          {state.logs.length === 0 ? (
            <p className="py-8 text-center font-sans text-sm text-stone-500">No log entries yet.</p>
          ) : (
            state.logs.map((log) => {
              const color = {
                stdout: "text-stone-700",
                stderr: "font-semibold text-red-700",
                notice: "text-amber-800",
                cmd: "font-semibold text-emerald-800",
              }[log.stream];

              return (
                <div key={log.id} className={`console-log-line ${log.stream} break-words leading-relaxed ${color}`}>
                  <time className="mr-2 select-none text-stone-400">[{log.timestamp}]</time>
                  <span>{log.text}</span>
                </div>
              );
            })
          )}
        </div>

        <form
          onSubmit={handleSubmit}
          className="flex flex-col gap-2 border-t border-stone-200 bg-white p-3 sm:flex-row sm:items-center sm:gap-3 sm:p-4"
        >
          <label htmlFor="diagnostics-command" className="shrink-0 font-mono text-xs font-semibold text-emerald-800">
            cranklink&gt;
          </label>
          <input
            id="diagnostics-command"
            type="text"
            value={cmdDraft}
            onChange={(event) => setCmdDraft(event.target.value)}
            placeholder="probe, units, settings read [name], firmware inspect…"
            className="min-h-[44px] min-w-0 flex-1 rounded-xl border border-stone-300 bg-white px-3 text-sm text-stone-900 placeholder:text-stone-400 focus:border-stone-500 focus:outline-none focus:ring-2 focus:ring-stone-200"
            style={{ fontSize: "16px" }}
            data-testid="console-input"
          />
          <Button type="submit" variant="primary" size="md" className="sm:min-w-20">
            Run
          </Button>
        </form>
      </Card>
    </main>
  );
}

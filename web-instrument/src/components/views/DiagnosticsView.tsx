import { useEffect, useRef, useState, type FormEvent } from "react";
import { Copy } from "lucide-react";
import type { InstrumentState, LogEntry } from "../../types.ts";
import type { SessionController } from "../../core/sessionController.ts";
import { Button } from "../ui/Button.tsx";

interface DiagnosticsViewProps {
  state: InstrumentState;
  controller: SessionController;
}

const STREAM_STYLES: Record<LogEntry["stream"], string> = {
  stdout: "text-stone-700",
  stderr: "text-red-800 bg-red-50 border-l-2 border-red-500 pl-2 -ml-2.5",
  notice: "text-amber-800",
  cmd: "font-semibold text-emerald-800 pt-2",
};

/** Outputs longer than this collapse to their first line. */
const COLLAPSE_AT = 240;

export function DiagnosticsView({ state, controller }: DiagnosticsViewProps) {
  const [cmdDraft, setCmdDraft] = useState("");
  const logRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const log = logRef.current;
    if (log) log.scrollTop = log.scrollHeight;
  }, [state.logs.length]);

  const handleSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const command = cmdDraft.trim();
    if (!command) return;
    setCmdDraft("");
    controller.runRawCommand(command).catch(() => {});
  };

  return (
    <div className="w-full flex flex-col gap-4" data-testid="diagnostics-page">
      <p className="text-sm text-stone-600">
        Every command this session ran and its full output. Errors shown elsewhere are summaries; the details are here.
      </p>

      <section aria-labelledby="log-heading" className="bg-white border border-stone-200 rounded-3xl shadow-xs flex min-h-[24rem] max-h-[70dvh] flex-col overflow-hidden" data-testid="console-body">
        <div className="flex items-center justify-between border-b border-stone-200 px-4 py-3 sm:px-5">
          <h2 id="log-heading" className="text-sm font-semibold text-stone-800">Session log</h2>
          <span className="text-xs text-stone-500">{state.logs.length} entries</span>
        </div>

        <div
          ref={logRef}
          className="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto bg-stone-50 px-4 py-4 font-mono text-xs sm:px-5"
          role="log"
          aria-label="Session log"
        >
          {state.logs.length === 0 ? (
            <p className="py-8 text-center font-sans text-sm text-stone-500">No log entries yet.</p>
          ) : (
            state.logs.map((log) => <LogLine key={log.id} log={log} />)
          )}
        </div>

        <form onSubmit={handleSubmit} className="flex flex-col gap-2 border-t border-stone-200 bg-white p-3 sm:flex-row sm:items-center sm:gap-3 sm:p-4">
          <label htmlFor="diagnostics-command" className="shrink-0 font-mono text-xs font-semibold text-emerald-800">
            cranklink&gt;
          </label>
          <input
            id="diagnostics-command"
            type="text"
            value={cmdDraft}
            onChange={(event) => setCmdDraft(event.target.value)}
            placeholder="probe, settings read [name], firmware inspect…"
            autoComplete="off"
            spellCheck={false}
            className="min-h-[44px] min-w-0 flex-1 rounded-xl border border-stone-300 bg-white px-3 font-mono text-stone-900 placeholder:text-stone-400 focus:border-stone-500 focus:outline-none focus:ring-2 focus:ring-amber-300"
            style={{ fontSize: "16px" }}
            data-testid="console-input"
          />
          <Button type="submit" variant="primary" size="md" className="sm:min-w-20">
            Run
          </Button>
        </form>
      </section>
    </div>
  );
}

function LogLine({ log }: { log: LogEntry }) {
  const long = log.text.length > COLLAPSE_AT || log.text.split("\n").length > 4;
  const className = `console-log-line ${log.stream} leading-relaxed [overflow-wrap:anywhere] whitespace-pre-wrap ${STREAM_STYLES[log.stream]}`;
  const time = <time className="mr-2 select-none text-stone-400">[{log.timestamp}]</time>;

  if (!long) {
    return (
      <div className={className}>
        {time}
        <span>{log.text}</span>
      </div>
    );
  }

  const firstLine = log.text.split("\n")[0].slice(0, 120);
  return (
    <details className={className}>
      <summary className="cursor-pointer select-none list-none [&::-webkit-details-marker]:hidden">
        {time}
        <span>{firstLine}</span>
        <span className="ml-2 font-sans text-stone-500 underline underline-offset-2">
          {Math.ceil(log.text.length / 1024)} KB, show all
        </span>
      </summary>
      <div className="relative mt-1 rounded-lg border border-stone-200 bg-white p-2">
        <button
          type="button"
          onClick={() => navigator.clipboard?.writeText(log.text).catch(() => {})}
          className="absolute right-1.5 top-1.5 inline-flex items-center gap-1 rounded-md px-2 py-1 font-sans text-[11px] text-stone-600 hover:bg-stone-100"
        >
          <Copy size={12} aria-hidden="true" />
          Copy
        </button>
        <pre className="whitespace-pre-wrap [overflow-wrap:anywhere] pr-14">{prettyJson(log.text)}</pre>
      </div>
    </details>
  );
}

function prettyJson(text: string): string {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return text;
  }
}

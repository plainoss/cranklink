import { useEffect, useRef, useState, type FormEvent } from "react";
import { Copy } from "lucide-react";
import type { InstrumentState, LogEntry } from "../../types.ts";
import type { SessionController } from "../../core/sessionController.ts";
import { Button, Card, Disclosure, Form, Input, Label, TextField } from "@heroui/react";
import { BUILD_REVISION, formatDiagnosticReport } from "../../domain/diagnostics.ts";

interface DiagnosticsViewProps {
  state: InstrumentState;
  controller: SessionController;
}

const STREAM_STYLES: Record<LogEntry["stream"], string> = {
  stdout: "text-foreground",
  stderr: "text-danger bg-danger-soft border-l-2 border-danger pl-2 -ml-2.5",
  notice: "text-warning-soft-foreground",
  cmd: "font-semibold text-success pt-2",
};

/** Outputs longer than this collapse to their first line. */
const COLLAPSE_AT = 240;

export function DiagnosticsView({ state, controller }: DiagnosticsViewProps) {
  const [cmdDraft, setCmdDraft] = useState("");
  const [copyState, setCopyState] = useState<"idle" | "copied" | "manual">("idle");
  const logRef = useRef<HTMLDivElement>(null);
  const canRunCommands = state.phase === "ready" && !state.linkLost;
  const report = formatDiagnosticReport(state, { url: location.href, userAgent: navigator.userAgent });
  const copyReport = async () => {
    try {
      if (!navigator.clipboard?.writeText) throw new Error("Clipboard unavailable");
      await navigator.clipboard.writeText(report);
      setCopyState("copied");
    } catch {
      setCopyState("manual");
    }
  };

  useEffect(() => {
    const log = logRef.current;
    if (log) log.scrollTop = log.scrollHeight;
  }, [state.logs.length]);

  const handleSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const command = cmdDraft.trim();
    if (!command || !canRunCommands) return;
    setCmdDraft("");
    controller.runRawCommand(command).catch(() => {});
  };

  return (
    <div className="w-full flex flex-col gap-4" data-testid="diagnostics-page">
      <p className="text-sm text-muted">
        Every command this session ran and its full output. Errors shown elsewhere are summaries; the details are here.
      </p>

      <Card className="gap-3">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <span className="text-xs text-muted">Build <code>{BUILD_REVISION}</code></span>
          <Button variant="tertiary" onPress={() => void copyReport()} data-testid="btn-copy-diagnostics">
            <Copy size={16} aria-hidden="true" />Copy diagnostic report
          </Button>
        </div>
        <p className="text-sm text-muted">Logs stay available in this tab after a failed connection or disconnect. Copy them before reloading.</p>
        <p role="status" className="text-sm">
          {copyState === "copied" ? "Diagnostic report copied." : copyState === "manual" ? "Automatic copy is unavailable. Select and copy the report below." : ""}
        </p>
        {copyState === "manual" && <textarea readOnly aria-label="Diagnostic report" value={report}
          onFocus={(event) => event.currentTarget.select()} rows={12}
          className="w-full rounded-lg border border-separator bg-surface-secondary p-3 font-mono text-xs" />}
      </Card>

      <Card aria-labelledby="log-heading" className="p-0 gap-0 min-h-[24rem] max-h-[70dvh] overflow-hidden" data-testid="console-body">
        <Card.Header className="flex flex-row items-center justify-between px-4 py-3 sm:px-5 border-b border-separator">
          <Card.Title id="log-heading" className="text-sm font-semibold">Session log</Card.Title>
          <span className="text-xs text-muted">{state.logs.length} entries</span>
        </Card.Header>

        <div
          ref={logRef}
          className="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto bg-surface-secondary px-4 py-4 font-mono text-xs sm:px-5"
          role="log"
          aria-label="Session log"
        >
          {state.logs.length === 0 ? (
            <p className="py-8 text-center font-sans text-sm text-muted">No log entries yet.</p>
          ) : (
            state.logs.map((log) => <LogLine key={log.id} log={log} />)
          )}
        </div>

        <Card.Footer className="p-3 sm:p-4 border-t border-separator">
          {canRunCommands ? <Form onSubmit={handleSubmit} className="flex w-full flex-col gap-2 sm:flex-row sm:items-end sm:gap-3">
            <TextField value={cmdDraft} onChange={setCmdDraft} className="min-w-0 flex-1" fullWidth>
              <Label className="font-mono text-xs font-semibold">cranklink&gt;</Label>
              <Input
                placeholder="probe, settings read [name], firmware inspect…"
                autoComplete="off"
                spellCheck={false}
                className="font-mono"
                style={{ fontSize: "16px" }}
                data-testid="console-input"
              />
            </TextField>
            <Button type="submit" className="sm:min-w-20">
              Run
            </Button>
          </Form> : <p className="text-sm text-muted">Connect to run commands. Viewing and copying these logs does not contact the bike.</p>}
        </Card.Footer>
      </Card>
    </div>
  );
}

function LogLine({ log }: { log: LogEntry }) {
  const long = log.text.length > COLLAPSE_AT || log.text.split("\n").length > 4;
  const className = `console-log-line ${log.stream} leading-relaxed [overflow-wrap:anywhere] whitespace-pre-wrap ${STREAM_STYLES[log.stream]}`;
  const time = <time className="mr-2 select-none text-muted">[{log.timestamp}]</time>;

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
    <Disclosure className={className}>
      <Disclosure.Heading>
        <Disclosure.Trigger className="text-left font-mono text-xs">
          {time}
          <span>{firstLine}</span>
          <span className="ml-2 font-sans text-muted underline underline-offset-2">
            {Math.ceil(log.text.length / 1024)} KB, show all
          </span>
        </Disclosure.Trigger>
      </Disclosure.Heading>
      <Disclosure.Content>
        <Disclosure.Body className="relative mt-1 rounded-lg border border-separator bg-surface p-2">
          <Button
            variant="ghost"
            size="sm"
            onPress={() => navigator.clipboard?.writeText(log.text).catch(() => {})}
            className="absolute right-1.5 top-1.5 font-sans"
          >
            <Copy size={12} aria-hidden="true" />
            Copy
          </Button>
          <pre className="whitespace-pre-wrap [overflow-wrap:anywhere] pr-16">{prettyJson(log.text)}</pre>
        </Disclosure.Body>
      </Disclosure.Content>
    </Disclosure>
  );
}

function prettyJson(text: string): string {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return text;
  }
}

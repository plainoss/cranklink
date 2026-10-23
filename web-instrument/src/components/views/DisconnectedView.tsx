import { Check, Bike } from "lucide-react";
import { Button, Card } from "@heroui/react";

/** GATT closure is observable; the bike's physical recovery is not. */
export function DisconnectedView({ onContinue }: { onContinue: () => void }) {
  return (
    <main className="min-h-dvh flex items-center justify-center px-4 py-8 bg-background text-foreground">
      <Card className="w-full max-w-md gap-5 p-6" data-testid="bluetooth-disconnected-view">
        <div className="flex items-center gap-2 text-sm font-semibold" role="status">
          <Check size={20} aria-hidden="true" />
          Bluetooth disconnected
        </div>
        <div className="flex flex-col gap-3">
          <Bike size={32} aria-hidden="true" />
          <h1 className="text-xl font-bold">Wait for the bike to return to riding mode</h1>
          <p className="text-sm text-muted">
            The connection is closed, but the bike may need a few seconds to leave connection mode.
            Wait for the display, buttons and shifting to respond normally.
          </p>
          <p className="text-sm text-muted">This page cannot detect when the bike is ready.</p>
        </div>
        <Button onPress={onContinue} className="min-h-11" data-testid="btn-back-to-connect">Back to connection screen</Button>
      </Card>
    </main>
  );
}

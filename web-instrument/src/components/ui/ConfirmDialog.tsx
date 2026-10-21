/**
 * ConfirmDialog.tsx - HeroUI AlertDialog for actions that write to the bike.
 */

import type { ReactNode } from "react";
import { AlertDialog, Button } from "@heroui/react";

interface ConfirmAction {
  label: string;
  onPress: () => void;
  variant?: "primary" | "danger" | "secondary";
  "data-testid"?: string;
}

interface ConfirmDialogProps {
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  status?: "default" | "warning" | "danger";
  children: ReactNode;
  /** Confirming actions, most important last; a Cancel button is always added first. */
  actions: ConfirmAction[];
  cancelLabel?: string;
}

export function ConfirmDialog({ isOpen, onOpenChange, title, status = "default", children, actions, cancelLabel = "Cancel" }: ConfirmDialogProps) {
  return (
    <AlertDialog>
      <AlertDialog.Backdrop isOpen={isOpen} onOpenChange={onOpenChange}>
        <AlertDialog.Container size="md">
          <AlertDialog.Dialog>
            <AlertDialog.Header>
              <AlertDialog.Icon status={status} />
              <AlertDialog.Heading>{title}</AlertDialog.Heading>
            </AlertDialog.Header>
            <AlertDialog.Body>{children}</AlertDialog.Body>
            <AlertDialog.Footer className="flex-wrap">
              <Button variant="tertiary" slot="close">
                {cancelLabel}
              </Button>
              {actions.map(({ label, onPress, variant = "primary", "data-testid": testId }) => (
                <Button key={label} variant={variant} onPress={onPress} data-testid={testId}>
                  {label}
                </Button>
              ))}
            </AlertDialog.Footer>
          </AlertDialog.Dialog>
        </AlertDialog.Container>
      </AlertDialog.Backdrop>
    </AlertDialog>
  );
}

import { h, type ComponentChildren } from "preact";
import { Dialog as BaseDialog } from "@base-ui/react/dialog";
import { X } from "lucide-preact";

export interface ModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description?: string;
  children: ComponentChildren;
  footer?: ComponentChildren;
  maxWidth?: "sm" | "md" | "lg" | "xl";
}

export function Modal({
  open,
  onOpenChange,
  title,
  description,
  children,
  footer,
  maxWidth = "md",
}: ModalProps) {
  const maxW = {
    sm: "max-w-sm",
    md: "max-w-md",
    lg: "max-w-lg",
    xl: "max-w-xl",
  }[maxWidth];

  return (
    <BaseDialog.Root open={open} onOpenChange={onOpenChange}>
      <BaseDialog.Portal>
        <BaseDialog.Backdrop className="fixed inset-0 bg-stone-900/50 backdrop-blur-xs z-50 transition-opacity" />
        <BaseDialog.Popup
          className={`fixed top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 bg-white rounded-xl shadow-2xl border border-stone-200 z-50 p-6 w-[calc(100%-2rem)] ${maxW} max-h-[90vh] overflow-y-auto focus:outline-none`}
        >
          <div className="flex items-start justify-between gap-4 mb-4">
            <div>
              <BaseDialog.Title className="text-lg font-semibold text-stone-900 font-sans">
                {title}
              </BaseDialog.Title>
              {description && (
                <BaseDialog.Description className="text-sm text-stone-600 mt-1 font-sans">
                  {description}
                </BaseDialog.Description>
              )}
            </div>
            <BaseDialog.Close className="text-stone-600 hover:text-stone-700 p-1.5 rounded-lg hover:bg-stone-100 transition-colors cursor-pointer min-h-[36px] min-w-[36px] flex items-center justify-center">
              <X size={18} />
            </BaseDialog.Close>
          </div>

          <div className="py-2">{children}</div>

          {footer && <div className="mt-6 pt-4 border-t border-stone-100 flex items-center justify-end gap-3">{footer}</div>}
        </BaseDialog.Popup>
      </BaseDialog.Portal>
    </BaseDialog.Root>
  );
}

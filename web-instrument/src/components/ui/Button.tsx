import { h, type ComponentChildren } from "preact";

export interface ButtonProps {
  children?: ComponentChildren;
  variant?: "primary" | "secondary" | "danger" | "ghost" | "outline" | "purple" | "custom";
  size?: "sm" | "md" | "lg";
  disabled?: boolean;
  onClick?: (e: MouseEvent) => void;
  className?: string;
  type?: "button" | "submit" | "reset";
  "data-action"?: string;
  "data-testid"?: string;
  title?: string;
}

export function Button({
  children,
  variant = "secondary",
  size = "md",
  disabled = false,
  onClick,
  className = "",
  type = "button",
  "data-action": dataAction,
  "data-testid": dataTestId,
  title,
}: ButtonProps) {
  const baseStyles =
    "inline-flex items-center justify-center font-medium transition-colors cursor-pointer select-none disabled:opacity-50 disabled:cursor-not-allowed rounded-lg font-sans text-center touch-manipulation";

  const sizeStyles = {
    sm: "min-h-[36px] px-3.5 py-1.5 text-xs",
    md: "min-h-[44px] px-4 py-2 text-sm",
    lg: "min-h-[50px] px-6 py-3 text-base",
  }[size];

  // If caller passes custom bg- or text- in className and didn't specify an explicit non-secondary variant,
  // do not inject conflicting secondary background/text classes.
  const hasCustomBgOrText = /(?:^|\s)(?:bg-|text-)/.test(className);
  const resolvedVariant = variant === "secondary" && hasCustomBgOrText ? "custom" : variant;

  const variantStyles = {
    primary:
      "bg-stone-900 text-white hover:bg-stone-800 active:bg-black shadow-xs border border-transparent",
    secondary:
      "bg-stone-100 text-stone-900 hover:bg-stone-200 active:bg-stone-300 border border-stone-200",
    danger:
      "bg-red-600 text-white hover:bg-red-700 active:bg-red-800 shadow-xs border border-transparent",
    ghost:
      "bg-transparent text-stone-700 hover:bg-stone-100 active:bg-stone-200 border border-transparent",
    outline:
      "bg-white text-stone-900 border border-stone-300 hover:bg-stone-50 active:bg-stone-100 shadow-xs",
    purple:
      "bg-purple-700 text-white hover:bg-purple-800 active:bg-purple-900 shadow-xs border border-transparent",
    custom: "",
  }[resolvedVariant];

  return (
    <button
      type={type}
      disabled={disabled}
      onClick={onClick}
      className={`${baseStyles} ${sizeStyles} ${variantStyles} ${className}`.trim()}
      data-action={dataAction}
      data-testid={dataTestId}
      title={title}
    >
      {children}
    </button>
  );
}

import type { ReactNode, MouseEvent } from "react";

export interface ButtonProps {
  children?: ReactNode;
  variant?: "primary" | "secondary" | "danger" | "ghost" | "outline" | "purple" | "custom";
  size?: "sm" | "md" | "lg";
  disabled?: boolean;
  onClick?: (e: MouseEvent<HTMLButtonElement>) => void;
  className?: string;
  type?: "button" | "submit" | "reset";
  "data-action"?: string;
  "data-testid"?: string;
  title?: string;
  id?: string;
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
  id,
}: ButtonProps) {
  const baseStyles =
    "inline-flex items-center justify-center font-medium transition-all duration-150 cursor-pointer select-none disabled:opacity-50 disabled:cursor-not-allowed rounded-2xl font-sans text-center touch-manipulation active:scale-[0.98] focus:outline-none";

  const sizeStyles = {
    sm: "min-h-[38px] px-4 py-2 text-xs",
    md: "min-h-[44px] px-5 py-2.5 text-xs sm:text-sm font-semibold",
    lg: "min-h-[50px] px-6 py-3 text-sm sm:text-base font-semibold",
  }[size];

  const hasCustomBgOrText = /(?:^|\s)(?:bg-|text-)/.test(className);
  const resolvedVariant = variant === "secondary" && hasCustomBgOrText ? "custom" : variant;

  const variantStyles = {
    primary:
      "bg-stone-900 text-white hover:bg-stone-800 active:bg-black shadow-xs border border-transparent",
    secondary:
      "bg-stone-100 text-stone-900 hover:bg-stone-200 active:bg-stone-300 border border-stone-200/80 shadow-2xs",
    danger:
      "bg-red-600 text-white hover:bg-red-700 active:bg-red-800 shadow-xs border border-transparent",
    ghost:
      "bg-transparent text-stone-700 hover:bg-stone-100 active:bg-stone-200 border border-transparent",
    outline:
      "bg-white text-stone-900 border border-stone-200 hover:bg-stone-50 active:bg-stone-100 shadow-2xs",
    purple:
      "bg-purple-700 text-white hover:bg-purple-800 active:bg-purple-900 shadow-xs border border-transparent",
    custom: "",
  }[resolvedVariant];

  return (
    <button
      id={id}
      type={type}
      disabled={disabled}
      onClick={onClick}
      title={title}
      data-action={dataAction}
      data-testid={dataTestId}
      className={`${baseStyles} ${sizeStyles} ${variantStyles} ${className}`.trim()}
    >
      {children}
    </button>
  );
}

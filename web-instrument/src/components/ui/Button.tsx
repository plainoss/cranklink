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
  const hasCustomBgOrText = /(?:^|\s)(?:bg-|text-)/.test(className);
  const resolvedVariant = variant === "secondary" && hasCustomBgOrText ? "custom" : variant;
  const variantClass = {
    primary: "button--primary",
    secondary: "button--secondary",
    danger: "button--danger",
    ghost: "button--ghost",
    outline: "button--outline",
    purple: "button--purple",
    // Keep the shared HeroUI base and size treatment while allowing callers
    // to supply their own colors and other utility styles.
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
      className={`button button--${size} ${variantClass} ${className}`.trim()}
    >
      {children}
    </button>
  );
}

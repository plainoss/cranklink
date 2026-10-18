import type { ButtonHTMLAttributes, ReactNode } from "react";

export interface ButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "className"> {
  children?: ReactNode;
  variant?: "primary" | "secondary" | "danger" | "ghost" | "outline" | "custom";
  size?: "sm" | "md" | "lg";
  className?: string;
  "data-action"?: string;
  "data-testid"?: string;
}

const VARIANT_CLASS = {
  primary: "button--primary",
  secondary: "button--secondary",
  danger: "button--danger",
  ghost: "button--ghost",
  outline: "button--outline",
  // Keeps the shared HeroUI base and size while callers supply colors.
  custom: "",
} as const;

export function Button({
  children,
  variant = "secondary",
  size = "md",
  className = "",
  type = "button",
  ...rest
}: ButtonProps) {
  return (
    <button type={type} {...rest} className={`button button--${size} ${VARIANT_CLASS[variant]} ${className}`.trim()}>
      {children}
    </button>
  );
}

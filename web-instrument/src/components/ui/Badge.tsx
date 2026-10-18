import type { ReactNode } from "react";

export interface BadgeProps {
  children: ReactNode;
  variant?: "success" | "neutral";
  className?: string;
  "data-testid"?: string;
}

const STYLES = {
  success: "bg-emerald-50 text-emerald-800 border-emerald-200",
  neutral: "bg-stone-100 text-stone-700 border-stone-200",
} as const;

export function Badge({ children, variant = "neutral", className = "", "data-testid": dataTestId }: BadgeProps) {
  return (
    <span
      className={`inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full text-xs font-medium border ${STYLES[variant]} ${className}`}
      data-testid={dataTestId}
    >
      {children}
    </span>
  );
}

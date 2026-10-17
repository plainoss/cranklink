import { h, type ComponentChildren } from "preact";

export interface BadgeProps {
  children: ComponentChildren;
  variant?: "success" | "warning" | "danger" | "sim" | "neutral";
  className?: string;
  "data-testid"?: string;
}

export function Badge({
  children,
  variant = "neutral",
  className = "",
  "data-testid": dataTestId,
}: BadgeProps) {
  const styles = {
    success: "bg-emerald-50 text-emerald-800 border-emerald-200",
    warning: "bg-amber-50 text-amber-800 border-amber-200",
    danger: "bg-red-50 text-red-800 border-red-200",
    sim: "bg-purple-50 text-purple-800 border-purple-200",
    neutral: "bg-stone-100 text-stone-700 border-stone-200",
  }[variant];

  return (
    <span
      className={`inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full text-xs font-medium border font-mono tracking-tight ${styles} ${className}`}
      data-testid={dataTestId}
    >
      {children}
    </span>
  );
}

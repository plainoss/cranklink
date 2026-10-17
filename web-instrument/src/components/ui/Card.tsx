import type { ReactNode } from "react";

export interface CardProps {
  children: ReactNode;
  className?: string;
  onClick?: () => void;
  hoverable?: boolean;
  "data-testid"?: string;
}

export function Card({
  children,
  className = "",
  onClick,
  hoverable = false,
  "data-testid": dataTestId,
}: CardProps) {
  const clickableStyles =
    hoverable || onClick
      ? "hover:border-stone-300 hover:shadow-sm cursor-pointer active:scale-[0.99] transition-all"
      : "";

  const hasPadding = /(?:^|\s)p[xytrbl]?-/.test(className);
  const defaultPadding = hasPadding ? "" : "p-6 sm:p-7";

  return (
    <div
      onClick={onClick}
      className={`bg-white border border-stone-200/80 rounded-3xl shadow-xs ${defaultPadding} ${clickableStyles} ${className}`.trim()}
      data-testid={dataTestId}
    >
      {children}
    </div>
  );
}

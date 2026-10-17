import { h, type ComponentChildren } from "preact";

export interface CardProps {
  children: ComponentChildren;
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
      ? "hover:border-stone-400 hover:shadow-md cursor-pointer active:scale-[0.99] transition-all"
      : "";

  return (
    <div
      onClick={onClick}
      className={`bg-white border border-stone-200/90 rounded-2xl p-4 sm:p-8 shadow-xs ${clickableStyles} ${className}`}
      data-testid={dataTestId}
    >
      {children}
    </div>
  );
}

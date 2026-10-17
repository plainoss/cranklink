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

  // If the caller specified padding (e.g. p-, px-, py-), do not inject default padding
  const hasPadding = /(?:^|\s)p[xytrbl]?-/.test(className);
  const defaultPadding = hasPadding ? "" : "p-5 sm:p-6";

  return (
    <div
      onClick={onClick}
      className={`bg-white border border-stone-200/90 rounded-2xl shadow-xs ${defaultPadding} ${clickableStyles} ${className}`.trim()}
      data-testid={dataTestId}
    >
      {children}
    </div>
  );
}

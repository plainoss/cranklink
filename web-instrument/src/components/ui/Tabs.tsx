import type { ReactNode } from "react";

export interface TabItem {
  id: string;
  label: string;
  count?: number;
}

export interface TabsProps {
  value: string;
  onValueChange: (val: string) => void;
  items: TabItem[];
  children?: ReactNode;
  className?: string;
}

export function Tabs({
  value,
  onValueChange,
  items,
  children,
  className = "",
}: TabsProps) {
  return (
    <div className={`w-full max-w-full overflow-hidden flex flex-col gap-3 ${className}`}>
      <div
        role="tablist"
        aria-orientation="horizontal"
        className="flex items-center gap-1.5 p-1 bg-stone-200/70 rounded-xl overflow-x-auto no-scrollbar"
      >
        {items.map((tab) => {
          const isSelected = value === tab.id;
          return (
            <button
              type="button"
              role="tab"
              key={tab.id}
              aria-selected={isSelected}
              onClick={() => onValueChange(tab.id)}
              className={`flex-1 flex items-center justify-center gap-2 px-4 py-2 text-xs sm:text-sm font-semibold rounded-lg transition-all whitespace-nowrap cursor-pointer select-none min-h-[38px] ${
                isSelected
                  ? "bg-white text-stone-900 shadow-xs"
                  : "text-stone-600 hover:text-stone-900 hover:bg-stone-300/40"
              }`}
            >
              <span>{tab.label}</span>
              {tab.count !== undefined && tab.count > 0 && (
                <span
                  className={`px-1.5 py-0.5 rounded-full text-[10px] font-mono ${
                    isSelected ? "bg-stone-900 text-white" : "bg-stone-300 text-stone-700"
                  }`}
                >
                  {tab.count}
                </span>
              )}
            </button>
          );
        })}
      </div>
      {children}
    </div>
  );
}

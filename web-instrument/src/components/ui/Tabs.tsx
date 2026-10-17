import { h, type ComponentChildren } from "preact";
import { Tabs as BaseTabs } from "@base-ui/react/tabs";

export interface TabItem {
  id: string;
  label: string;
  count?: number;
}

export interface TabsProps {
  value: string;
  onValueChange: (val: string) => void;
  items: TabItem[];
  children?: ComponentChildren;
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
    <BaseTabs.Root
      value={value}
      onValueChange={(val: any) => onValueChange(String(val))}
      className={`w-full max-w-full overflow-hidden flex flex-col ${className}`}
    >
      <BaseTabs.List className="flex border-b border-stone-200 gap-1 overflow-x-auto no-scrollbar">
        {items.map((tab) => (
          <BaseTabs.Tab
            key={tab.id}
            value={tab.id}
            className="flex items-center gap-2 px-4 py-2.5 text-sm font-medium border-b-2 border-transparent text-stone-600 hover:text-stone-900 data-[selected]:border-stone-900 data-[selected]:text-stone-900 transition-colors whitespace-nowrap cursor-pointer select-none"
          >
            <span>{tab.label}</span>
            {tab.count !== undefined && tab.count > 0 && (
              <span className="px-1.5 py-0.2 rounded-full text-xs font-mono bg-stone-100 text-stone-600">
                {tab.count}
              </span>
            )}
          </BaseTabs.Tab>
        ))}
      </BaseTabs.List>
      {children}
    </BaseTabs.Root>
  );
}

export const TabPanel = BaseTabs.Panel;

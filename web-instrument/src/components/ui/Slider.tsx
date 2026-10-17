import { h } from "preact";
import { Slider as BaseSlider } from "@base-ui/react/slider";

export interface SliderProps {
  value: number;
  min: number;
  max: number;
  step?: number;
  onChange: (value: number) => void;
  label?: string;
  valueFormatter?: (value: number) => string;
  unit?: string;
  disabled?: boolean;
  className?: string;
  "data-testid"?: string;
}

export function Slider({
  value,
  min,
  max,
  step = 1,
  onChange,
  label,
  valueFormatter,
  unit = "",
  disabled = false,
  className = "",
  "data-testid": dataTestId,
}: SliderProps) {
  const displayVal = valueFormatter ? valueFormatter(value) : `${value}${unit ? ` ${unit}` : ""}`;

  return (
    <div className={`w-full flex flex-col gap-1.5 ${className}`} data-testid={dataTestId}>
      {(label || displayVal) && (
        <div className="flex items-center justify-between text-sm">
          {label && <span className="font-medium text-stone-700">{label}</span>}
          <span className="font-mono font-semibold text-stone-900 tabular-nums">
            {displayVal}
          </span>
        </div>
      )}
      <BaseSlider.Root
        value={value}
        min={min}
        max={max}
        step={step}
        disabled={disabled}
        onValueChange={(val: number | number[]) => {
          const next = Array.isArray(val) ? val[0] : val;
          if (typeof next === "number" && !isNaN(next)) {
            onChange(next);
          }
        }}
        className="relative flex items-center select-none touch-none w-full h-9 cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed py-2 px-3"
      >
        <BaseSlider.Track className="bg-stone-200 relative grow rounded-full h-2.5 overflow-hidden border border-stone-300">
          <BaseSlider.Indicator className="absolute bg-stone-900 rounded-full h-full" />
        </BaseSlider.Track>
        <BaseSlider.Thumb className="block w-6 h-6 bg-white border-2 border-stone-900 rounded-full shadow-md hover:scale-110 active:scale-95 transition-transform focus:outline-none focus:ring-2 focus:ring-stone-400" />
      </BaseSlider.Root>
      <div className="flex justify-between text-[11px] font-mono text-stone-600 px-0.5">
        <span>{valueFormatter ? valueFormatter(min) : `${min}${unit}`}</span>
        <span>{valueFormatter ? valueFormatter(max) : `${max}${unit}`}</span>
      </div>
    </div>
  );
}

import type { ChangeEvent } from "react";

export interface SliderProps {
  value: number;
  min: number;
  max: number;
  step?: number;
  onChange: (value: number) => void;
  /** Accessible name; the visible label belongs to the surrounding row. */
  label: string;
  /** Spoken value including its unit, e.g. "25 km/h". */
  valueText: string;
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
  valueText,
  disabled = false,
  className = "",
  "data-testid": dataTestId,
}: SliderProps) {
  const span = max - min || 1;
  const percentage = Math.min(100, Math.max(0, ((value - min) / span) * 100));

  const handleChange = (e: ChangeEvent<HTMLInputElement>) => {
    const next = Number(e.target.value);
    if (!Number.isNaN(next)) onChange(next);
  };

  return (
    <div className={`relative flex items-center select-none w-full h-11 px-1 ${className}`} data-testid={dataTestId}>
      <div className="absolute left-1 right-1 h-2.5 rounded-full bg-stone-200 border border-stone-300 pointer-events-none overflow-hidden">
        <div className="h-full bg-stone-900 rounded-full" style={{ width: `${percentage}%` }} />
      </div>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        onChange={handleChange}
        aria-label={label}
        aria-valuetext={valueText}
        className="peer relative w-full h-11 opacity-0 cursor-pointer disabled:cursor-not-allowed z-10 touch-pan-x"
      />
      <div
        className="absolute top-1/2 -translate-y-1/2 -translate-x-1/2 w-6 h-6 bg-white border-2 border-stone-900 rounded-full shadow-md pointer-events-none peer-focus-visible:ring-4 peer-focus-visible:ring-amber-300"
        style={{ left: `calc(4px + (100% - 8px) * ${percentage / 100})` }}
      />
    </div>
  );
}

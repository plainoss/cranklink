import { useState, useRef, useEffect, type ChangeEvent } from "react";

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
  const percentage = Math.min(100, Math.max(0, ((value - min) / (max - min)) * 100));

  const handleChange = (e: ChangeEvent<HTMLInputElement>) => {
    const next = Number(e.target.value);
    if (!isNaN(next)) {
      onChange(next);
    }
  };

  return (
    <div className={`w-full flex flex-col gap-2 ${className}`} data-testid={dataTestId}>
      {(label || displayVal) && (
        <div className="flex items-center justify-between text-sm">
          {label && <span className="font-semibold text-stone-800 text-xs sm:text-sm">{label}</span>}
          <span className="font-mono font-bold text-stone-900 tabular-nums text-xs sm:text-sm bg-stone-100 px-2 py-0.5 rounded-md border border-stone-200">
            {displayVal}
          </span>
        </div>
      )}
      <div className="relative flex items-center select-none w-full h-11 py-2 px-1">
        {/* Custom styled track background & fill */}
        <div className="absolute left-1 right-1 h-3 rounded-full bg-stone-200 border border-stone-300 pointer-events-none overflow-hidden">
          <div
            className="h-full bg-stone-900 rounded-full transition-all duration-75"
            style={{ width: `${percentage}%` }}
          />
        </div>
        {/* Native range input with high opacity/transparent styling for maximum cross-device accessibility */}
        <input
          type="range"
          min={min}
          max={max}
          step={step}
          value={value}
          disabled={disabled}
          onChange={handleChange}
          aria-label={label || "Slider"}
          aria-valuemin={min}
          aria-valuemax={max}
          aria-valuenow={value}
          className="relative w-full h-11 opacity-0 cursor-pointer disabled:cursor-not-allowed z-10 touch-pan-x"
        />
        {/* Custom thumb tracking the value */}
        <div
          className="absolute top-1/2 -translate-y-1/2 -translate-x-1/2 w-6 h-6 bg-white border-2 border-stone-900 rounded-full shadow-md pointer-events-none transition-transform duration-75"
          style={{ left: `calc(4px + (100% - 8px) * ${percentage / 100})` }}
        />
      </div>
    </div>
  );
}

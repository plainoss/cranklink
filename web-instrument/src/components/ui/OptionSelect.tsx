import { Label, ListBox, Select } from "@heroui/react";

export interface SelectOption {
  id: string;
  label: string;
}

interface OptionSelectProps {
  /** Visible label; omit it and pass aria-label when the surrounding row names the field. */
  label?: string;
  "aria-label"?: string;
  "aria-describedby"?: string;
  value: string | null;
  options: SelectOption[];
  onChange: (id: string) => void;
  isDisabled?: boolean;
  "data-testid"?: string;
}

/** HeroUI Select for a flat list of string-keyed options. */
export function OptionSelect({ label, "aria-label": ariaLabel, "aria-describedby": describedBy, value, options, onChange, isDisabled, "data-testid": testId }: OptionSelectProps) {
  return (
    <Select
      aria-label={label ? undefined : ariaLabel}
      aria-describedby={describedBy}
      value={value}
      onChange={(key) => key !== null && onChange(String(key))}
      isDisabled={isDisabled}
      fullWidth
      data-testid={testId}
    >
      {label && <Label>{label}</Label>}
      <Select.Trigger>
        <Select.Value />
        <Select.Indicator />
      </Select.Trigger>
      <Select.Popover>
        <ListBox>
          {options.map((option) => (
            <ListBox.Item key={option.id} id={option.id} textValue={option.label}>
              {option.label}
              <ListBox.ItemIndicator />
            </ListBox.Item>
          ))}
        </ListBox>
      </Select.Popover>
    </Select>
  );
}

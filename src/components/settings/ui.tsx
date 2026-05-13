import React from "react";

export function Row({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="grid grid-cols-[220px_1fr] items-center gap-6 py-1">
      <div>
        <div className="text-sm text-neutral-800">{label}</div>
        {hint && <div className="text-xs text-neutral-500 mt-0.5">{hint}</div>}
      </div>
      <div className="flex items-center gap-2 min-w-0">{children}</div>
    </div>
  );
}

export function Toggle({
  checked,
  onChange,
  label,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label?: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={() => onChange(!checked)}
      className={`relative inline-flex h-[22px] w-[38px] flex-none items-center rounded-full transition-colors duration-150 ${
        checked ? "bg-accent" : "bg-neutral-300"
      } focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/40`}
    >
      <span
        aria-hidden
        className={`inline-block h-[18px] w-[18px] rounded-full bg-white shadow transition-transform duration-150 ${
          checked ? "translate-x-[18px]" : "translate-x-[2px]"
        }`}
      />
    </button>
  );
}

export function Button({
  variant = "primary",
  children,
  ...rest
}: React.ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "secondary" | "ghost";
}) {
  const styles = {
    primary:
      "bg-accent text-white hover:bg-blue-600 active:bg-blue-700 shadow-sm",
    secondary:
      "border border-neutral-300 bg-white text-neutral-800 hover:border-neutral-400 hover:bg-neutral-50",
    ghost: "text-neutral-600 hover:text-neutral-900",
  }[variant];
  return (
    <button
      {...rest}
      className={`rounded-md px-3 py-1.5 text-sm font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed ${styles} ${rest.className ?? ""}`}
    >
      {children}
    </button>
  );
}

export function Select<T extends string | number>({
  value,
  onChange,
  options,
}: {
  value: T;
  onChange: (v: T) => void;
  options: { value: T; label: string }[];
}) {
  return (
    <select
      value={value}
      onChange={(e) => onChange(e.target.value as T)}
      className="rounded-md border border-neutral-300 bg-white px-2 py-1 text-sm focus:outline-none focus:border-accent focus:ring-2 focus:ring-accent/30"
    >
      {options.map((o) => (
        <option key={String(o.value)} value={o.value}>
          {o.label}
        </option>
      ))}
    </select>
  );
}

export function Radio({
  value,
  options,
  onChange,
}: {
  value: string;
  options: { value: string; label: string }[];
  onChange: (v: string) => void;
}) {
  return (
    <div className="flex gap-4 text-sm">
      {options.map((o) => (
        <label key={o.value} className="flex items-center gap-1.5 cursor-pointer">
          <input
            type="radio"
            checked={value === o.value}
            onChange={() => onChange(o.value)}
            className="accent-accent"
          />
          <span>{o.label}</span>
        </label>
      ))}
    </div>
  );
}

export function Slider({
  value,
  min,
  max,
  step,
  onChange,
  format,
}: {
  value: number;
  min: number;
  max: number;
  step?: number;
  onChange: (v: number) => void;
  format?: (v: number) => string;
}) {
  return (
    <>
      <input
        type="range"
        min={min}
        max={max}
        step={step ?? 1}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        className="w-44 accent-accent"
      />
      <span className="text-xs text-neutral-500 tabular-nums w-12">
        {format ? format(value) : value}
      </span>
    </>
  );
}

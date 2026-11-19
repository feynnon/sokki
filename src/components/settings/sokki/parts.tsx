import React from "react";

/** The page's name at the top, like Superwhisper's. */
export const PageHeader: React.FC<{
  title: string;
  description?: string;
  children?: React.ReactNode;
}> = ({ title, description, children }) => (
  <header className="flex items-start justify-between gap-4 px-1">
    <div className="min-w-0">
      <h1 className="font-display text-title font-[650] leading-[1.15] tracking-[-0.015em]">
        {title}
      </h1>
      {description && (
        <p className="mt-1 max-w-[60ch] text-body text-ink-2">{description}</p>
      )}
    </div>
    {children}
  </header>
);

/** A few mutually exclusive choices side by side. */
export function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (value: T) => void;
  label: string;
}) {
  return (
    // Same height and minimum width as a dropdown; equal segments; the chosen
    // one is raised above the track, never sunk into it.
    <div
      role="radiogroup"
      aria-label={label}
      className="grid grid-flow-col auto-cols-fr h-[var(--vb-control-h)] min-w-[var(--vb-control-w)] rounded-[8px] border border-line bg-wash p-[2px] text-[13px] font-medium"
    >
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={value === o.value}
          onClick={() => onChange(o.value)}
          className={`h-full cursor-pointer whitespace-nowrap rounded-[6px] px-3 transition-colors duration-150 focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary ${
            value === o.value
              ? "bg-[var(--vb-segment)] text-text shadow-[0_1px_2px_rgba(0,0,0,0.25)]"
              : "text-ink-2 hover:text-text"
          }`}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

/** The whole page column. */
export const Page: React.FC<{ children: React.ReactNode }> = ({ children }) => (
  <div className="w-full max-w-[680px] mx-auto flex flex-col gap-7 pt-4 pb-10">
    {children}
  </div>
);

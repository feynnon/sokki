import React from "react";

/** The page's name at the top, like Superwhisper's. */
export const PageHeader: React.FC<{
  title: string;
  description?: string;
  children?: React.ReactNode;
}> = ({ title, description, children }) => (
  <header className="flex items-start justify-between gap-4 px-1">
    <div className="min-w-0">
      <h1 className="text-[22px] font-semibold tracking-[-0.01em]">{title}</h1>
      {description && (
        <p className="text-[13px] text-ink-3 mt-1 max-w-[60ch]">
          {description}
        </p>
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
    <div
      role="radiogroup"
      aria-label={label}
      className="inline-flex rounded-[9px] bg-wash p-[2px] text-[13px] font-medium"
    >
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={value === o.value}
          onClick={() => onChange(o.value)}
          className={`h-[28px] cursor-pointer rounded-[7px] px-3 focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary ${
            value === o.value
              ? "bg-background text-text shadow-sm"
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

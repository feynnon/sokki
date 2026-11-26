import React, { useId, useState } from "react";
import { ChevronRight } from "lucide-react";

/** Sokki's one pattern for explaining a setting in more than a sentence:
 * a question under the description ("What is stored?") that opens the
 * details inside the same card. Closed until the user asks. */
export interface SettingDetails {
  /** The question, e.g. "What is stored?". */
  label: string;
  content: React.ReactNode;
}

interface SettingContainerProps {
  title: string;
  description: string;
  children: React.ReactNode;
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
  layout?: "horizontal" | "stacked";
  disabled?: boolean;
  tooltipPosition?: "top" | "bottom";
  /** Horizontal only: the control takes the row's free width, for content that fits itself to it. */
  grow?: boolean;
  details?: SettingDetails;
}

export const SettingContainer: React.FC<SettingContainerProps> = ({
  title,
  description,
  children,
  descriptionMode = "tooltip",
  grouped = false,
  layout = "horizontal",
  disabled = false,
  tooltipPosition = "top",
  grow = false,
  details,
}) => {
  const [open, setOpen] = useState(false);
  const panelId = useId();
  const toggle = details && (
    <button
      type="button"
      aria-expanded={open}
      aria-controls={panelId}
      onClick={() => setOpen((o) => !o)}
      className="mt-1 inline-flex items-center gap-1 text-[12.5px] leading-[18px] font-medium text-logo-primary cursor-pointer hover:underline underline-offset-[3px]"
    >
      {details.label}
      <ChevronRight
        width={14}
        height={14}
        aria-hidden="true"
        className={`transition-transform duration-150 motion-reduce:transition-none ${open ? "rotate-90" : ""}`}
      />
    </button>
  );
  const panel = details && open && (
    <div
      id={panelId}
      role="region"
      aria-label={details.label}
      className="mx-4 mb-3 rounded-[10px] bg-wash px-3.5 py-3"
    >
      {details.content}
    </div>
  );
  // Title with its explanation underneath; the control on the right.
  const label = (
    <div className={`min-w-0 ${disabled ? "opacity-50" : ""}`}>
      <h3 className="text-[14px] font-medium leading-5">{title}</h3>
      {description && (
        <p className="text-[12.5px] leading-[18px] text-ink-3 mt-0.5">
          {description}
        </p>
      )}
      {toggle}
    </div>
  );
  if (layout === "stacked") {
    return (
      <div className={grouped ? "" : "vb-card"}>
        <div className="px-4 py-3">
          <div className="mb-2">{label}</div>
          <div className="w-full">{children}</div>
        </div>
        {panel}
      </div>
    );
  }
  // Every control sits in a right column at least --vb-control-w wide, so
  // dropdowns, pickers and keys share one right edge and one height.
  const row = (
    <div
      className={`group flex items-center justify-between gap-8 min-h-[60px] px-4 py-3 ${grouped || details ? "" : "vb-card"}`}
    >
      <div className="min-w-0 flex-1">{label}</div>
      <div
        className={
          grow
            ? "relative flex min-w-0 flex-1 justify-end"
            : "relative flex shrink-0 justify-end min-w-[var(--vb-control-w)]"
        }
      >
        {children}
      </div>
    </div>
  );
  if (!details) return row;
  return (
    <div className={grouped ? "" : "vb-card"}>
      {row}
      {panel}
    </div>
  );
};

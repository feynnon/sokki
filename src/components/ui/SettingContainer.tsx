import React from "react";
import { InfoTooltip } from "./InfoTooltip";

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
}) => {
  // Title with its explanation underneath; the control on the right.
  const label = (
    <div className={`min-w-0 ${disabled ? "opacity-50" : ""}`}>
      <h3 className="text-[14px] font-medium leading-5">{title}</h3>
      {description && (
        <p className="text-[12.5px] leading-[18px] text-ink-3 mt-0.5">
          {description}
        </p>
      )}
    </div>
  );
  if (layout === "stacked") {
    return (
      <div className={grouped ? "px-4 py-3" : "px-4 py-3 vb-card"}>
        <div className="mb-2">{label}</div>
        <div className="w-full">{children}</div>
      </div>
    );
  }
  // Every control sits in a right column at least --vb-control-w wide, so
  // dropdowns, pickers and keys share one right edge and one height.
  return (
    <div
      className={`group flex items-center justify-between gap-8 min-h-[60px] px-4 py-3 ${grouped ? "" : "vb-card"}`}
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
};

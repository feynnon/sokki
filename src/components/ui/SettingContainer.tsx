import React from "react";
import { InfoTooltip } from "./InfoTooltip";
import { IS_VERBATIM } from "@/lib/brand";

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
  if (IS_VERBATIM) {
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
    return (
      <div
        className={`flex items-center justify-between gap-6 min-h-[56px] px-4 py-2.5 ${grouped ? "" : "vb-card"}`}
      >
        <div className="min-w-0 flex-1">{label}</div>
        <div
          className={
            grow
              ? "relative flex min-w-0 flex-1 justify-end"
              : "relative shrink-0"
          }
        >
          {children}
        </div>
      </div>
    );
  }
  const info = (
    <InfoTooltip
      text={description}
      placement={tooltipPosition === "bottom" ? "below" : "above"}
    />
  );

  const containerClasses = grouped
    ? "px-4 p-2"
    : "px-4 p-2 rounded-lg border border-line";

  if (layout === "stacked") {
    if (descriptionMode === "tooltip") {
      return (
        <div className={containerClasses}>
          <div className="flex items-center gap-2 mb-2">
            <h3
              className={`text-sm font-medium ${disabled ? "opacity-50" : ""}`}
            >
              {title}
            </h3>
            {info}
          </div>
          <div className="w-full">{children}</div>
        </div>
      );
    }

    return (
      <div className={containerClasses}>
        <div className="mb-2">
          <h3 className={`text-sm font-medium ${disabled ? "opacity-50" : ""}`}>
            {title}
          </h3>
          <p className={`text-sm ${disabled ? "opacity-50" : ""}`}>
            {description}
          </p>
        </div>
        <div className="w-full">{children}</div>
      </div>
    );
  }

  // Horizontal layout (default)
  const horizontalContainerClasses = `${
    grouped
      ? "flex items-center justify-between min-h-12 px-4 p-2"
      : "flex items-center justify-between min-h-12 px-4 p-2 rounded-lg border border-line"
  }${grow ? " gap-4" : ""}`;
  const titleClasses = grow ? "max-w-2/3 shrink-0" : "max-w-2/3";
  const controlClasses = grow
    ? "relative flex min-w-0 flex-1 justify-end"
    : "relative";

  if (descriptionMode === "tooltip") {
    return (
      <div className={horizontalContainerClasses}>
        <div className={titleClasses}>
          <div className="flex items-center gap-2">
            <h3
              className={`text-sm font-medium ${disabled ? "opacity-50" : ""}`}
            >
              {title}
            </h3>
            {info}
          </div>
        </div>
        <div className={controlClasses}>{children}</div>
      </div>
    );
  }

  return (
    <div className={horizontalContainerClasses}>
      <div className={titleClasses}>
        <h3 className={`text-sm font-medium ${disabled ? "opacity-50" : ""}`}>
          {title}
        </h3>
        <p className={`text-sm ${disabled ? "opacity-50" : ""}`}>
          {description}
        </p>
      </div>
      <div className={controlClasses}>{children}</div>
    </div>
  );
};

import React from "react";
import { IS_VERBATIM } from "@/lib/brand";

interface SettingsGroupProps {
  title?: string;
  description?: string;
  children: React.ReactNode;
}

export const SettingsGroup: React.FC<SettingsGroupProps> = ({
  title,
  description,
  children,
}) => {
  if (IS_VERBATIM) {
    // Superwhisper-like: a plain heading over a soft card.
    return (
      <section className="space-y-2">
        {title && (
          <div className="px-1">
            <h2 className="text-[13px] font-semibold text-ink-2">{title}</h2>
            {description && (
              <p className="text-xs text-ink-3 mt-0.5">{description}</p>
            )}
          </div>
        )}
        <div className="vb-card overflow-visible">
          <div className="divide-y divide-line">{children}</div>
        </div>
      </section>
    );
  }
  return (
    <div className="space-y-2">
      {title && (
        <div className="px-4">
          <h2 className="text-xs font-medium text-ink-3 uppercase tracking-wide">
            {title}
          </h2>
          {description && (
            <p className="text-xs text-ink-3 mt-1">{description}</p>
          )}
        </div>
      )}
      <div className="bg-background border border-line rounded-lg overflow-visible">
        <div className="divide-y divide-line">{children}</div>
      </div>
    </div>
  );
};

import React from "react";

/** The Sokki mark: a sound wave stepping down into a V. */
export const SokkiMark: React.FC<{ size?: number; className?: string }> = ({
  size = 22,
  className,
}) => {
  const bars: [number, number, number][] = [
    [5, 10, 22],
    [13, 16, 22],
    [21, 22, 22],
    [29.5, 28, 26],
    [38, 22, 22],
    [46, 16, 22],
    [54, 10, 22],
  ];
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 64 64"
      className={className}
      aria-hidden="true"
    >
      {bars.map(([x, y, h], i) => (
        <rect
          key={x}
          x={x}
          y={y}
          width={5}
          height={h}
          rx={2.5}
          fill={i === 3 ? "var(--color-logo-primary)" : "currentColor"}
        />
      ))}
    </svg>
  );
};

/** Mark and name: small for the sidebar, large for the welcome screen. */
export const SokkiLogo: React.FC<{ className?: string; large?: boolean }> = ({
  className,
  large = false,
}) => (
  <div
    className={`flex items-center text-text ${large ? "gap-[10px]" : "gap-2"} ${className ?? ""}`}
    aria-label="Sokki"
    role="img"
  >
    <SokkiMark size={large ? 44 : 22} />
    <span
      className={`font-bold tracking-[-0.02em] ${large ? "text-[28px]" : "text-[19px]"}`}
    >
      {"Sokki"}
    </span>
  </div>
);

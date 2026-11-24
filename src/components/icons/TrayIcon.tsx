/** The menu bar icon, drawn like src-tauri/brands/sokki/src/tray_*.svg: seven
 * bars in currentColor (like a macOS template image), the centre bar red while
 * recording and blue while transcribing. */
type TrayIconState = "idle" | "recording" | "transcribing";

const BARS: [number, number, number][] = [
  [5, 11, 22],
  [13, 17, 22],
  [21, 23, 22],
  [29.5, 29, 26],
  [38, 23, 22],
  [46, 17, 22],
  [54, 11, 22],
];

const CENTRE: Record<TrayIconState, string> = {
  idle: "currentColor",
  recording: "#FF5F57",
  transcribing: "var(--color-logo-primary)",
};

const TrayIcon = ({
  state,
  size,
}: {
  state: TrayIconState;
  size: number | string;
}) => (
  <svg
    width={size}
    height={size}
    viewBox="0 0 64 64"
    className="fill-current"
    xmlns="http://www.w3.org/2000/svg"
    aria-hidden="true"
  >
    {BARS.map(([x, y, h], i) => (
      <rect
        key={x}
        x={x}
        y={y}
        width={5}
        height={h}
        rx={2.5}
        fill={i === 3 ? CENTRE[state] : "currentColor"}
      />
    ))}
  </svg>
);

export default TrayIcon;

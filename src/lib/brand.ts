/**
 * Build-time brand profile (see src-tauri/src/brand.rs). The default build is
 * Handy; `VITE_HANDY_BRAND=verbatim` builds "Verbatim" with its own logo and
 * colours. Importing this module tags the document so the brand's CSS applies.
 */
import "../styles/brand-verbatim.css";

export type Brand = "handy" | "verbatim";

export const BRAND: Brand =
  import.meta.env.VITE_HANDY_BRAND === "verbatim" ? "verbatim" : "handy";

export const IS_VERBATIM = BRAND === "verbatim";

if (typeof document !== "undefined") {
  document.documentElement.dataset.brand = BRAND;
}

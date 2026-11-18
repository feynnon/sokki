/**
 * Build-time brand profile (see src-tauri/src/brand.rs). The default build is
 * Handy; `VITE_HANDY_BRAND=sokki` builds "Sokki" with its own logo and
 * colours. Importing this module tags the document so the brand's CSS applies.
 */
import "../styles/brand-sokki.css";

export type Brand = "handy" | "sokki";

export const BRAND: Brand =
  import.meta.env.VITE_HANDY_BRAND === "sokki" ? "sokki" : "handy";

export const IS_SOKKI = BRAND === "sokki";

if (typeof document !== "undefined") {
  document.documentElement.dataset.brand = BRAND;
}

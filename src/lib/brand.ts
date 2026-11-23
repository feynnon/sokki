/**
 * Sokki's look (see src-tauri/src/brand.rs). Importing this module tags the
 * document so the brand's CSS applies.
 */
import "../styles/brand-sokki.css";

if (typeof document !== "undefined") {
  document.documentElement.dataset.brand = "sokki";
}

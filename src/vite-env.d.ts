/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Build-time brand: "verbatim", or unset for Handy (src/lib/brand.ts). */
  readonly VITE_HANDY_BRAND?: string;
}

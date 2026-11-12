import { defineConfig } from "astro/config";

const site = new URL(process.env.SITE_URL || "https://cranklink.plainoss.app");
if (
  !["https:", "http:"].includes(site.protocol) ||
  site.pathname !== "/" ||
  site.search ||
  site.hash ||
  site.username ||
  site.password
) {
  throw new Error(
    "SITE_URL must be an HTTP(S) origin, without a path, query or fragment.",
  );
}

export default defineConfig({
  output: "static",
  site: site.href,
  trailingSlash: "always",
  build: { inlineStylesheets: "never" },
});

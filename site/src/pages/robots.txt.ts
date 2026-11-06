import type { APIRoute } from "astro";

export const GET: APIRoute = ({ site }) =>
  new Response(
    process.env.SITE_NOINDEX === "1"
      ? "User-agent: *\nDisallow: /\n"
      : `User-agent: *\nAllow: /\nSitemap: ${new URL("sitemap.xml", site)}\n`,
    { headers: { "Content-Type": "text/plain; charset=utf-8" } },
  );

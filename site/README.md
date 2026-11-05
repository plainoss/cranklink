# Cranklink project website

The static Astro website uses the reviewed Field notes design: “Your bike. Your settings.”, early web app and Google Play links, supported settings, platform choice, source modification, recovery, and expandable hardware compatibility. It is separate from the browser application in `web-instrument/`.

From `site/`, with Node.js 22.23.4 or newer:

```sh
npm ci
npm run dev               # local development
npm run check             # Astro and TypeScript diagnostics
npm test                  # README compatibility parser regressions
npm run build             # static output plus assets/link checks
npm run test:browser      # production output served by Wrangler; build first
```

Browser tests run with JavaScript disabled on desktop and phone viewports. Set `CRANKLINK_SITE_TEST_PORT` to avoid a port conflict; tests never reuse or stop another server. `npm run preview` serves the generated site for inspection. `npm run build:cloudflare` installs locked dependencies and runs the same typecheck, unit tests and build used by hosting.

The complete motor and other-component tables are imported from the root `README.md` at build time. `src/lib/compatibility.ts` validates their headings, columns and row shape; source-format changes fail the build rather than silently producing stale or mislabelled data. Images are imported from `docs/images/` and optimized by Astro. No copied compatibility JSON, external fonts, client runtime, or accounts are required.

Set `SITE_URL=https://your-domain.example` when building for a custom domain. This controls canonical, Open Graph, sitemap and robots URLs. It must be an HTTP(S) origin with no path, query or fragment. `SITE_NOINDEX=1` marks review builds noindex and disallows crawling; production defaults to indexing.

The Worker name, account and static routing are in `wrangler.jsonc`. `public/_headers` supplies a script-free CSP and immutable caching for hashed assets. It does not add the browser app's COOP/COEP requirements to this site. `404.astro` is served as a real 404 by Workers static assets.

See [the implementation and hosting plan](../docs/project-site.md) for CI, Cloudflare Workers Builds and deployment setup. `cloudflare-builds.json` records the desired dashboard/API settings; it is a setup reference, not a file Wrangler reads automatically.

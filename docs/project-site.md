# Project website: implementation and hosting

## Scope and implementation plan

1. Build the approved Field notes homepage in `site/` using static Astro output. Preserve the early web app and Google Play links, the “Your bike. Your settings.” headline, settings/platform/source benefits, recovery highlight and project origin. Remove draft-switching UI. The origin story does not link to the later resistor/bridge repair.
2. Read the current motor and other-component compatibility tables directly from `README.md`. Keep the short overview and put complete tables, validation status and recovery footnotes inside a keyboard-accessible native disclosure. Fail builds when the source table format changes.
3. Add production metadata, local image optimization, a favicon, robots/sitemap output, a real 404 page, and static response headers. The homepage works without client JavaScript.
4. Validate Astro types, compatibility parsing, built assets and links, then test the production artifact on desktop and phone with JavaScript disabled. Include the site in the repository's unified verification command and GitHub merge-queue validation.
5. Host a separate `cranklink-site` static-assets Worker in the existing `plainoss` Cloudflare account. Connect Workers Builds to `plainoss/cranklink`, with `main` as the production branch and other branches as previews. Leave the change unmerged until reviewed; do not enable auto-merge or queue it.

## Build and validation

Local commands are in [site/README.md](../site/README.md). `./tools/verify.sh` includes site types, compatibility parser tests, and the static build/asset checks. The **Validate Project Site** workflow also runs production browser tests through Wrangler. It triggers for pushes to `main`, pull requests, merge groups and manual runs. Its job always exists; an internal changed-path filter skips work when the site, source README and source images are unaffected.

GitHub validation does not need Cloudflare secrets. Fork PRs can run the same checks, and build artifacts are not deployed by untrusted PR code. The separate Workers Builds integration owns hosting. Its build command also runs the typecheck, unit tests and artifact checks before deploying.

## Cloudflare hosting

The checked-in [Wrangler configuration](../site/wrangler.jsonc) uses:

| Setting | Value |
| --- | --- |
| Account | `plainoss` (`60d502e306ecf0bdad0bb3719d83b30c`) |
| Worker | `cranklink-site` |
| Default URL | `https://cranklink-site.plainoss.workers.dev/` |
| Static assets | `site/dist/` |
| HTML routing | Force trailing slashes; serve `404.html` for missing pages |
| Server adapter / Worker script | None |

The existing app stays at `https://cranklink.plainoss.workers.dev/`. Deploying this Worker does not replace that app or configure a custom domain.

For a manual deployment, authenticate Wrangler for the account and run from `site/`:

```sh
npm run build:cloudflare
npm run deploy
```

Publish a named review preview without replacing its production deployment (Wrangler can create the new Worker and preview together):

```sh
SITE_NOINDEX=1 npm run build
npm run deploy:preview -- --name astro-project-site
```

Use the URL returned by Wrangler. Rebuild without `SITE_NOINDEX=1` before a production deployment. Keep the canonical URL pointed at the intended production origin even for review previews.

The initial review build is hosted at <https://astro-project-site-cranklink-site.plainoss.workers.dev/>. It is marked noindex. The production URL is reserved for deployment from the reviewed `main` branch; creating this preview does not merge the PR.

## Automatic production and preview builds

Connect **only the new `cranklink-site` Worker** to the existing GitHub repository in Cloudflare's **Settings → Builds**. Configure production branch `main`, enable previews for non-production branches, and apply the checked-in [build settings reference](../site/cloudflare-builds.json):

| Setting | Production | Preview |
| --- | --- | --- |
| Root directory | `site` | `site` |
| Build command | `npm run build:cloudflare` | `npm run build:cloudflare` |
| Deploy command | `npx wrangler deploy` | `npx wrangler preview` |
| `SKIP_DEPENDENCY_INSTALL` | `1` | `1` |
| `NODE_VERSION` | `22.23.4` | `22.23.4` |
| `SITE_NOINDEX` | `0` | `1` |
| Watch paths | `site/*`, `README.md`, `docs/images/*`, `docs/images/ep800-recovery/*` | Same |

Watch paths are repository-relative, including the README and source images outside `site/`. Configure a deployment build token for this Worker in Cloudflare; do not put credentials in Git or GitHub PR workflows. When the Cloudflare GitHub App is already connected to `plainoss/cranklink` for the browser app, reuse that repository authorization rather than changing the app's existing Worker configuration.

Cloudflare's [Builds API reference](https://developers.cloudflare.com/workers/ci-cd/builds/api-reference/) supports the same settings, but requires a user-scoped token with **Workers Builds Configuration: Edit**, plus **Workers Scripts: Read** to look up the Worker tag. A Wrangler OAuth login can deploy Workers while still being refused by the Builds API. In that case, use a signed-in dashboard session or an appropriately scoped token; do not substitute or expose the OAuth token in GitHub secrets.

Before merge, verify the branch preview, the **Build and test project site** check and the existing required checks. After review and merge, the production-branch integration deploys the tested source. Merely opening the PR does not merge it or add it to the merge queue.

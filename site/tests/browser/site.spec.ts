import { expect, test } from "@playwright/test";

test("apps appear before the screenshot and compatibility opens with the keyboard", async ({
  page,
}) => {
  const response = await page.goto("/");
  expect(response?.status()).toBe(200);
  expect(response?.headers()["content-security-policy"]).toContain(
    "default-src 'none'",
  );
  const hero = page.locator(".fieldhero");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(
    "Your bike.Your settings.",
  );
  await expect(
    hero.getByRole("link", { name: "Open web app" }),
  ).toHaveAttribute("href", "https://cranklink.plainoss.workers.dev/");
  await expect(
    hero.getByRole("link", { name: "Get it on Google Play" }),
  ).toHaveAttribute(
    "href",
    "https://play.google.com/store/apps/details?id=app.plainoss.cranklink",
  );
  expect(
    await hero
      .getByRole("link", { name: "Get it on Google Play" })
      .evaluate((el) =>
        Boolean(
          el.compareDocumentPosition(document.querySelector(".fieldvisual")!) &
          Node.DOCUMENT_POSITION_FOLLOWING,
        ),
      ),
  ).toBe(true);

  const disclosure = page.locator("details.full-compatibility");
  const summary = disclosure.locator("summary");
  await expect(disclosure).not.toHaveAttribute("open");
  await summary.focus();
  await page.keyboard.press("Enter");
  await expect(disclosure).toHaveAttribute("open", "");
  await expect(disclosure.getByRole("table")).toHaveCount(2);
  await expect(
    disclosure.getByRole("cell", {
      name: "Nothing; identified only",
      exact: true,
    }),
  ).toBeVisible();
  await expect(
    disclosure.getByText("Command line only, with", { exact: false }),
  ).toBeVisible();
  expect(
    await page.evaluate(
      () =>
        document.documentElement.scrollWidth <=
        document.documentElement.clientWidth,
    ),
  ).toBe(true);
  const scroll = page.getByRole("region", { name: "Motors", exact: true });
  await scroll.focus();
  if (await scroll.evaluate((el) => el.scrollWidth > el.clientWidth)) {
    await page.keyboard.press("ArrowRight");
    await expect
      .poll(() => scroll.evaluate((el) => el.scrollLeft))
      .toBeGreaterThan(0);
  }
  await summary.focus();
  await page.keyboard.press("Enter");
  await expect(disclosure).not.toHaveAttribute("open");
});

test("images load, local resources work and missing routes return a real 404", async ({
  page,
  request,
}) => {
  await page.goto("/");
  for (const image of await page.getByRole("img").all()) {
    await image.scrollIntoViewIfNeeded();
    await expect
      .poll(() => image.evaluate((el) => (el as HTMLImageElement).naturalWidth))
      .toBeGreaterThan(0);
  }
  const missing = await page.goto("/not-a-real-page/");
  expect(missing?.status()).toBe(404);
  await expect(
    page.getByRole("heading", { name: "Page not found." }),
  ).toBeVisible();
  await page.getByRole("link", { name: "Back to cranklink" }).click();
  await expect(page).toHaveURL("/");
  for (const path of ["/robots.txt", "/sitemap.xml", "/favicon.svg"]) {
    expect((await request.get(path)).status(), path).toBe(200);
  }
});

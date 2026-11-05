import assert from "node:assert/strict";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parse } from "parse5";

const root = fileURLToPath(new URL("../dist/", import.meta.url));
const repository = fileURLToPath(new URL("../../", import.meta.url));
const htmlFiles = [];
function walk(directory) {
  for (const entry of readdirSync(directory)) {
    const file = path.join(directory, entry);
    if (statSync(file).isDirectory()) walk(file);
    else if (file.endsWith(".html")) htmlFiles.push(file);
  }
}
walk(root);
assert.ok(
  htmlFiles.some((file) => path.relative(root, file) === "index.html"),
  "Missing homepage",
);
assert.ok(existsSync(path.join(root, "404.html")), "Missing 404 page");
assert.ok(
  existsSync(path.join(root, "_headers")),
  "Missing Cloudflare headers",
);

function elements(node) {
  return [
    ...(node.tagName ? [node] : []),
    ...(node.childNodes || []).flatMap(elements),
  ];
}
for (const file of htmlFiles) {
  const nodes = elements(parse(readFileSync(file, "utf8")));
  const attr = (node, name) => node.attrs?.find((a) => a.name === name)?.value;
  const ids = new Set(nodes.map((node) => attr(node, "id")).filter(Boolean));
  assert.equal(
    nodes.filter((node) => node.tagName === "h1").length,
    1,
    `Exactly one h1: ${file}`,
  );
  assert.ok(
    !nodes.some((node) => node.tagName === "script"),
    `Site must work without scripts: ${file}`,
  );
  assert.ok(
    !nodes.some(
      (node) => node.tagName === "style" || attr(node, "style") !== undefined,
    ),
    `CSP forbids inline styles: ${file}`,
  );
  assert.ok(
    nodes.some(
      (node) => node.tagName === "link" && attr(node, "rel") === "canonical",
    ),
    `Missing canonical: ${file}`,
  );
  for (const node of nodes) {
    const references = ["href", "src"]
      .map((name) => attr(node, name))
      .filter(Boolean);
    if (attr(node, "srcset"))
      references.push(
        ...attr(node, "srcset")
          .split(",")
          .map((part) => part.trim().split(/\s+/)[0]),
      );
    for (const reference of references) {
      // Repository links are checked locally; external URLs are never fetched in a build.
      const repoPrefix = "https://github.com/plainoss/cranklink/blob/main/";
      if (reference.startsWith(repoPrefix)) {
        assert.ok(
          existsSync(
            path.join(
              repository,
              reference.slice(repoPrefix.length).split("#")[0],
            ),
          ),
          `Broken repository link: ${reference}`,
        );
        continue;
      }
      if (/^(?:[a-z][a-z\d+.-]*:|\/\/)/i.test(reference)) continue;
      const url = new URL(
        reference,
        `https://local.invalid/${path.relative(root, file)}`,
      );
      const local = path.join(root, decodeURIComponent(url.pathname));
      const target = local.endsWith(path.sep)
        ? path.join(local, "index.html")
        : local;
      assert.ok(
        existsSync(target),
        `Missing local asset or page: ${reference} in ${file}`,
      );
      if (url.hash && path.resolve(target) === path.resolve(file)) {
        assert.ok(
          ids.has(decodeURIComponent(url.hash.slice(1))),
          `Missing fragment: ${reference} in ${file}`,
        );
      }
    }
    if (node.tagName === "img") {
      assert.ok(attr(node, "alt"), `Missing image description: ${file}`);
      assert.ok(
        Number(attr(node, "width")) > 0 && Number(attr(node, "height")) > 0,
        `Missing image dimensions: ${file}`,
      );
    }
  }
}
console.log(
  `Validated ${htmlFiles.length} static HTML pages, image assets, local/repository links and script-free CSP compatibility.`,
);

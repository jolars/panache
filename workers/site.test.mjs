import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { after, before, test } from "node:test";
import { createTestHarness } from "wrangler";

const server = createTestHarness({
  workers: [{ configPath: new URL("./wrangler.jsonc", import.meta.url) }],
});

before(async () => {
  await server.listen();
});
after(async () => {
  await server.close();
});

async function request(path, options = {}) {
  const response = await server.fetch(new URL(path, "https://panache.bz"), {
    redirect: "manual",
    ...options,
  });
  // Drain runtime streams so fixture responses cannot delay server shutdown.
  const body = await response.arrayBuffer();
  return new Response(
    [204, 205, 304].includes(response.status) ? null : body,
    response,
  );
}

test("serves every published sitemap URL with its existing canonical", async () => {
  const sitemap = await readFile(
    new URL("../docs/_site/sitemap.xml", import.meta.url),
    "utf8",
  );
  const urls = [...sitemap.matchAll(/<loc>([^<]+)<\/loc>/g)].map(
    (match) => match[1],
  );
  assert.ok(urls.length >= 22);
  for (const url of urls) {
    const response = await request(url);
    assert.equal(response.status, 200, url);
    const html = await response.text();
    const canonical = [...html.matchAll(/<link\b[^>]*>/g)]
      .map((match) => match[0])
      .find((tag) => /\brel=["']canonical["']/.test(tag));
    assert.ok(canonical?.includes(`href="${url}"`), url);
  }
});

test("redirects legacy documentation, directory aliases, and extensionless pages", async () => {
  for (const [path, target] of [
    ["/user-guide/configuration.html", "/reference/configuration.html"],
    ["/user-guide/integrations.html", "/guide/integrations.html"],
    ["/user-guide/lsp", "/guide/lsp.html"],
    ["/user-guide/formatting.html", "/guide/formatting.html"],
    ["/guide/configuration.html", "/reference/configuration.html"],
    ["/index.html", "/"],
    ["/guide/index.html", "/guide/"],
    ["/reference/index.html", "/reference/"],
    ["/development/index.html", "/development/"],
    ["/playground/index.html", "/playground/"],
    ["/guide", "/guide/"],
    ["/getting-started", "/getting-started.html"],
    ["/guide/lsp", "/guide/lsp.html"],
  ]) {
    const response = await request(`${path}?source=old-link`);
    assert.equal(response.status, 301, path);
    assert.equal(
      response.headers.get("Location"),
      `https://panache.bz${target}?source=old-link`,
      path,
    );
    assert.equal((await request(target)).status, 200, target);
  }
  const www = await request(
    "https://www.panache.bz/guide/lsp.html?editor=nvim",
  );
  assert.equal(www.status, 301);
  assert.equal(
    www.headers.get("Location"),
    "https://panache.bz/guide/lsp.html?editor=nvim",
  );
});

test("preserves heading anchors linked from the README", async () => {
  for (const [path, anchor] of [
    ["/getting-started.html", "mise-and-aqua"],
    ["/guide/lsp.html", "rstudio"],
    ["/guide/integrations.html", "pre-commit"],
  ]) {
    const html = await (await request(path)).text();
    assert.ok(html.includes(`id="${anchor}"`), `${path}#${anchor}`);
  }
});

test("negotiates complete Markdown for GET and HEAD without HTML cache validators", async () => {
  for (const path of [
    "/",
    "/guide/",
    "/getting-started.html",
    "/reference/configuration.html",
  ]) {
    const html = await request(path);
    assert.match(html.headers.get("Content-Type"), /text\/html/);
    assert.match(html.headers.get("Vary"), /Accept/);
    for (const method of ["GET", "HEAD"]) {
      const markdown = await request(`${path}?agent=1`, {
        method,
        headers: {
          Accept: "text/markdown",
          "If-None-Match": html.headers.get("ETag"),
          Range: "bytes=0-20",
        },
      });
      assert.equal(markdown.status, 200, `${method} ${path}`);
      assert.match(markdown.headers.get("Content-Type"), /text\/markdown/);
      assert.match(markdown.headers.get("Vary"), /Accept/);
      const body = await markdown.text();
      if (method === "HEAD") assert.equal(body, "");
      else assert.ok(body.length > 100, path);
    }
  }
});

test("preserves public downloads, content types, and CORS", async () => {
  for (const [path, source, contentType] of [
    ["/install", "../scripts/panache-installer.sh", /text\/plain/],
    ["/install.ps1", "../scripts/panache-installer.ps1", /text\/plain/],
    [
      "/panache.schema.json",
      "../docs/panache.schema.json",
      /application\/json/,
    ],
    ["/funding.json", "../funding.json", /application\/json/],
  ]) {
    const response = await request(path);
    assert.equal(response.status, 200, path);
    assert.match(response.headers.get("Content-Type"), contentType, path);
    assert.equal(
      response.headers.get("Access-Control-Allow-Origin"),
      "*",
      path,
    );
    assert.deepEqual(
      Buffer.from(await response.arrayBuffer()),
      await readFile(new URL(source, import.meta.url)),
      path,
    );
  }
  const directMarkdown = await request("/guide/formatting.llms.md");
  assert.equal(directMarkdown.status, 200);
  assert.match(directMarkdown.headers.get("Content-Type"), /text\/markdown/);
});

test("serves search and the playground's JavaScript and WebAssembly", async () => {
  const search = await request("/search.json");
  assert.equal(search.status, 200);
  assert.ok((await search.json()).length > 0);
  const playground = await request("/playground/", {
    headers: { Accept: "text/markdown" },
  });
  assert.equal(playground.status, 200);
  assert.match(playground.headers.get("Content-Type"), /text\/html/);
  for (const [path, type] of [
    ["/playground/pkg/panache_wasm.js", /javascript/],
    ["/playground/pkg/panache_wasm_bg.wasm", /application\/wasm/],
  ]) {
    const response = await request(path);
    assert.equal(response.status, 200, path);
    assert.match(response.headers.get("Content-Type"), type, path);
  }
});

test("keeps unknown pages and unsafe requests from becoming successful HTML pages", async () => {
  for (const path of [
    "/missing-page.html",
    "/missing-directory/",
    "/missing-page",
    "/missing.llms.md",
  ]) {
    assert.equal((await request(path)).status, 404, path);
  }
  const response = await request("/getting-started.html", { method: "POST" });
  assert.ok(response.status >= 400);
});

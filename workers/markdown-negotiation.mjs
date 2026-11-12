const pageRedirects = new Map([
  ["/user-guide", "/guide/"],
  ["/user-guide/", "/guide/"],
  ["/user-guide/index.html", "/guide/"],
  ["/user-guide/configuration", "/reference/configuration.html"],
  ["/user-guide/configuration.html", "/reference/configuration.html"],
  ["/user-guide/integrations", "/guide/integrations.html"],
  ["/user-guide/integrations.html", "/guide/integrations.html"],
  ["/user-guide/lsp", "/guide/lsp.html"],
  ["/user-guide/lsp.html", "/guide/lsp.html"],
  ["/user-guide/formatting", "/guide/formatting.html"],
  ["/user-guide/formatting.html", "/guide/formatting.html"],
  ["/guide/configuration", "/reference/configuration.html"],
  ["/guide/configuration.html", "/reference/configuration.html"],
  ["/guide", "/guide/"],
  ["/reference", "/reference/"],
  ["/development", "/development/"],
  ["/index.html", "/"],
  ["/guide/index.html", "/guide/"],
  ["/reference/index.html", "/reference/"],
  ["/development/index.html", "/development/"],
  ["/playground", "/playground/"],
  ["/playground/index.html", "/playground/"],
]);

function acceptsMarkdown(header) {
  return header.split(",").some((part) => {
    const [type, ...parameters] = part.trim().toLowerCase().split(";");
    if (type.trim() !== "text/markdown") return false;
    const quality = parameters
      .map((parameter) => parameter.trim().match(/^q\s*=\s*([0-9.]+)$/))
      .find(Boolean);
    return !quality || (Number(quality[1]) > 0 && Number(quality[1]) <= 1);
  });
}

function markdownPath(path) {
  if (path === "/playground" || path.startsWith("/playground/")) return null;
  if (path.endsWith("/")) return `${path}index.llms.md`;
  if (path.endsWith(".html")) return `${path.slice(0, -5)}.llms.md`;
  if (!path.slice(path.lastIndexOf("/") + 1).includes("."))
    return `${path}.llms.md`;
  return null;
}

function varyOnAccept(response, markdown = false) {
  const headers = new Headers(response.headers);
  if (markdown) headers.set("Content-Type", "text/markdown; charset=utf-8");
  const vary = headers.get("Vary");
  if (
    vary !== "*" &&
    !vary?.split(",").some((name) => name.trim().toLowerCase() === "accept")
  ) {
    headers.set("Vary", vary ? `${vary}, Accept` : "Accept");
  }
  return new Response(response.body, { status: response.status, headers });
}

export async function handleRequest(request, fetchAsset) {
  if (request.method !== "GET" && request.method !== "HEAD") {
    return fetchAsset(request);
  }

  const url = new URL(request.url);
  if (url.hostname === "www.panache.bz") {
    url.hostname = "panache.bz";
    url.protocol = "https:";
    return Response.redirect(url.href, 301);
  }

  const redirect = pageRedirects.get(url.pathname);
  if (redirect) {
    url.protocol = "https:";
    url.pathname = redirect;
    return Response.redirect(url.href, 301);
  }

  const path = markdownPath(url.pathname);
  if (!path) {
    return fetchHtml(request, fetchAsset);
  }

  if (acceptsMarkdown(request.headers.get("Accept") ?? "")) {
    const markdownUrl = new URL(request.url);
    markdownUrl.pathname = path;
    markdownUrl.search = "";
    const headers = new Headers(request.headers);
    headers.delete("If-None-Match");
    headers.delete("If-Modified-Since");
    headers.delete("Range");
    const markdown = await fetchAsset(
      new Request(markdownUrl, { method: request.method, headers }),
    );
    if (markdown.ok) return varyOnAccept(markdown, true);
    await markdown.body?.cancel();
  }

  return varyOnAccept(await fetchHtml(request, fetchAsset));
}

async function fetchHtml(request, fetchAsset) {
  const url = new URL(request.url);
  if (url.pathname.endsWith("/")) {
    url.pathname += "index.html";
    return fetchAsset(new Request(url, request));
  }

  const response = await fetchAsset(request);
  if (
    response.status === 404 &&
    !url.pathname.slice(url.pathname.lastIndexOf("/") + 1).includes(".")
  ) {
    // Preserve published .html URLs while accepting their old extensionless aliases.
    url.pathname += ".html";
    const html = await fetchAsset(new Request(url, request));
    const exists = html.ok;
    await html.body?.cancel();
    if (exists) {
      await response.body?.cancel();
      return Response.redirect(url.href, 301);
    }
  }

  return response;
}

export default {
  fetch(request, env) {
    return handleRequest(request, (assetRequest) =>
      env.ASSETS.fetch(assetRequest),
    );
  },
};

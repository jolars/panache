# Website hosting

Cloudflare Workers serves the complete Quarto website from `docs/_site` through
the `ASSETS` binding. The same deployment publishes the HTML, generated
Markdown, playground, installers, schema, and request handler.

The existing Worker name, `panache-markdown-negotiation`, is retained so
deployment history stays together. Its Custom Domains are `panache.bz` and
`www.panache.bz`. Pages on `www` redirect to the canonical hostname.

The assets configuration keeps published `.html` URLs intact. The Worker serves
directory URLs from their `index.html` assets and permanently redirects legacy
documentation paths and directory aliases. Redirects preserve query strings, and
browsers retain fragments when following them. Unknown pages return HTTP 404.

Requests accepting `text/markdown` receive the corresponding `.llms.md` asset,
with `Vary: Accept`. Images, libraries, and playground bundles use direct static
asset serving. Public downloads retain their content types and CORS headers from
`_headers`.

## Build and preview

From the repository's development environment:

```sh
npm ci --prefix workers
compute run -- task docs:build
compute run -- task docs:check
task docs:worker
```

The build script compiles the WASM playground, renders Quarto, copies public
downloads and asset headers, and compares copied files with their sources.
`docs:check` runs request-handler tests, a deployment dry run, and tests of the
built site through the Workers runtime. The runtime tests cover the sitemap,
canonical URLs, old links, Markdown GET and HEAD requests, search, downloads,
WebAssembly, and missing pages. `docs-preview` still supports Quarto's authoring
preview.

On Linux, devenv supplies a loader wrapper for npm's pinned `workerd` binary.
Use `devenv shell -- <command>` when a noninteractive shell has not loaded the
environment. This keeps the runtime version aligned with the lockfile on NixOS.

## Deployment

The Documentation workflow publishes on `v*` release tags and manual dispatches.
It builds and validates the site before deploying the same output with Wrangler.
The normal pull-request documentation job runs these checks without production
credentials.

Configure these repository secrets for the deployment step:

- `CLOUDFLARE_ACCOUNT_ID`: the account that owns the Worker and site zone.
- `CLOUDFLARE_API_TOKEN`: permission to deploy the Worker and update its routes
  and Custom Domains in the `panache.bz` zone.

For a manual deployment after building and checking the site:

```sh
wrangler login
task docs:deploy
```

Custom Domains make Workers the origin and create the required DNS records. When
cutting over from another host, remove conflicting website DNS records for the
two hostnames, attach the Custom Domains, and verify both hosts before disabling
the old host. Remove the old `panache.bz/*` overlay route after the Custom
Domains are active. Unrelated DNS records must remain intact.

Verify the live site after deployment:

```sh
curl -i https://panache.bz/getting-started.html
curl -I https://panache.bz/user-guide/configuration.html
curl -I https://www.panache.bz/guide/lsp.html
curl -i https://panache.bz/getting-started.html -H 'Accept: text/markdown'
curl -I https://panache.bz/panache.schema.json
curl -I https://panache.bz/nonexistent-page.html
```

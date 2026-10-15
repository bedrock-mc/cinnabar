# Cinnabar website

The existing Minecraft styled landing page lives here. Downloads still redirect to
GitHub's latest stable release; Linux displays the release installer command. The
renderer reads the repository's release manifest, title and icon directly, so no
branding or release filename snapshots need synchronization.

```sh
python3 website/render.py
node --test website/tests/app.test.cjs
python3 -m unittest discover -s website/tests -p 'test_*.py' -v
```

`website/dist/` contains only generated site files and is ignored. The Website
workflow tests pull requests and validates `wrangler.jsonc` with a dry run. After
verification, pushes to `dev` that change the site, its branding/release inputs or
the workflow deploy the static-assets Worker using the workflow's pinned Wrangler
version. Manual dispatch can deploy a reviewed branch. App-only changes do not deploy.

The `website` GitHub environment supplies the `CLOUDFLARE_API_TOKEN` secret and
`CLOUDFLARE_ACCOUNT_ID` variable. `wrangler.jsonc` pins the Worker and account,
serves `dist/` without a Worker script, and disables request logs. The workflow
follows HTML redirects and byte-compares every deployed file with the build.

The machine's owner must apply `deploy/Caddyfile` by hand to the shared proxy,
validate the combined configuration, and reload Caddy. It permanently redirects
the old domain to the Worker, preserving the path and query string. Its redirect
target is also the source of the workflow's post-deploy check URL.

# Cinnabar website

The existing Minecraft styled landing page lives here. Downloads still redirect to
GitHub's latest stable release; Linux displays the release installer command. The
renderer reads the repository's release manifest, title and icon directly, so no
branding or release filename snapshots need synchronization.

```sh
python3 website/render.py
node --test website/tests/*.test.cjs
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

The `RestartFU/configurations` repository owns the custom domain's Caddy proxy.
It forwards requests to the Worker over HTTPS while preserving the public domain,
path and query string. No local Go server or SSH deployment is needed. The
workflow's `WEBSITE_URL` selects the destination for post-deploy file checks.

The line below the downloads shows unique connected, visible Cinnabar accounts
from `GET /v1/presence/count`. The API origin comes from the shared bridge contract.
It refreshes once per minute while the page is visible, with a five-second request
timeout. Failed or invalid responses clear the count; background tabs stop polling.
Deploy the API endpoint before the website to enable the count.

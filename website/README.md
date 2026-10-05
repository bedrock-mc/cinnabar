# Cinnabar website

The existing Minecraft styled landing page lives here. Downloads still redirect to
GitHub's latest stable release; Linux displays the release installer command. The
renderer reads the repository's release manifest, title and icon directly, so no
branding or release filename snapshots need synchronization.

```sh
python3 website/render.py
node --test website/tests/app.test.cjs
python3 -m unittest discover -s website/tests -p 'test_*.py' -v
go test website/server/main.go website/server/main_test.go
CGO_ENABLED=0 go build -trimpath -ldflags='-s -w' -o website/dist/server website/server/main.go
```

`website/dist/` is generated and ignored. The Website workflow verifies pull requests
without using deployment credentials. Changes merged into `dev` to website files,
their branding/release inputs or the workflow deploy automatically. Manual dispatch
can deploy a reviewed branch for verification. App-only changes do not deploy.

Like Zeno Practice, deployment uses GitHub hosted jobs and SSH to this machine.
The `website` environment holds `WEBSITE_SSH_KEY`, `WEBSITE_KNOWN_HOSTS`, and the
`WEBSITE_DEPLOY_HOST` variable. The dedicated `cinnabar` account has a locked Unix
password. Its deployment key permits only the root-owned `deploy/receive.py`
receiver, with forwarding and interactive sessions disabled. It has no sudo or
Docker group membership.

The receiver accepts the five generated files plus the CI-built Linux Go server,
rejects paths, links, duplicates, incomplete archives and oversized payloads, then
switches `site/current` atomically. Public files sit in `public/`; the executable
sits outside that directory. Publication serializes and retains five releases.

A bounded, read-only BusyBox container runs the Go server as an unprivileged user.
Its small supervisor restarts only its own Go child when the release pointer changes,
so server code updates deploy with the page without giving the account privileged
restart access. Forwardme keeps routing `cinnabar.restartfu.com` to
`http://cinnabar-site:80` and owns HTTPS. There is no nginx process or image.
The stdlib Go handler preserves the page, MIME types, HEAD and byte-range behavior;
directories, dotfiles and files outside the public root return 404. The workflow
waits for the exact release at `/healthz`, then byte-compares all public assets.

Host setup installs the receiver at `/usr/local/libexec/cinnabar-website-deploy`,
the account's SSH key at `/home/cinnabar/.ssh/authorized_keys`, and the supervisor and
Compose file under `/home/danick/deployments/cinnabar-site/deploy/`. Changes to the host
receiver or container supervisor/Compose settings require a deliberate administrator installation;
normal publication updates the static files and server binary together. Roll back the page and server together by repointing
`/home/cinnabar/site/current` to a retained release as the account owner.

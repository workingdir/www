# www

The source for [cwd.dev](https://cwd.dev). One Rust binary that serves the
website over HTTP and a read-only shell over SSH, both backed by the same tree of
my public repos.

```
ssh cwd.dev                              # browse the projects in a shell
curl cwd.dev                             # the same intro, as text
git clone ssh://cwd.dev/projects/<name>  # clone any project, read-only
```

This repo holds the app only. Servers, DNS, NixOS, Traefik and deploys live in
[workingdir/infrastructure](https://github.com/workingdir/infrastructure), which
pins a specific commit of this repo and builds it. A green push to `main` here
dispatches a staging promotion there; production is a separate merge. See
[Deployment](#deployment).

## How it works

A background task lists the public repos under
[github.com/workingdir](https://github.com/workingdir) and mirrors each one with
`git clone --mirror` into `repos/` (override with `CWD_REPOS`). The shell builds
`projects/<name>` from each repo's file tree, read from the GitHub API, and pulls
file contents from the mirror the first time you `cat` one. `git-upload-pack` is
bridged to the system `git` so clones work; pushes are refused.

The pieces:

- `vfs.rs`: the read-only tree. `projects/` comes from the mirrors; the rest is static.
- `shell.rs`: the command logic (`ls`, `cd`, `cat`, `tree`, `open`, `clone`, ...). No I/O, unit-tested.
- `http.rs` / `site.rs`: a small HTTP/1.1 server. It content-negotiates, so browsers get the site and curl gets the intro as text. The page is `templates/index.html` (askama), prose lives in `content/index.md`, and the fonts and background script in `assets/` are embedded in the binary.
- `ssh.rs`: the russh server. Anonymous auth and a readline-style line editor (history, cursor keys, Ctrl-A/E/U/W/L/C/D). It also runs one-shots like `ssh cwd.dev "ls projects"`.

## Run

```bash
cargo run -- web                   # website only, no SSH deps
cargo run -- local                 # the shell over your terminal, for dev
cargo run --features ssh -- serve  # website + SSH together
cargo test --features ssh          # shell engine tests
```

Against a running `serve`:

```bash
ssh -p 4242 guest@127.0.0.1            # interactive
ssh -p 4242 guest@127.0.0.1 "tree"     # one-shot
curl 127.0.0.1:4280                    # text edition
```

## Configuration

Env vars only, so one binary runs as both production and staging:

| Var | Default | Meaning |
| --- | --- | --- |
| `CWD_HTTP` | `0.0.0.0:4280` | HTTP bind address (behind Traefik in prod) |
| `CWD_SSH` | `0.0.0.0:4242` | SSH bind address (`:22` prod, `:2222` staging) |
| `CWD_ENV` | `production` | environment name; non-prod shows a `[staging]` tag |
| `CWD_HOSTKEY` | `cwd_host_ed25519` | ed25519 host key path |
| `CWD_REPOS` | `repos` | where the repo mirrors are kept |
| `CWD_ORG` | `workingdir` | GitHub org to mirror |
| `CWD_GITHUB_TOKEN` | (unset) | optional; raises the GitHub rate limit |
| `CWD_SYNC_INTERVAL` | `900` | seconds between syncs |

Modes: `cwd web` (HTTP only), `cwd serve` (HTTP + SSH, needs `--features ssh`),
`cwd local` (shell over stdio, for dev).

## Docker

The whole site is in the binary, so the image is the binary plus `git`:

```bash
docker build -t cwd .
docker run --rm -p 8080:8080 -p 2222:2222 -v cwd-data:/data cwd
```

The `/data` volume holds the host key and the mirrors. Override `CWD_HTTP`,
`CWD_SSH`, `CWD_ENV` with `-e`.

## Deployment

CI runs fmt, clippy, test, a release build and a Docker build on every PR and
push. A green push to `main` dispatches `promote-www-to-staging.yml` in
`workingdir/infrastructure` with the exact SHA, which deploys staging. Production
is a merge from `infrastructure:staging` to `infrastructure:production`. The
infrastructure README has the full flow.

One secret is needed here: `INFRASTRUCTURE_DEPLOY_TOKEN`, a fine-grained PAT with
Actions: write on `workingdir/infrastructure`.

## Notes

- Read-only and anonymous. Any SSH auth is accepted and there is no write path.
  It is a public box, so keep it that way.
- The host key is generated on first run and saved to `CWD_HOSTKEY`, so clients
  do not get a changed-host-key warning across restarts. It is gitignored.
- Binding `:80`, `:443` or `:22` needs `CAP_NET_BIND_SERVICE` or a proxy in front.

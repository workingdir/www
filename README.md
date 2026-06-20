# www · cwd.dev

> **Repo split.** This repo (`workingdir/www`) owns the app source only.
> Production and staging state, servers, DNS, NixOS, Traefik and deploys live in
> [`workingdir/infrastructure`](https://github.com/workingdir/infrastructure),
> which pins an exact commit of this repo as a Nix flake input. `main` here does
> not deploy production. A green push dispatches a *staging* promotion in the
> infrastructure repo (see [Deployment](#deployment)).

One Rust binary that is the whole of **cwd.dev**. It serves the website over HTTP
and an interactive, read-only faux shell over SSH, both reading the same virtual
filesystem.

```
ssh cwd.dev                            # a read-only shell of all the projects
curl cwd.dev                           # the same thing, as text, in your terminal
git clone ssh://cwd.dev/projects/helio # clone any project, read-only
open https://cwd.dev                   # the designed website
```

Each project under `projects/` is materialised as a real git repo on startup
(`repos/`, override `CWD_REPOS`), and `git-upload-pack` is bridged to the system
`git`. Pushes (`receive-pack`) are refused; it is a mirror. Needs `git` on the host.

## How it works

```
            ┌──────────────── cwd (one binary) ────────────────┐
  :80/:443  │  http.rs   browser  -> HTML site                 │
            │            curl/wget -> plain-text shell intro    │
   :22      │  ssh.rs    interactive faux shell (russh)         │
            │              │                                    │
            │              └── shell.rs ── vfs.rs (read-only) ──┤
            └──────────────────────────────────────────────────┘
```

- **`vfs.rs`**: a read-only tree. Seeded from a manifest today; in production it
  gets rebuilt from the `github.com/workingdir` repos (default branch) on a timer.
  The shell never knows the difference.
- **`shell.rs`**: command logic over the VFS (`ls cd pwd cat tree open ...`),
  shared by SSH and the local demo. Unit-tested, no I/O.
- **`http.rs`**: minimal HTTP/1.1 (std only). It content-negotiates, so browsers
  get the embedded site and terminal clients get the shell intro.
- **`ssh.rs`**: russh server, anonymous auth, and a readline-style line editor
  (history with the arrow keys, cursor movement, Ctrl-A/E/U/W/L/C/D). It also
  handles one-shot `ssh cwd.dev "ls projects"`.

## Run

```bash
# website only (no SSH deps, instant build)
cargo run -- web                      # http://0.0.0.0:4280

# the faux shell over your own terminal (for development)
cargo run -- local

# the real thing: website + SSH together
cargo run --features ssh -- serve     # CWD_HTTP / CWD_SSH override the addresses
```

Then:

```bash
ssh -p 4242 guest@127.0.0.1                 # interactive
ssh -p 4242 guest@127.0.0.1 "tree"          # one-shot
curl 127.0.0.1:4280                         # terminal edition
```

```bash
cargo test --features ssh                   # shell engine tests
```

## Configuration

Env vars only, no config files, so the same binary runs as both production and
staging on one host:

| Var           | Default            | Meaning                                            |
| ------------- | ------------------ | -------------------------------------------------- |
| `CWD_HTTP`    | `0.0.0.0:4280`     | HTTP bind address (sits behind Traefik in prod)    |
| `CWD_SSH`     | `0.0.0.0:4242`     | SSH bind address (`:22` prod, `:2222` staging)     |
| `CWD_ENV`     | `production`       | environment name; non-prod shows a `[staging]` tag |
| `CWD_HOSTKEY` | `cwd_host_ed25519` | persistent ed25519 host key path                   |
| `CWD_REPOS`   | `repos`            | where per-project git repos are materialised       |

Modes: `cwd web` (HTTP only), `cwd serve` (HTTP + SSH, needs `--features ssh`),
`cwd local` (faux shell over stdio, for dev).

## Nix

The repo is a flake. The infrastructure repo builds the binary from a pinned
revision of this one:

```bash
nix build            # -> ./result/bin/cwd (built with the ssh feature, git on PATH)
nix flake check      # builds the binary as a check
nix develop          # cargo/rustc/clippy/git dev shell
```

`buildRustPackage` reads `cargoLock.lockFile`, so builds are reproducible without
a vendored hash. `git` is wrapped onto the binary's `PATH` so the git-over-SSH
bridge works whatever the service environment looks like.

## Deployment

CI (`.github/workflows/ci.yml`) runs fmt, clippy, test and `cargo build --release
--features ssh` on every PR and push. On a green push to `main` it dispatches
`promote-www-to-staging.yml` in `workingdir/infrastructure` with this exact SHA.
That promotion lands on `infrastructure:staging` and deploys staging. Production
is a deliberate merge from `infrastructure:staging` to `infrastructure:production`.
The infrastructure README has the full flow and rollback steps.

One GitHub secret is needed here: `INFRASTRUCTURE_DEPLOY_TOKEN`, a fine-grained
PAT (or GitHub App token) with Actions: write on `workingdir/infrastructure` and
nothing else.

## Notes for production

- **Read-only and anonymous by design.** There is no write path and any auth is
  accepted (it is a public box). Keep it that way; never mount real disk.
- **Host key.** A persistent ed25519 key is generated on first run and saved to
  `cwd_host_ed25519` (override with `CWD_HOSTKEY`), so clients do not see
  "REMOTE HOST IDENTIFICATION HAS CHANGED" across restarts. Keep it safe and out
  of git (it is `.gitignore`d).
- **Ports.** Binding `:80`/`:443`/`:22` needs `CAP_NET_BIND_SERVICE` or a proxy.
  In production Traefik terminates TLS and the binary owns `:22` directly.
- **GitHub sync.** `vfs::sync()` is still a TODO: clone/pull the `workingdir`
  repos into the tree on an interval, swapped behind an `ArcSwap` so sessions read
  a consistent snapshot.
- **Hardening.** Connection and rate limits, per-session timeouts, output caps.

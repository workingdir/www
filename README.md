# www

The source for [cwd.dev](https://cwd.dev). One Rust binary that serves the
website over HTTP and a read-only, anonymous shell over SSH, both backed by the
same tree of my public repos.

```
ssh cwd.dev                              # browse the projects in a shell
curl cwd.dev                             # the same intro, as text
git clone ssh://cwd.dev/projects/<name>  # clone any project, read-only
```

## How it works

A background task mirrors the public repos under
[github.com/workingdir](https://github.com/workingdir) into `repos/`. The shell
builds `projects/<name>` from each repo's file tree (via the GitHub API) and
reads file contents from the mirror when you `cat` them; `git-upload-pack` is
bridged to the system `git` for clones, and pushes are refused. The website is
`templates/index.html` (askama) with prose in `content/index.md`; fonts and the
background script are embedded in the binary. SSH is an opt-in feature, so the
default build stays lean.

## Run

```bash
cargo run -- web                   # website only, no SSH deps
cargo run -- local                 # the shell over your terminal, for dev
cargo run --features ssh -- serve  # website + SSH together
cargo test --features ssh          # tests
```

## Configuration

Env vars only:

| Var | Default | Meaning |
| --- | --- | --- |
| `CWD_HTTP` | `0.0.0.0:4280` | HTTP bind address |
| `CWD_SSH` | `0.0.0.0:4242` | SSH bind address |
| `CWD_ENV` | `production` | environment name; non-prod shows a `[staging]` tag |
| `CWD_HOSTKEY` | `cwd_host_ed25519` | ed25519 host key (made on first run, gitignored) |
| `CWD_REPOS` | `repos` | where the repo mirrors are kept |
| `CWD_ORG` | `workingdir` | GitHub org to mirror |
| `CWD_GITHUB_TOKEN` | (unset) | optional; raises the GitHub rate limit |
| `CWD_SYNC_INTERVAL` | `900` | seconds between syncs |

## Docker

```bash
docker build -t cwd .
docker run --rm -p 8080:8080 -p 2222:2222 -v cwd-data:/data cwd
```

The `/data` volume holds the host key and the mirrors.

# Build the cwd binary with the ssh feature, then ship it on a slim runtime with
# git (the git-over-ssh bridge shells out to it). The website, fonts, and scripts
# are embedded in the binary, so nothing else needs copying.

FROM rust:1-bookworm AS build
WORKDIR /app
COPY . .
RUN cargo build --release --features ssh

FROM debian:bookworm-slim
# git for the git-over-ssh bridge and repo mirroring; curl + jq to list the org's
# repos from the GitHub API.
RUN apt-get update \
  && apt-get install -y --no-install-recommends git ca-certificates curl jq \
  && rm -rf /var/lib/apt/lists/*
COPY --from=build /app/target/release/cwd /usr/local/bin/cwd

# Override at run time. State (host key, materialised repos) lives under /data.
ENV CWD_HTTP=0.0.0.0:8080 \
    CWD_SSH=0.0.0.0:2222 \
    CWD_HOSTKEY=/data/host_ed25519 \
    CWD_REPOS=/data/repos
VOLUME ["/data"]
WORKDIR /data
EXPOSE 8080 2222

ENTRYPOINT ["cwd"]
CMD ["serve"]

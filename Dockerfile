# astria — turn any folder into a queryable knowledge graph.
#
# Multi-stage build: a Rust+Node builder compiles the napi cdylib and the
# TypeScript CLI, then a slim Node runtime ships the `astria` binary with
# nothing else — no compiler, no build cache, no dev dependencies.
#
#   Build:   docker build -t astria .
#   Analyze: docker run --rm -v "$PWD":/workspace astria run /workspace
#   Query:   docker run --rm -v "$PWD":/workspace astria query "auth flow" --graph /workspace
#   Serve:   docker run --rm -p 8620:8620 -v "$PWD":/workspace \
#              astria mcp --http --host 0.0.0.0 --token "$ASTRIA_MCP_TOKEN" --graph /workspace
#
# The HTTP MCP server refuses to bind 0.0.0.0 without a token — that is the
# container default here, so --token is required for the serve pattern.

# syntax=docker/dockerfile:1

ARG RUST_VERSION=1.88
ARG NODE_MAJOR=22

FROM rust:${RUST_VERSION}-slim-bookworm AS builder
ARG NODE_MAJOR
# Node toolchain for the napi artifact copy and the CLI build; ca-certificates
# for the NodeSource repo and any ort-sys runtime download.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl gnupg \
 && curl -fsSL https://deb.nodesource.com/setup_${NODE_MAJOR}.x | bash - \
 && apt-get install -y --no-install-recommends nodejs \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY packages packages
COPY scripts scripts

# The native module: same commands CI runs (cargo build → copy cdylib to
# dist/astria.node). Embeddings (fastembed/ONNX) are on by default; build
# with `--build-arg NAPI_FEATURES=--no-default-features` for a smaller image
# that answers everything except `--embed`/semantic recall.
ARG NAPI_FEATURES=""
RUN cargo build --release --locked -p astria-napi ${NAPI_FEATURES} \
 && mkdir -p packages/astria-cli/dist \
 && cp target/release/libastria_napi.so packages/astria-cli/dist/astria.node

# The TypeScript CLI on top of the fresh binary.
RUN npm ci --include=dev \
 && npm run build --workspace packages/astria-cli

FROM node:${NODE_MAJOR}-bookworm-slim AS runtime

WORKDIR /app
COPY --from=builder /build/packages/astria-cli/package.json ./package.json
COPY --from=builder /build/packages/astria-cli/dist ./dist
COPY --from=builder /build/packages/astria-cli/skills ./skills
COPY --from=builder /build/packages/astria-cli/README.md ./README.md
COPY --from=builder /build/packages/astria-cli/LICENSE ./LICENSE
RUN npm install --omit=dev --omit=optional --no-audit --no-fund \
 && npm install --global --no-audit --no-fund .

ENV NODE_ENV=production
# MCP HTTP transport port (astria mcp --http).
EXPOSE 8620

# Graphs are written where the user mounts the repo; default to a volume
# friendlier layout than scattering state.
ENTRYPOINT ["astria"]
CMD ["--help"]

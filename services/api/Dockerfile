# Bonded API — production image.
# Build:  docker build -f services/api/Dockerfile -t bonded-api .
# Run:    docker run --rm -p 8787:8787 --env-file .env bonded-api
#
# Note: the evaluator prefers Docker for test isolation. This image runs
# tests on the host instead (Docker-in-Docker is out of scope); presets use
# Node's built-in test runner, so host execution is acceptable for the demo.
FROM rust:1.95-bookworm AS build
WORKDIR /src
COPY services/api ./services/api
RUN cd services/api && cargo build --release

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates git curl \
 && apt-get clean && rm -rf /var/lib/apt/lists/*
# Node.js runtime for the evaluator's host-mode test execution.
COPY --from=node:22-bookworm-slim /usr/local/bin/node /usr/local/bin/node
COPY --from=node:22-bookworm-slim /usr/local/lib/node_modules /usr/local/lib/node_modules
RUN ln -s /usr/local/lib/node_modules/npm/bin/npm-cli.js /usr/local/bin/npm \
 && ln -s /usr/local/lib/node_modules/npm/bin/npx-cli.js /usr/local/bin/npx
WORKDIR /app
COPY --from=build /src/services/api/target/release/bonded-api /app/bonded-api
COPY demo /app/demo
ENV DEMO_DIR=/app/demo
ENV PORT=8787
EXPOSE 8787
CMD ["/app/bonded-api"]

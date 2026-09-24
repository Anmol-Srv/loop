# MCP docs are compiled into the binary, so the runtime image carries no source.
# The GUI binary is not built here: it is gated behind the `app` feature, which
# `--bins` skips, so a container never pulls in the egui stack.
#
# Both stages are Debian trixie. The runtime stage was bookworm, whose glibc
# (2.36) is older than the one the build stage links against (2.41): the image
# built fine and then every binary in it refused to start.
FROM rust:1.96-slim-trixie AS build
WORKDIR /src
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock build.rs ./
COPY src ./src
COPY migrations ./migrations
# The agent skill and onboarding docs are compiled in, served at /api/agent/*.
COPY agent-kit ./agent-kit
RUN cargo build --release --bins

FROM debian:trixie-slim
# curl is here for the health check and nothing else.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 acp
COPY --from=build /src/target/release/acp-server /usr/local/bin/
COPY --from=build /src/target/release/acp-admin  /usr/local/bin/
COPY --from=build /src/target/release/acp        /usr/local/bin/
COPY --from=build /src/target/release/acp-mcp    /usr/local/bin/

# Ready means the database answers, not just that a binary exists: the old
# check ran `acp --version`, which passed with the server down.
HEALTHCHECK --interval=15s --timeout=3s --start-period=20s --retries=3 \
  CMD curl -fsS "http://127.0.0.1:${PORT}/health/ready" >/dev/null || exit 1
USER acp
EXPOSE 8080
ENV PORT=8080 HOST=0.0.0.0
CMD ["acp-server"]

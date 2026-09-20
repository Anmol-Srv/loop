# MCP docs are compiled into the binary, so the runtime image carries no source.
# The GUI binary is not built here: it is gated behind the `app` feature, which
# `--bins` skips, so a container never pulls in the egui stack.
FROM rust:1.96-slim AS build
WORKDIR /src
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
RUN cargo build --release --bins

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 acp
COPY --from=build /src/target/release/acp-server /usr/local/bin/
COPY --from=build /src/target/release/acp-admin  /usr/local/bin/
COPY --from=build /src/target/release/acp        /usr/local/bin/
COPY --from=build /src/target/release/acp-mcp    /usr/local/bin/

HEALTHCHECK --interval=30s --timeout=3s --start-period=10s \
  CMD ["/usr/local/bin/acp", "--version"]
USER acp
EXPOSE 8080
ENV PORT=8080
CMD ["acp-server"]

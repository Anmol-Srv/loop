# Templates, MCP docs, and static assets are compiled into the binary, so the
# runtime image carries no source and no asset directory.
FROM rust:1.96-slim AS build
WORKDIR /src
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
COPY templates ./templates
COPY static ./static
RUN cargo build --release --bins

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 acp
COPY --from=build /src/target/release/acp-server /usr/local/bin/
COPY --from=build /src/target/release/acp-admin  /usr/local/bin/
COPY --from=build /src/target/release/acp        /usr/local/bin/
COPY --from=build /src/target/release/acp-mcp    /usr/local/bin/
USER acp
EXPOSE 8080
ENV PORT=8080
CMD ["acp-server"]

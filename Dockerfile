FROM rust:1-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends protobuf-compiler && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
COPY web ./web
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 gateway
COPY --from=build /src/target/release/payment-gateway /usr/local/bin/payment-gateway
USER gateway
EXPOSE 8080
ENV RUST_LOG=info
ENTRYPOINT ["/usr/local/bin/payment-gateway"]

FROM docker.io/library/rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --bin eqoxide-assets

FROM docker.io/library/debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/eqoxide-assets /usr/local/bin/eqoxide-assets
VOLUME ["/data"]
EXPOSE 8088
ENTRYPOINT ["/usr/local/bin/eqoxide-assets"]
CMD ["serve", "--data", "/data", "--addr", "0.0.0.0:8088", "--secret-file", "/data/secret"]


FROM rust:slim-bullseye as builder

WORKDIR /build
COPY . .
RUN cargo build --release

FROM debian:bullseye-slim AS runtime

WORKDIR /broker
COPY --from=builder /build/target/release/broker /usr/local/bin

ENTRYPOINT ["/usr/local/bin/broker"]

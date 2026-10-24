# syntax=docker/dockerfile:1
# arena-cloud: built from the repository root.
#   docker build -f deploy/docker/arena-cloud.Dockerfile -t arena-cloud .
#
# Build stage: the Rust toolchain pinned by rust-toolchain.toml (the image tag matches its
# channel; `pnpm check:deploy` enforces it). Runtime: distroless/cc (glibc + libgcc + CA
# certificates, no shell), as the image's non-root user.

FROM rust:1.95.0-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --locked --release -p arena-cloud \
 && cp target/release/arena-cloud /arena-cloud

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /arena-cloud /usr/local/bin/arena-cloud
USER 65532:65532
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/arena-cloud"]
CMD ["--role", "control"]

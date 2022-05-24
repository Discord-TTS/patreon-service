FROM lukemathwalker/cargo-chef:latest-rust-latest AS chef

WORKDIR /build

# Container to generate a recipe.json
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Container to build the bot
FROM chef AS builder

# This is a dummy build to get the dependencies cached.
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --release

# This is the actual build, copy in the rest of the sources
COPY . .
RUN cargo build --release

# Now make the runtime container
FROM debian:bullseye-slim

COPY --from=builder /build/target/release/patreon-service /usr/local/bin/patreon-service

CMD ["/usr/local/bin/patreon-service"]

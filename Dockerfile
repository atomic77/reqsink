# -*- mode: dockerfile -*-
# Borrowed from 
# https://github.com/emk/rust-musl-builder/blob/master/examples/using-diesel/Dockerfile

# TODO Add base templates and think about how we'll allow for user-defined templates
ARG BASE_IMAGE=ekidd/rust-musl-builder:stable

# Our first FROM statement declares the build environment.
FROM ${BASE_IMAGE} AS builder

# Add our source code.
# TODO Don't send whole target context
ADD --chown=rust:rust . ./

# Build our application.
RUN cargo build --release

# Now, we need to build our _real_ Docker container, copying in `using-diesel`.
FROM alpine:latest
RUN apk --no-cache add ca-certificates
COPY --from=builder \
    /home/rust/src/target/x86_64-unknown-linux-musl/release/reqsinkrs \
    /usr/local/bin/
CMD /usr/local/bin/reqsinkrs

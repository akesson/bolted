# The step-20 Linux environment. One image, two tiers:
#   test:os:linux   — runs cargo directly in the container (rows P1, D1-D4);
#   test:os:systemd — boots /lib/systemd/systemd as PID 1 (--privileged) and drives the
#                     lifecycle rows L1-L5 inside.
# The Rust version matches the mise pin (mise.toml [tools] rust); bump them together.
FROM rust:1.95.0-bookworm
RUN apt-get update \
    && apt-get install -y --no-install-recommends systemd \
    && rm -rf /var/lib/apt/lists/*

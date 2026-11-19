# Cloudflare Container image for the headless lane. Remote Control is not
# available here (setup-token / API keys cannot serve it); runs are `claude -p`.
FROM rust:1-bookworm AS guard
COPY . /src
RUN cargo install --locked --root /out --path /src/crates/playpilot

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends git ca-certificates curl gh \
 && rm -rf /var/lib/apt/lists/* \
 && useradd -m -u 1000 agent
COPY --from=guard /out/bin/playpilot guard /usr/local/bin/playpilot guard
COPY profiles/node.settings.json /home/agent/.config/fleet/node.settings.json
COPY runtimes/cf-container/entrypoint.sh /usr/local/bin/fleet-entrypoint
USER agent
WORKDIR /home/agent
RUN curl -fsSL https://claude.ai/install.sh | bash
ENV PATH=/home/agent/.local/bin:$PATH PLAYPILOT_RUNTIME=cf-container
ENTRYPOINT ["fleet-entrypoint"]

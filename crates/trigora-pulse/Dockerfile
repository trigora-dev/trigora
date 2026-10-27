FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*
COPY trigora-pulse /usr/local/bin/trigora-pulse
ENTRYPOINT ["trigora-pulse", "/pulse"]

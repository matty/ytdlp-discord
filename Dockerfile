FROM rust:1.86 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y curl \
    && curl -L https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux -o /usr/local/bin/yt-dlp \
    && chmod +x /usr/local/bin/yt-dlp \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/ytdlp-discord /usr/local/bin/ytdlp-discord
COPY config.toml ./
RUN mkdir -p /app/output
CMD ["/usr/local/bin/ytdlp-discord"]

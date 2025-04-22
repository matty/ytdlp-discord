FROM rust:1.86 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y yt-dlp && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/ytdlp-discord /usr/local/bin/ytdlp-discord
COPY config.toml ./
CMD ["/usr/local/bin/ytdlp-discord"]

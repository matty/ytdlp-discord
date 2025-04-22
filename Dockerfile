FROM rust:1.86 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y yt-dlp && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/ytdlp-output-rs /usr/local/bin/ytdlp-output-rs
COPY config.toml ./

RUN useradd -m appuser && chown appuser:appuser /usr/local/bin/videostream-rs /app/config.toml
USER appuser

CMD ["/usr/local/bin/ytdlp-output-rs"]

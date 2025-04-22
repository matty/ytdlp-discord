use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::prelude::*;
use regex::Regex;
use std::process::Stdio;
use std::fs;
use std::env;
use anyhow::{Result, Context as AnyhowContext};
use serde::Deserialize;
use config::Config;
use log::{info, error};
use config::Environment;

#[derive(Debug, Deserialize)]
struct Settings {
    discord_token: String,
    output_dir: String,
    guild_ids: Option<Vec<u64>>,
    channel_id: Option<u64>,
    cookies_path: Option<String>,
}

impl Settings {
    fn from_env_and_file() -> Result<Self> {
        let mut s = Config::builder();
        s = s.add_source(config::File::with_name("config").required(false));
        s = s.add_source(Environment::default());
        let mut settings: Settings = s.build()?.try_deserialize()?;
        // Give priority to DISCORD_TOKEN env var
        if let Ok(token) = env::var("DISCORD_TOKEN") {
            settings.discord_token = token;
        }
        if let Ok(guilds) = env::var("GUILD_IDS") {
            // Try to parse as JSON array first
            let guilds = guilds.trim();
            if let Ok(ids) = serde_json::from_str::<Vec<u64>>(guilds) {
                settings.guild_ids = Some(ids);
            } else {
                // If JSON parsing fails, try to parse as a single ID
                if let Ok(id) = guilds.parse::<u64>() {
                    settings.guild_ids = Some(vec![id]);
                } else {
                    // If both parsing attempts fail, return an error with guidance
                    return Err(anyhow::anyhow!("GUILD_IDS must be either a single numeric ID or a JSON array, e.g. [123456789,987654321]"));
                }
            }
        }
        if let Ok(channel) = env::var("CHANNEL_ID") {
            if let Ok(id) = channel.parse() {
                settings.channel_id = Some(id);
            }
        }
        // Set cookies_path from env if present, otherwise default to config/cookies.txt if exists
        if let Ok(cookies_path) = env::var("YTDLP_COOKIES_PATH") {
            settings.cookies_path = Some(cookies_path);
        } else if settings.cookies_path.is_none() {
            let default_path = "config/cookies.txt";
            if std::path::Path::new(default_path).exists() {
                settings.cookies_path = Some(default_path.to_string());
            }
        }
        Ok(settings)
    }
}

struct Handler {
    url_regex: Regex,
    output_dir: String,
    allowed_guilds: Option<Vec<u64>>,
    allowed_channel: Option<u64>,
    cookies_path: Option<String>,
}

impl Handler {
    fn is_allowed_guild(&self, guild_id: serenity::model::id::GuildId) -> bool {
        match &self.allowed_guilds {
            Some(ids) => ids.contains(&guild_id.get()),
            None => true,
        }
    }
}

fn is_valid_url(url: &str) -> bool {
    // Basic URL validation: must start with http:// or https:// and have at least one dot
    let re = Regex::new(r"^https?://[\w\-\.]+\.[a-zA-Z]{2,}(/\S*)?$" ).unwrap();
    re.is_match(url)
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: Message) {
        if msg.author.bot {
            return;
        }
        if let Some(guild_id) = msg.guild_id {
            if !self.is_allowed_guild(guild_id) {
                return;
            }
        }
        if let Some(allowed_channel) = self.allowed_channel {
            if msg.channel_id.get() != allowed_channel {
                return;
            }
        }
        if let Some(url_match) = self.url_regex.find(&msg.content) {
            if !is_valid_url(url_match.as_str()) {
                let _ = msg.channel_id.say(&ctx.http, "Invalid URL.").await;
                return;
            }
            if let Err(e) = msg.channel_id.say(&ctx.http, "OK! I will process that.").await {
                log::error!("Failed to send acknowledgment: {}", e);
            }
            let url = url_match.as_str().to_owned();
            let output_dir = self.output_dir.clone();
            let msg_channel = msg.channel_id;
            let ctx_clone = ctx.clone();
            let cookies_path = self.cookies_path.clone();
            tokio::spawn(async move {
                match download_url_with_cookies(
                    &url,
                    &output_dir,
                    cookies_path.as_deref(),
                ).await {
                    Ok(_) => {
                        let _ = msg_channel.say(&ctx_clone.http, format!("Downloaded: <{}>", url)).await;
                    }
                    Err(e) => {
                        let _ = msg_channel.say(&ctx_clone.http, format!("Failed to download {}: {}", url, e)).await;
                    }
                }
            });
        } else {
            let _ = msg.channel_id.say(&ctx.http, "Invalid URL.").await;
        }
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("Connected as {}", ready.user.name);
        if let Some(ref allowed_guilds) = self.allowed_guilds {
            for guild in ready.guilds {
                if !allowed_guilds.contains(&guild.id.get()) {
                    info!("Leaving unauthorized guild: {}", guild.id);
                    if let Err(e) = guild.id.leave(&ctx.http).await {
                        error!("Failed to leave guild {}: {}", guild.id, e);
                    }
                }
            }
        }
    }
}

async fn download_url_with_cookies(
    url: &str,
    output_dir: &str,
    cookies_path: Option<&str>,
) -> Result<()> {
    log::info!("Downloading URL: {}", url);
    fs::create_dir_all(output_dir)
        .with_context(|| format!("Failed to create output directory: {}", output_dir))?;
    let mut cmd = tokio::process::Command::new("yt-dlp");
    cmd.arg(url)
        .arg("-P").arg(output_dir);
    if let Some(cookies) = cookies_path {
        log::info!("Using cookies file: {}", cookies);
        cmd.arg("--cookies").arg(cookies);
    }
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::piped());
    let child = cmd.spawn()
        .with_context(|| "Failed to spawn yt-dlp process")?;
    let output = child.wait_with_output().await
        .with_context(|| "Failed to wait for yt-dlp process")?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(anyhow::anyhow!("yt-dlp failed with status: {}\nError output: {}", output.status, stderr.trim()))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Configure logger with default info level if not set
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "serenity=warn,ytdlp_output_rs=info");
    }
    env_logger::init();
    let settings = Settings::from_env_and_file()
        .context("Failed to load configuration from file or environment")?;
    let url_regex = Regex::new(r"https?://\S+")
        .context("Failed to compile URL regex")?;
    let handler = Handler {
        url_regex,
        output_dir: settings.output_dir.clone(),
        allowed_guilds: settings.guild_ids.clone(),
        allowed_channel: settings.channel_id,
        cookies_path: settings.cookies_path.clone(),
    };
    let mut client = Client::builder(&settings.discord_token, GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT)
        .event_handler(handler)
        .await
        .context("Failed to create Discord client")?;
    client.start().await.context("Discord client exited with error")?;
    Ok(())
}

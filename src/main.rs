#![allow(non_snake_case)]
#![feature(drain_filter)]
#![feature(try_blocks)]

mod messages;
mod mysql;
mod web;

use anyhow::{Context, Result};
use env_logger::Env;
use twilight_gateway::{Intents, Shard};

use std::env;
use std::fs::File;
use std::io::BufReader;

mod utils {
    pub mod config;
}

use actix_rt::signal::ctrl_c;

use crate::utils::config::Config;

#[actix_rt::main]
async fn main() -> Result<()> {
    // Default log level is debug
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let path = env::args().nth(1).unwrap_or("config.yaml".to_string());

    let file = File::open(&path).context(format!("Can't read config file {}", &path))?;
    let reader = BufReader::new(file);

    let config: Config = serde_yaml::from_reader(reader).context("Config file read failure")?;

    let intents = Intents::GUILDS | Intents::GUILD_MESSAGES | Intents::GUILD_MEMBERS | Intents::GUILD_VOICE_STATES;

    let mut shard = Shard::new(&config.discord_token, intents);
    shard.start().await?;

    if config.web_enabled {
        web::spawn(config.clone()).await?;
    }

    if config.mysql_enabled {
        mysql::spawn(&shard, config.clone()).await?;
    }

    if config.messages_enabled {
        messages::spawn(&shard, config.clone()).await?;
    }

    ctrl_c().await?;

    Ok(())
}

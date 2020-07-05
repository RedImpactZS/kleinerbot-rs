#![allow(non_snake_case)]
#![feature(drain_filter)]
#![feature(try_blocks)]

mod messages;
mod mysql;
mod web;

use anyhow::{Context, Result};
use env_logger::Env;
use twilight::{gateway::Shard, http::Client as TwilightHttp};

use std::env;
use std::fs::File;
use std::io::BufReader;

mod utils {
    pub mod config;
}

use crate::utils::config::Config;

#[actix_rt::main]
async fn main() -> Result<()> {
    // Default log level is debug
    env_logger::from_env(Env::default().default_filter_or("debug")).init();

    let path = env::args().nth(1).unwrap_or("config.yaml".to_string());

    let file = File::open(&path).context(format!("Can't read config file {}", &path))?;
    let reader = BufReader::new(file);

    let config: Config = serde_yaml::from_reader(reader).context("Config file read failure")?;

    let mut shard = Shard::new(&config.discord_token);
    shard.start().await?;

    let http = TwilightHttp::new(&config.discord_token);

    if config.web_enabled {
        web::spawn(&http, config.clone()).await?;
    }

    if config.mysql_enabled {
        mysql::spawn(&shard, config.clone()).await?;
    }

    messages::event_hanler(&shard, &http).await?;

    Ok(())
}

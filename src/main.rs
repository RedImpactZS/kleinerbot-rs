#![allow(non_snake_case)]
#![feature(drain_filter)]
#![feature(try_blocks)]

mod mysql;
mod web;
mod messages;

use twilight::{
    http::Client as TwilightHttp,
    gateway::Shard
    };
use anyhow::Result;
use env_logger::Env;

mod utils {
    pub mod config;
}

use crate::utils::config::Config;

#[actix_rt::main]
async fn main() -> Result<()> {
    // Default log level is debug
    env_logger::from_env(Env::default().default_filter_or("debug")).init();

    let config = envy::from_env::<Config>()?;

    let mut shard = Shard::new(&config.discord_token);
    shard.start().await?;

    mysql::spawn(&shard,config.clone()).await?;

    let http = TwilightHttp::new(&config.discord_token);

    web::spawn(&http,config.clone()).await?;

    messages::event_hanler(&shard,&http).await?;

    Ok(())
}
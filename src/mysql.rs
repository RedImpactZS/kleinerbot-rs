use anyhow::Result;
use core::time::Duration;
use log::{info, warn};
use twilight_gateway::Shard;
use twilight_model::gateway::{payload::UpdateStatus,presence::{Activity, ActivityType, Status}};

use sqlx::{Row, mysql::{MySqlConnectOptions, MySqlPool}};

use actix_rt::Arbiter;

use crate::utils::config::Config;

const IDNAMES: [&str; 3] = ["UNK", "ZS", "TTT"];
//i am too lazy to create ID names in db

const ACTIVITY_TEMPLATE: Activity = Activity {
    application_id: None,
    assets: None,
    created_at: None,
    details: None,
    flags: None,
    id: None,
    instance: None,
    kind: ActivityType::Playing,
    name: String::new(),
    emoji: None,
    party: None,
    secrets: None,
    state: None,
    timestamps: None,
    url: None,
};

struct ServerData {
    id: i32,
    players: i32,
    slots: i32,
    map: String,
}

async fn task(shard: &Shard, opts: &MySqlConnectOptions) -> Result<()> {
    let mut lastid: usize = 0;

    let fifteen_secs = Duration::new(15, 0);

    loop {
        let pool = MySqlPool::connect_with(opts.clone()).await?;

        let query = sqlx::query("SELECT id,players,slots,map FROM `gex_servers` WHERE id < 100 ORDER BY id")
        .fetch_all(&pool)
        .await?;

        let mut sdata = vec!();
        for data in query.into_iter() {
            sdata.push(ServerData {
                id: data.try_get("id")?,
                players: data.try_get("players")?,
                slots: data.try_get("slots")?,
                map: data.try_get("map")?,
            });
        }

        let data: &ServerData = &sdata[lastid];
        let mut mapname = data.map.clone();
        mapname.truncate(14);

        let mut activity = ACTIVITY_TEMPLATE.clone();

        activity.name = format!(
            "{}|{}|{}/{}",
            IDNAMES[data.id as usize], mapname, data.players, data.slots
        )
        .to_owned();

        shard
            .command(&UpdateStatus::new(vec!(activity), false, None, Status::Online))
            .await?;

        lastid += 1;
        if lastid == sdata.len() {
            lastid = 0
        }

        actix_rt::time::delay_for(fifteen_secs).await;
    }
}

pub async fn spawn(shard: &Shard, config: Config) -> Result<()> {

    let opts = MySqlConnectOptions::new()
    .host(&config.mysql_hostname)
    .port(config.mysql_port.to_owned())
    .username(&config.mysql_user)
    .password(&config.mysql_password)
    .database(&config.mysql_dbname);

    info!(
        "Connecting to mysql {}:{}",
        &config.mysql_hostname, &config.mysql_port
    );

    let shard1 = shard.clone();

    Arbiter::spawn(async move {
        loop {
            task(&shard1, &opts)
                .await
                .unwrap_or_else(|err| warn!("MySQL task failed: {}", err));
        }
    });

    Ok(())
}

use core::time::Duration;
use log::{info};
use anyhow::Result;
use twilight::{
    model::gateway::{payload::UpdateStatus,presence::{Status,Activity,ActivityType}},
    gateway::Shard
    };

use mysql_async::prelude::*;

use crate::utils::config::Config;

const IDNAMES: [&str;3] = ["UNK","ZS","TTT"];
//i am too lazy to create ID names in db

const ACTIVITY_TEMPLATE:Activity = Activity {
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
    id:i32,
    players:i32,
    slots:i32,
    map:String
}


async fn task(shard: &Shard,opts: mysql_async::OptsBuilder) -> Result<()> {

    let mut lastid:usize = 0;

    let fifteen_secs = Duration::new(15, 0);

    loop {

        let conn = mysql_async::Conn::new(opts.clone()).await?;
        let result = conn.prep_exec("SELECT id,players,slots,map FROM `gex_servers` WHERE id < 100 ORDER BY id", ()).await?;
        let (conn , sdata) = result.map_and_drop(|row| {
            let (id,players,slots,map) = mysql_async::from_row(row);
            ServerData{id,players,slots,map}
        }).await?;

        conn.disconnect().await?;

        let data: &ServerData = &sdata[lastid];
        let mut mapname = data.map.clone();
        mapname.truncate(16);

        let mut activity = ACTIVITY_TEMPLATE.clone();
        activity.name = format!("{}|{}|{}/{}",IDNAMES[data.id as usize],mapname,data.players,data.slots).to_owned();

        shard.command(&UpdateStatus::new(false, activity, None, Status::Online)).await?;

        lastid += 1;
        if lastid == sdata.len() {
            lastid = 0
        }
        
        actix_rt::time::delay_for(fifteen_secs).await;

    }
}

pub async fn spawn(shard: &Shard,config: Config) -> Result<()> {

    let mut opts = mysql_async::OptsBuilder::new();
    opts.ip_or_hostname(&config.mysql_hostname)
        .tcp_port(config.mysql_port.to_owned())
        .user(Some(&config.mysql_user))
        .pass(Some(&config.mysql_password))
        .db_name(Some(&config.mysql_dbname));

    info!("Connecting to mysql {}:{}",&config.mysql_hostname,&config.mysql_port);

    let shard1= shard.clone();

    actix_rt::spawn(async move {
        task(&shard1,opts).await.unwrap()
    });

    Ok(())
}
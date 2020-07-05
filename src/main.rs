#![allow(non_snake_case)]
#![feature(drain_filter)]
#![feature(try_blocks)]

use std::sync::Mutex;
use core::time::Duration;
use futures::stream::StreamExt;
use twilight::{
    cache::{
        twilight_cache_inmemory::config::{InMemoryConfigBuilder, EventType},
        InMemoryCache,
    },
    model::{gateway::{payload::UpdateStatus,presence::{Status,Activity,ActivityType}}, channel::GuildChannel, id::{ChannelId,UserId,MessageId}},
    gateway::{Shard, Event},
    http::Client as HttpClient,
    builders::embed::EmbedBuilder
};
use mysql_async::prelude::*;
use actix_web::{web, App, HttpServer, Responder, client::Client, http::StatusCode};
use openssl::ssl::{SslAcceptor, SslFiletype, SslMethod};
use serde::Deserialize;
use anyhow::{Result,Context,anyhow};
use env_logger::Env;
use log::{info, error, debug, warn};

//They are not rly a secret
const DEFAULT_PORT:u16 = 3444;
const CID_GITHUB:ChannelId = ChannelId(478623542380855306);
const CID_LOG:ChannelId = ChannelId(697846732201000970);
const MAXFILESIZE:usize = 1000000000000000; // Doesn't matter as hard limit is 50 mb 

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

const IDNAMES: [&str;3] = ["UNK","ZS","TTT"];
//i am too lazy to create ID names in db

#[derive(Deserialize,Debug)]
struct Body {
    #[serde(default)]
    activity: String,
    #[serde(default)]
    content: String,
    #[serde(default="Body::default_channel")]
    channelID: u64,
}

impl Body {
    fn default_channel() -> u64 { 544557150064738315 }
}

#[derive(Deserialize, Debug, Clone)]
struct Config {
  #[serde(default="Config::default_hostname")]
  web_hostname: String,
  #[serde(default="Config::default_web_port")]
  web_port: u16,
  botapi_token: String,
  discord_token: String,
  #[serde(default="Config::default_hostname")]
  mysql_hostname: String,
  #[serde(default="Config::default_mysql_port")]
  mysql_port: u16,
  mysql_user: String,
  mysql_password: String,
  mysql_dbname: String,
  web_sslkey: String,
  web_chain: String,
}

impl Config {
    fn default_hostname() -> String { "localhost".to_string() }
    fn default_web_port() -> u16 { DEFAULT_PORT }
    fn default_mysql_port() -> u16 { DEFAULT_PORT }
}

#[derive(Deserialize,Debug)]
struct Info {
    token: String,
    #[serde(default)]
    github: bool,
}

#[derive(Deserialize)]
struct AuthorOrRepo {
    name:String
}

#[derive(Deserialize)]
struct Commit {
    id:String,
    message:String,
    author:AuthorOrRepo
}

#[derive(Deserialize)]
struct Github {
    r#ref: String,
    repository: AuthorOrRepo,
    commits: Vec<Commit>
}

struct BotData {
    shard: Shard,
    http: HttpClient,
    token: String
}

struct ServerData {
    id:i32,
    players:i32,
    slots:i32,
    map:String
}

struct ImagesData {
    id: MessageId,
    images: Vec<Image>
}

struct Image {
    name: String,
    body: bytes::Bytes
}

async fn mysql(shard: &Shard,mysql_hostname: &String,mysql_port: &u16,mysql_user: &String,mysql_pass: &String,mysql_db: &String) -> Result<()> {
    info!("Connecting to mysql {}:{}",mysql_hostname,mysql_port);

    let mut lastid:usize = 0;

    let fifteen_secs = Duration::new(15, 0);

    let mut opts = mysql_async::OptsBuilder::new();
    opts.ip_or_hostname(mysql_hostname)
        .tcp_port(mysql_port.to_owned())
        .user(Some(mysql_user))
        .pass(Some(mysql_pass))
        .db_name(Some(mysql_db));

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

async fn request(info: web::Query<Info>, body: bytes::Bytes, http: web::Data<Mutex<BotData>>) -> impl Responder {
    let result: Result<_> = try {
        let data = http.lock().unwrap(); // Static data

        let token = &info.token;
        if token.clone() != data.token {
            return format!("Invalid token {}",token).with_status(StatusCode::FORBIDDEN );
        }

        let http = &data.http;

        let bodystr = std::str::from_utf8(&body)?;

        if info.github {
            debug!("Handling web hook");
            let github:Github = serde_json::from_str(&bodystr)?;

            //It's api so it won't break too
            let r#ref = github.r#ref.split('/').last().context("Failed to get split result")?; 
            let commits = github.commits;

            let regex = regex::Regex::new(r"(?:\r\n|\r|\n)")?;

            let mut text = String::new();

            for commit in &commits {
                text += &format!("$Commit #{} by {}\n {}\n\n",
                &commit.id[..7],
                commit.author.name,
                regex.replace(commit.message.as_str(),"\n "));
            }

            &http.create_message(CID_GITHUB).content(
            format!("```md\n{} new commit(s) of {}:{}\n {} ```",
            commits.len(),
            github.repository.name,
            r#ref,
            text))?.await?;

            return format!("").with_status(StatusCode::OK);
        }

        match web::Query::<Body>::from_query(&bodystr) {
            Ok(qsl) => {
                if qsl.activity.len() > 0 {
                    debug!("Activity updated to {}",qsl.activity);
        
                    let mut activity = ACTIVITY_TEMPLATE.clone();
                    activity.name = qsl.activity.clone();
                    &data.shard.command(&UpdateStatus::new(false, activity, None, Status::Online)).await?;
                    return format!("Updating activity").with_status(StatusCode::OK);
                }
        
                let content	= &qsl.content;
                if content.len() < 1 || content.len() > 2000 {
                    return format!("Content size is invalid").with_status(StatusCode::INTERNAL_SERVER_ERROR);
                }
        
                debug!("Forwarding message: {}",content);
        
                &http.create_message(ChannelId(qsl.channelID)).content(content)?.await?;
        
                format!("Successfully passed message").with_status(StatusCode::OK)
            }
            Err(err) => {error!("QSL Parse: {}",err); return format!("Internal error").with_status(StatusCode::INTERNAL_SERVER_ERROR) }
        }
    };

    match result {
        Ok(a) => a,
        Err(err) => {error!("Web request error: {}",err); format!("Internal error").with_status(StatusCode::INTERNAL_SERVER_ERROR ) },
    }
}

async fn web(shard: &Shard, http: &HttpClient, hostname: &String, port: &u16, internal_token: &String, web_keypath: &String, web_chainpath: &String) -> Result<(),std::io::Error> {
    let mut builder =
    SslAcceptor::mozilla_intermediate(SslMethod::tls())?;
    builder.set_private_key_file(web_keypath, SslFiletype::PEM)?;
    builder.set_certificate_chain_file(web_chainpath)?;

    info!("Running web thread {}:{}", &hostname, &port);
    let data = web::Data::new(Mutex::new(BotData{shard:shard.clone(),http:http.clone(),token:internal_token.clone()}));
    
    HttpServer::new(move || {
        App::new()
            .app_data(data.clone()) 
            .route("/*", web::post().to(request))
    })
    .bind_openssl(format!("{}:{}",&hostname,&port),builder)?
    .run().await
}

// https://discordapp.com/channels/381880193251409931/700425302936911884/725105267905134612
fn get_avatar_url(user_id: UserId, avatar_hash: impl Into<String>) -> String {
    format!("https://cdn.discordapp.com/avatars/{}/a_{}.webp?size=256", user_id.0, avatar_hash.into())
}

#[actix_rt::main]
async fn main() -> Result<()> {
    // Default log level is debug
    env_logger::from_env(Env::default().default_filter_or("debug")).init();

    let five_secs = Duration::new(5, 0);

    let config:Config = envy::from_env::<Config>()?;

    let mut shard = Shard::new(&config.discord_token);
    shard.start().await?;

    let http = HttpClient::new(&config.discord_token);

    let (config1,http1,shard1) = (config.clone(),http.clone(),shard.clone());

    actix_rt::spawn(async move {
        loop {
            mysql(&shard1,&config1.mysql_hostname,&config1.mysql_port,&config1.mysql_user,&config1.mysql_password,&config1.mysql_dbname).await
            .map_err(|e|error!("Actvity task died: {}",e))
            .ok();
            actix_rt::time::delay_for(five_secs).await
        };
    });

    let shard2= shard.clone();

    actix_rt::spawn(async move {
        loop {
            web(&shard2,&http1,&config.web_hostname,&config.web_port,&config.botapi_token,&config.web_sslkey,&config.web_chain).await
            .map_err(|e|error!("Web task died: {}",e))
            .ok();
            actix_rt::time::delay_for(five_secs).await
        }
    });

    let client = Client::default();

    let cache_config = InMemoryConfigBuilder::new()
        .event_types(
            EventType::all()
        )
        .build();
    let cache = InMemoryCache::from(cache_config);

    let mut cattaches: Vec<ImagesData> = Vec::with_capacity(256);

    let mut events = shard.events().await;

    while let Some(event) = events.next().await {
        let result: Result<_> = try {
            match &event {
                Event::Ready(ready) => {
                    debug!("User '{}' is ready",ready.user.name);
                }
                Event::MessageCreate(msg) => {
                    if &msg.channel_id==&CID_LOG {continue}
                    let message = &***msg;
                    //Is it really possible to have multiple images in one message?
                    let mut images: Vec<Image> = Vec::with_capacity(16);
                    for attach in &message.attachments {
                        let mut response = client.get(&attach.proxy_url).send().await.unwrap();

                        if !response.status().is_success() {return Err(anyhow!("Web request failed {}",response.status()))}
                        
                        match response.body().limit(MAXFILESIZE).await {
                            Ok(body) => {
                                images.push(Image{name:attach.filename.to_owned(),body});
                                debug!("Caching attachment: {}", &attach.filename);
                            },
                            Err(c) => warn!("Warning: Failed to fetch image: {}\n {}",&attach.proxy_url,c)
                        }
                
                    }
                    if !images.is_empty() {
                        if cattaches.len()>=cattaches.capacity() {debug!("Stack is filled, draining first");cattaches.drain(0..1);}
                        debug!("Caching attachment {}/{}",cattaches.len(),cattaches.capacity());
                        cattaches.push(ImagesData{id:msg.id,images});
                    }
                }
                Event::MessageUpdate(msg) => {
                    if &msg.channel_id==&CID_LOG {continue}
                    
                    let oldmsg = cache.message(msg.channel_id,msg.id).await?.context("Message cache miss")?;
                    let gchannel = cache.guild_channel(oldmsg.channel_id).await?.context("Channel cache miss")?;
                    match gchannel.as_ref() {

                        GuildChannel::Text(ref c) => {

                            let author = msg.author.clone().context("UPD: Failed to get cached author")?;
                            let avatar = &author.avatar.to_owned().context("UPD: Failed to get avatar")?;
                            let newcontent = &msg.content.to_owned().context("UPD: Failed to content")?;
                            let timestamp = &msg.timestamp.to_owned().context("UPD: Failed to get timestamp")?;

                            let embed = EmbedBuilder::new()
                            .color(0xffd700)
                            .title(format!("at #{}",c.name))
                            .author()
                            .name(&author.name)
                            .icon_url(get_avatar_url(author.id,avatar))
                            .commit()
                            .description(format!("{} -> {}",oldmsg.content,newcontent))
                            .timestamp(timestamp)
                            .footer(format!("A:{} | M:{}",author.id,msg.id))
                            .commit()
                            .build();
                        
                            &http.create_message(CID_LOG).embed(embed)?.await?;
                        }
                        _ => {}
                    }
                    debug!("Updated message logged {}",msg.id);
                }
                Event::MessageDelete(msg) => {
                    if msg.channel_id==CID_LOG {continue}

                    let oldmsg = cache.message(msg.channel_id,msg.id).await?.context("Message cache miss")?;
                    let gchannel = cache.guild_channel(oldmsg.channel_id).await?.context("DEL:Failed to get cached channel")?;
                    match gchannel.as_ref() {

                        GuildChannel::Text(ref c) => {

                            let author = cache.user(oldmsg.author.clone()).await?.context("DEL:Failed to get cached author")?;
                            let avatar = &author.avatar.to_owned().context("UPD: Failed to get avatar")?;
                            let delcontent = &oldmsg.content;
                            let timestamp = &oldmsg.timestamp;

                            let embed = EmbedBuilder::new()
                            .color(0xb90702)
                            .title(format!("at #{}",c.name))
                            .author()
                            .name(&author.name)
                            .icon_url(get_avatar_url(author.id,avatar))
                            .commit()
                            .description(if delcontent.len() > 0 {delcontent} else {"Attachment only"})
                            .timestamp(timestamp)
                            .footer(format!("A:{} | M:{}",author.id,msg.id))
                            .commit()
                            .build();

                            http.create_message(CID_LOG).embed(embed)?.await?;

                            let image = cattaches.drain_filter(|data| data.id == msg.id).next();
                            if !image.is_none() {
                                let mut message = http.create_message(CID_LOG);

                                for image in image.unwrap().images {
                                    let name = image.name.clone();
                                    debug!("Restoring attachment {}",name);
                                    message = message.attachment(name,image.body.clone());
                                }
                            
                                message.await?;
                            } 
                        }
                        _ => {}
                    }
                    debug!("Deleted message logged {}",msg.id);
                }
                _ => {}
            }
            cache.update(&event).await.expect("Cache failed, OhNoe"); 
        };
        match result {
            Err(err) => error!("Discord events queue failed: {}",err),
            _ => {}
        };
    }

    Ok(())
}
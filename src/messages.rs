use std::sync::Arc;

use actix_web::client::Client;
use anyhow::{Context, Result};
use futures::stream::StreamExt;
use log::{info, warn};
use twilight_embed_builder::{EmbedAuthorBuilder, EmbedBuilder, EmbedFooterBuilder, ImageSource};
use twilight_cache_inmemory::{InMemoryCache, EventType};
use twilight_gateway::{Event, Shard};
use twilight_http::Client as TwilightHttp;
use twilight_model::{channel::GuildChannel,id::{ChannelId, MessageId, UserId},};

use actix_rt::Arbiter;

use crate::utils::config::Config;

const CID_LOG: ChannelId = ChannelId(697846732201000970);
const MAXFILESIZE: usize = 1000000000000000; // TODO: Get actual size of max file as usize

struct ImagesData {
    id: MessageId,
    images: Vec<Image>,
}

struct Image {
    name: String,
    body: bytes::Bytes,
}

// https://discordapp.com/channels/381880193251409931/700425302936911884/725105267905134612
fn get_avatar_url(user_id: UserId, avatar_hash: impl Into<String>) -> Result<ImageSource> {
    let avatar = ImageSource::url(
    format!(
        "https://cdn.discordapp.com/avatars/{}/a_{}.webp?size=256",
        user_id.0,
        avatar_hash.into()
    ))?;
    Ok(avatar)
}

async fn task(shard: &Shard, config: &Config) -> Result<()> {
    let cache_config = InMemoryCache::builder()
        .event_types(
            EventType::READY
                | EventType::GUILD_CREATE
                | EventType::GUILD_UPDATE
                | EventType::VOICE_STATE_UPDATE
                | EventType::VOICE_SERVER_UPDATE
                | EventType::UNAVAILABLE_GUILD
                | EventType::MEMBER_CHUNK
                | EventType::MEMBER_ADD
                | EventType::MEMBER_REMOVE
                | EventType::MEMBER_UPDATE
                | EventType::CHANNEL_CREATE
                | EventType::CHANNEL_UPDATE
                | EventType::MESSAGE_CREATE
                | EventType::MESSAGE_DELETE
                | EventType::MESSAGE_DELETE_BULK
                | EventType::MESSAGE_UPDATE,
        )
        .message_cache_size(32768)
        .build();
    let cache = InMemoryCache::from(cache_config);

    let client = Client::default();

    let http = TwilightHttp::new(&config.discord_token);

    let mut cattaches: Vec<ImagesData> = Vec::with_capacity(256);

    let mut events = shard.events();

    while let Some(event) = events.next().await {
        let result: Result<_> = try {
            match &event {
                Event::Ready(ready) => {
                    info!("User '{}' is ready", ready.user.name);
                }
                Event::MessageCreate(msg) => {
                    if &msg.channel_id == &CID_LOG {
                        continue;
                    }
                    let message = &***msg;
                    //Is it really possible to have multiple images in one message?
                    let mut images: Vec<Image> = Vec::with_capacity(16);
                    for attach in &message.attachments {
                        let response_res = client.get(&attach.proxy_url).send().await;

                        if response_res.is_err() {
                            warn!("MessageCreate: Actix web generic failure {}",response_res.unwrap_err());
                            continue;
                        }

                        let mut response = response_res.unwrap();

                        if !response.status().is_success() {
                            warn!("MessageCreate: Web request failed {}", response.status());
                            continue;
                        }

                        match response.body().limit(MAXFILESIZE).await {
                            Ok(body) => {
                                images.push(Image {
                                    name: attach.filename.to_owned(),
                                    body,
                                });
                                info!("MessageCreate: Caching attachment: {}", &attach.filename);
                            }
                            Err(c) => warn!(
                                "MessageCreate: Failed to fetch image: {}\n {}",
                                &attach.proxy_url, c
                            ),
                        }
                    }
                    if !images.is_empty() {
                        if cattaches.len() >= cattaches.capacity() {
                            info!("MessageCreate: Stack is filled, draining first");
                            cattaches.drain(0..1);
                        }
                        info!(
                            "Caching attachment {}/{}",
                            cattaches.len(),
                            cattaches.capacity()
                        );
                        cattaches.push(ImagesData { id: msg.id, images });
                    }
                }
                Event::MessageUpdate(msg) => {
                    if &msg.channel_id == &CID_LOG {
                        continue;
                    }

                    let oldmsg = cache
                        .message(msg.channel_id, msg.id)
                        .context("MessageUpdate: Message cache miss")?;

                    let gchannel = cache
                        .guild_channel(oldmsg.channel_id)
                        .context("MessageUpdate: Channel cache miss")?;

                    match gchannel.as_ref() {
                        GuildChannel::Text(ref c) => {
                            let author = msg
                                .author
                                .clone()
                                .context("MessageUpdate: Author cache miss")?;

                            let avatar = &author
                                .avatar
                                .to_owned()
                                .context("MessageUpdate: Avatar cache miss")?;

                            let newcontent = &msg
                                .content
                                .to_owned()
                                .context("MessageUpdate: Content cache miss")?;

                            let timestamp = &msg
                                .timestamp
                                .to_owned()
                                .context("MessageUpdate: Timestamp cache miss")?;

                            let embed = EmbedBuilder::new()
                                .color(0xffd700)?
                                .title(format!("at #{}", c.name))?
                                .author(EmbedAuthorBuilder::new()
                                .name(&author.name)?
                                .icon_url(get_avatar_url(author.id, avatar)?)
                                )
                                .description(format!("{} -> {}", oldmsg.content, newcontent))?
                                .timestamp(timestamp)
                                .footer(EmbedFooterBuilder::new(format!("A:{} | M:{}", author.id, msg.id))?)
                                .build()?;

                            &http.create_message(CID_LOG).embed(embed)?.await?;
                        }
                        _ => {}
                    }
                    info!("MessageUpdate: Message logged {}", msg.id);
                }
                Event::MessageDelete(msg) => {
                    if msg.channel_id == CID_LOG {
                        continue;
                    }

                    let oldmsg = cache
                        .message(msg.channel_id, msg.id)
                        .context("MessageDelete: Message cache miss")?;

                    let gchannel = cache
                        .guild_channel(oldmsg.channel_id)
                        .context("MessageDelete: Channel cache miss")?;

                    match gchannel.as_ref() {
                        GuildChannel::Text(ref c) => {
                            let author = cache
                                .user(oldmsg.author.clone())
                                .context("MessageDelete: Author cache miss")?;

                            let avatar = &author
                                .avatar
                                .to_owned()
                                .context("MessageDelete: Avatar cache miss")?;

                            let delcontent = &oldmsg.content;
                            let timestamp = &oldmsg.timestamp;

                            let embed = EmbedBuilder::new()
                                .color(0xb90702)?
                                .title(format!("at #{}", c.name))?
                                .author(EmbedAuthorBuilder::new()
                                .name(&author.name)?
                                .icon_url(get_avatar_url(author.id, avatar)?)
                                )
                                .description(if delcontent.len() > 0 {
                                    delcontent
                                } else {
                                    "Attachment only"
                                })?
                                .timestamp(timestamp)
                                .footer(EmbedFooterBuilder::new(format!("A:{} | M:{}", author.id, msg.id))?)
                                .build()?;

                            http.create_message(CID_LOG).embed(embed)?.await?;

                            let image = cattaches.drain_filter(|data| data.id == msg.id).next();

                            if !image.is_none() {
                                let mut message = http.create_message(CID_LOG);

                                for image in
                                    image.context("MessageDelete: Image cache miss")?.images
                                {
                                    let name = image.name.clone();
                                    info!("MessageDelete: Restoring attachment {}", name);
                                    message = message.attachment(name, image.body.clone());
                                }

                                message.await?;
                            }
                        }
                        _ => {}
                    }
                    info!("MessageDelete: Message logged {}", msg.id);
                }
                Event::VoiceStateUpdate(vcstate) => {
                    let mut vcstate = Arc::new((*vcstate.to_owned()).0);

                    let oldvcstate = cache
                        .voice_state(
                            vcstate.user_id,
                            vcstate
                                .guild_id
                                .context("VoiceStateUpdate: guild_id cache miss")?,
                        );

                    let mut color = 0x1a7701;
                    let mut message = "joined";

                    if oldvcstate.is_some() && vcstate.channel_id.is_none() {
                        vcstate = oldvcstate.context("VoiceStateUpdate: Voicestate cache miss")?;
                        color = 0x77011a;
                        message = "left";
                    }

                    let author = cache
                        .user(vcstate.user_id)
                        .context("VoiceStateUpdate: Author cache miss")?;

                    let avatar = &author
                        .avatar
                        .to_owned()
                        .context("VoiceStateUpdate: Avatar cache miss")?;

                    let gchannel = cache
                        .guild_channel(
                            vcstate
                                .channel_id
                                .context("VoiceStateUpdate: Channel id cache miss")?,
                        )
                        .context("VoiceStateUpdate: Channel cache miss")?;

                    match gchannel.as_ref() {
                        GuildChannel::Voice(ref c) => {
                            
                            let embed = EmbedBuilder::new()
                            .color(color)?
                            .author(EmbedAuthorBuilder::new()
                            .name(&author.name)?
                            .icon_url(get_avatar_url(author.id, avatar)?)
                            )
                            .description(format!(
                                "{} {} the voice chat 🔈{}",
                                author.name, message, c.name
                            ))?
                            .build()?;

                            &http.create_message(CID_LOG).embed(embed)?.await?;
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        };
        result.unwrap_or_else(|err| warn!("Discord events queue failed: {}", err));
        cache.update(&event);
    }

    Ok(())
}

pub async fn spawn(shard: &Shard, config: Config) -> Result<()> {
    let shard1 = shard.clone();

    Arbiter::spawn(async move {
        loop {
            task(&shard1, &config)
                .await
                .unwrap_or_else(|err| warn!("Discord messages task failed: {}", err));
        }
    });

    Ok(())
}


use futures::stream::StreamExt;
use twilight::{
    cache::{
        twilight_cache_inmemory::config::{InMemoryConfigBuilder, EventType},
        InMemoryCache,
    },
    model::{channel::GuildChannel, id::{ChannelId,UserId,MessageId}},
    gateway::{Shard, Event},
    builders::embed::EmbedBuilder,
    http::Client as TwilightHttp
};
use anyhow::{Result,Context,anyhow};
use log::{error, debug, warn};
use actix_web::client::Client;

const CID_LOG:ChannelId = ChannelId(697846732201000970);
const MAXFILESIZE:usize = 1000000000000000; // TODO: Get actual size of max file as usize

struct ImagesData {
    id: MessageId,
    images: Vec<Image>
}

struct Image {
    name: String,
    body: bytes::Bytes
}

// https://discordapp.com/channels/381880193251409931/700425302936911884/725105267905134612
fn get_avatar_url(user_id: UserId, avatar_hash: impl Into<String>) -> String {
    format!("https://cdn.discordapp.com/avatars/{}/a_{}.webp?size=256", user_id.0, avatar_hash.into())
}

pub async fn event_hanler(shard: &Shard, http: &TwilightHttp) -> Result<()> {

    let cache_config = InMemoryConfigBuilder::new()
        .event_types(
            EventType::all()
        )
        .build();
    let cache = InMemoryCache::from(cache_config);

    let client = Client::default();

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
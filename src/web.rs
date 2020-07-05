use std::sync::Mutex;
use log::{info, error, debug};
use anyhow::{Result,Context};
use serde::Deserialize;
use openssl::ssl::{SslAcceptor, SslAcceptorBuilder, SslFiletype, SslMethod};
use actix_web::{web, App, HttpServer, Responder, http::StatusCode};
use twilight::{
        model::id::ChannelId,
        http::Client as TwilightHttp
    };

use crate::utils::config::Config;

const CID_GITHUB:ChannelId = ChannelId(478623542380855306);

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
    http: TwilightHttp,
    token: String
}

#[derive(Deserialize,Debug)]
struct Info {
    token: String,
    #[serde(default)]
    github: bool,
}

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

async fn task(builder: SslAcceptorBuilder,data: web::Data<Mutex<BotData>>, addr: String) -> Result<(),std::io::Error> {
    HttpServer::new(move || {
        App::new()
            .app_data(data.clone()) 
            .route("/*", web::post().to(request))
    })
    .bind_openssl(addr,builder)?
    .run().await
}

pub async fn spawn(http: &TwilightHttp,config: Config) -> Result<()> {

    let mut builder =
    SslAcceptor::mozilla_intermediate(SslMethod::tls())?;
    builder.set_private_key_file(config.web_sslkey, SslFiletype::PEM)?;
    builder.set_certificate_chain_file(config.web_chain)?;

    let addr = format!("{}:{}", &config.web_hostname, &config.web_port);

    info!("Running web thread {}", addr);
    let data = web::Data::new(Mutex::new(BotData{http:http.clone(),token:config.botapi_token.clone()}));
    
    actix_rt::spawn(async move {
        task(builder, data, addr).await.unwrap()
    });

    Ok(())
}

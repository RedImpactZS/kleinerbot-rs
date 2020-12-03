use actix_web::{http::StatusCode, web, App, HttpServer, Responder};
use anyhow::{Context, Result};
use log::{error, info, warn};
use std::{fs::File, io::BufReader, error::Error};
use rust_tls::internal::pemfile::{certs, rsa_private_keys};
use rust_tls::{NoClientAuth, ServerConfig};
use serde::Deserialize;
use std::sync::Mutex;
use twilight_http::Client as TwilightHttp;
use twilight_model::id::ChannelId;

use actix_rt::Arbiter;

use crate::utils::config::Config;

const CID_GITHUB: ChannelId = ChannelId(478623542380855306);

#[derive(Deserialize)]
struct AuthorOrRepo {
    name: String,
}

#[derive(Deserialize)]
struct Commit {
    id: String,
    message: String,
    author: AuthorOrRepo,
}

#[derive(Deserialize)]
struct Github {
    r#ref: String,
    repository: AuthorOrRepo,
    commits: Vec<Commit>,
}

struct BotData {
    http: TwilightHttp,
    token: String,
}

#[derive(Deserialize, Debug)]
struct Info {
    token: String,
    #[serde(default)]
    github: bool,
}

#[derive(Deserialize, Debug)]
struct Body {
    #[serde(default)]
    activity: String,
    #[serde(default)]
    content: String,
    #[serde(default = "Body::default_channel")]
    channelID: u64,
}

impl Body {
    fn default_channel() -> u64 {
        544557150064738315
    }
}

async fn request(
    info: web::Query<Info>,
    body: bytes::Bytes,
    http: web::Data<Mutex<BotData>>,
) -> impl Responder {
    let result: Result<_> = try {
        let data = http.lock().unwrap(); // Static data

        let token = &info.token;
        if token.clone() != data.token {
            return format!("Invalid token {}", token).with_status(StatusCode::FORBIDDEN);
        }

        let http = &data.http;

        let bodystr = std::str::from_utf8(&body)?;

        if info.github {
            info!("Handling web hook");
            let github: Github = serde_json::from_str(&bodystr)?;

            //It's api so it won't break too
            let r#ref = github
                .r#ref
                .split('/')
                .last()
                .context("Failed to get split result")?;

            let commits = github.commits;

            let regex = regex::Regex::new(r"(?:\r\n|\r|\n)")?;

            let mut text = String::new();

            for commit in &commits {
                text += &format!(
                    "$Commit #{} by {}\n {}\n\n",
                    &commit.id[..7],
                    commit.author.name,
                    regex.replace(commit.message.as_str(), "\n ")
                );
            }

            &http
                .create_message(CID_GITHUB)
                .content(format!(
                    "```md\n{} new commit(s) of {}:{}\n {} ```",
                    commits.len(),
                    github.repository.name,
                    r#ref,
                    text
                ))?
                .await?;

            return format!("").with_status(StatusCode::OK);
        }

        match web::Query::<Body>::from_query(&bodystr) {
            Ok(qsl) => {
                let content = &qsl.content;
                if content.len() < 1 || content.len() > 2000 {
                    return format!("Content size is invalid")
                        .with_status(StatusCode::INTERNAL_SERVER_ERROR);
                }

                info!("Forwarding message: {}", content);

                &http
                    .create_message(ChannelId(qsl.channelID))
                    .content(content)?
                    .await?;

                format!("Successfully passed message").with_status(StatusCode::OK)
            }
            Err(err) => {
                error!("QSL Parse: {}", err);
                return format!("Internal error").with_status(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    };

    match result {
        Ok(a) => a,
        Err(err) => {
            error!("Web request error: {}", err);
            format!("Internal error").with_status(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn task(config: &Config) -> Result<(),Box<dyn Error>> {
    let mut ssl_config = ServerConfig::new(NoClientAuth::new());

    let ssl = config.web_ssl;

    if ssl {
        let cert_file = &mut BufReader::new(File::open(&config.web_cert)?);
        let key_file = &mut BufReader::new(File::open(&config.web_privkey)?);
        let cert_chain = certs(cert_file).unwrap();
        let mut keys = rsa_private_keys(key_file).unwrap();
        ssl_config.set_single_cert(cert_chain, keys.remove(0))?;
    
    }

    let addr = format!("{}:{}", &config.web_hostname, &config.web_port);

    info!("Running web thread {}", addr);
    let data = web::Data::new(Mutex::new(BotData {
        http: TwilightHttp::new(&config.discord_token),
        token: config.botapi_token.clone(),
    }));

    let mut server = HttpServer::new(move || {
        App::new()
            .app_data(data.clone())
            .route("/*", web::post().to(request))
    })
    .disable_signals();

    if ssl {
        server = server.bind_rustls(addr, ssl_config)?;
    } else {
        server = server.bind(addr)?;
    }

    server.run().await?;

    Ok(())
}

pub async fn spawn(config: Config) -> Result<()> {
    Arbiter::spawn(async move {
        loop {
            task(&config)
                .await
                .unwrap_or_else(|err| warn!("Web task failed: {}", err));
        }
    });

    Ok(())
}

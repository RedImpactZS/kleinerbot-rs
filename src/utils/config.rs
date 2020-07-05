use serde::Deserialize;
#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    #[serde(default = "Config::default_yes")]
    pub web_enabled: bool,
    #[serde(default = "Config::default_hostname")]
    pub web_hostname: String,
    #[serde(default = "Config::default_web_port")]
    pub web_port: u16,
    pub botapi_token: String,
    pub discord_token: String,
    #[serde(default = "Config::default_yes")]
    pub mysql_enabled: bool,
    #[serde(default = "Config::default_hostname")]
    pub mysql_hostname: String,
    #[serde(default = "Config::default_mysql_port")]
    pub mysql_port: u16,
    pub mysql_user: String,
    pub mysql_password: String,
    pub mysql_dbname: String,
    pub web_privkey: String,
    pub web_cert: String,
}

impl Config {
    fn default_hostname() -> String {
        "localhost".to_string()
    }
    fn default_web_port() -> u16 {
        3444
    }
    fn default_mysql_port() -> u16 {
        3306
    }
    fn default_yes() -> bool {
        true
    }
}

use std::env;
use std::sync::OnceLock;

use dotenvy::dotenv;

#[derive(Debug)]
pub struct Config {
    pub host: String,
    pub openai_api_key: String,
    pub google_maps_api_key: String,
    pub username: String,
    pub password: String,
    pub db_pass: String,
    pub db_name: String,
    pub static_file_dir: String,
}

impl Config {
    pub fn from_env() -> &'static Self {
        static CONFIG: OnceLock<Config> = OnceLock::new();
        CONFIG.get_or_init(|| {
            dotenv().ok();
            let host = env::var("HOST").expect("HOST must be set");
            let openai_api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");
            let google_maps_api_key =
                env::var("GOOGLE_MAPS_API_KEY").expect("GOOGLE_MAPS_API_KEY must be set");
            let username = env::var("BASIC_AUTH_USER").expect("BASIC_AUTH_USER must be set");
            let password = env::var("BASIC_AUTH_PASS").expect("BASIC_AUTH_PASS must be set");
            let db_pass = env::var("DB_APP_USER_PASS").expect("DB_APP_USER_PASS must be set");
            let db_name = env::var("DB_NAME").expect("DB_NAME must be set");
            let static_file_dir =
                env::var("STATIC_FILE_DIR").unwrap_or_else(|_| "static".to_string());

            Self {
                host,
                openai_api_key,
                google_maps_api_key,
                username,
                password,
                db_pass,
                db_name,
                static_file_dir,
            }
        })
    }

    pub fn get_db_url(&self) -> String {
        format!(
            "postgres://app_user:{}@localhost/{}",
            self.db_pass, self.db_name
        )
    }
}

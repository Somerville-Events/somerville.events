use std::env;
use std::sync::OnceLock;

use dotenvy::dotenv;

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub openai_api_key: String,
    pub google_maps_api_key: String,
    pub username: String,
    pub password: String,
    pub db_user: String,
    pub db_pass: String,
    pub db_name: String,
    pub static_file_dir: String,
    pub openai_base_url: String,
    pub google_maps_base_url: String,
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
            let db_user = env::var("DB_APP_USER").expect("DB_APP_USER must be set");
            let db_pass = env::var("DB_APP_USER_PASS").expect("DB_APP_USER_PASS must be set");
            let db_name = env::var("DB_NAME").expect("DB_NAME must be set");
            let static_file_dir =
                env::var("STATIC_FILE_DIR").unwrap_or_else(|_| "static".to_string());
            let openai_base_url = env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
            let google_maps_base_url = env::var("GOOGLE_MAPS_BASE_URL")
                .unwrap_or_else(|_| "https://places.googleapis.com/v1".to_string());

            Self {
                host,
                openai_api_key,
                google_maps_api_key,
                username,
                password,
                db_user,
                db_pass,
                db_name,
                static_file_dir,
                openai_base_url,
                google_maps_base_url,
            }
        })
    }

    pub fn get_db_url(&self) -> String {
        format!(
            "postgres://{}:{}@localhost/{}",
            self.db_user, self.db_pass, self.db_name
        )
    }
}

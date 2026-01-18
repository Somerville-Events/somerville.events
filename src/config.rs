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
    pub public_url: String,
    pub activitypub_private_key_pem: String,
    pub activitypub_public_key_pem: String,
}

impl Config {
    pub fn from_env() -> &'static Self {
        static CONFIG: OnceLock<Config> = OnceLock::new();
        CONFIG.get_or_init(|| {
            dotenv().ok();
            let host = env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
            let openai_api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");
            let google_maps_api_key =
                env::var("GOOGLE_MAPS_API_KEY").expect("GOOGLE_MAPS_API_KEY must be set");
            let username = env::var("BASIC_AUTH_USER").expect("BASIC_AUTH_USER must be set");
            let password = env::var("BASIC_AUTH_PASS").expect("BASIC_AUTH_PASS must be set");
            let db_pass = env::var("DB_APP_USER_PASS").expect("DB_APP_USER_PASS must be set");
            let db_name = env::var("DB_NAME").expect("DB_NAME must be set");
            let static_file_dir =
                env::var("STATIC_FILE_DIR").unwrap_or_else(|_| "static".to_string());
            let public_url = env::var("PUBLIC_URL").expect("PUBLIC_URL must be set");
            let activitypub_private_key_pem = env::var("ACTIVITYPUB_PRIVATE_KEY_PEM")
                .expect("ACTIVITYPUB_PRIVATE_KEY_PEM must be set");
            let activitypub_public_key_pem = env::var("ACTIVITYPUB_PUBLIC_KEY_PEM")
                .expect("ACTIVITYPUB_PUBLIC_KEY_PEM must be set");

            Self {
                host,
                openai_api_key,
                google_maps_api_key,
                username,
                password,
                db_pass,
                db_name,
                static_file_dir,
                public_url,
                activitypub_private_key_pem,
                activitypub_public_key_pem,
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

use std::env;
use std::sync::OnceLock;

pub struct Config {
    pub host: String,
    pub api_key: String,
    pub username: String,
    pub password: String,
    pub db_user: String,
    pub db_pass: String,
    pub db_name: String,
    pub static_file_dir: String,
}

impl Config {
    pub fn from_env() -> Self {
        let host = env::var("HOST").expect("HOST must be set");
        let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");
        let username = env::var("BASIC_AUTH_USER").expect("BASIC_AUTH_USER must be set");
        let password = env::var("BASIC_AUTH_PASS").expect("BASIC_AUTH_PASS must be set");
        let db_user = env::var("DB_APP_USER").expect("DB_APP_USER must be set");
        let db_pass = env::var("DB_APP_USER_PASS").expect("DB_APP_USER_PASS must be set");
        let db_name = env::var("DB_NAME").expect("DB_NAME must be set");
        let static_file_dir = env::var("STATIC_FILE_DIR").unwrap_or_else(|_| "./static".to_string());

        Self {
            host,
            api_key,
            username,
            password,
            db_user,
            db_pass,
            db_name,
            static_file_dir,
        }
    }

    pub fn global() -> &'static Config {
        static CONFIG: OnceLock<Config> = OnceLock::new();
        CONFIG.get_or_init(Config::from_env)
    }

    pub fn get_db_url(&self) -> String {
        format!("postgres://{}:{}@localhost/{}", self.db_user, self.db_pass, self.db_name)
    }
}


use anyhow::Result;
use somerville_events::config::Config;
use somerville_events::startup;
use std::net::TcpListener;

#[actix_web::main]
async fn main() -> Result<()> {
    let config = Config::from_env();
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    log::info!("Starting server at http://localhost:8080");

    let host = config.host.clone();
    let port = 8080;
    let address = format!("{}:{}", host, port);
    let listener = TcpListener::bind(address)?;

    startup::run(listener, config.clone()).await?.await?;
    Ok(())
}

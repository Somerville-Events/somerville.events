use anyhow::{anyhow, Result};
use awc;
/**
 * Common utility for scrapers.
 */
use chaser_oxide::{Browser, BrowserConfig, ChaserPage, ChaserProfile};
use futures::StreamExt;
use sqlx::postgres::{PgPoolOptions, Postgres};
use sqlx::Pool;

/// Shared scraper struct
pub struct Scraper {
    pub http_client: awc::Client,
    chaser: Option<(ChaserPage, Browser)>,
    pub pool: Pool<Postgres>,
}

impl Scraper {
    pub async fn new(db_url: &str) -> anyhow::Result<Self> {
        // Create HTTP client
        let http_client = awc::Client::default();

        // Connect to database
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(db_url)
            .await
            .map_err(|e| anyhow!("Failed to connect to database: {}", e))?;

        log::info!("Connected to database");

        Ok(Scraper {
            http_client,
            chaser: None,
            pool,
        })
    }

    /// Returns a reference to the chaser page
    /// Initializes it if this is called for the first time
    pub async fn browser(&mut self) -> Result<&ChaserPage> {
        if let Some(ref chaser) = self.chaser {
            Ok(&chaser.0)
        } else {
            // Launch browser
            let (browser, mut handler) = Browser::launch(
                BrowserConfig::builder()
                    .new_headless_mode()
                    .build()
                    .map_err(|e| anyhow::anyhow!("{}", e))?,
            )
            .await?;

            actix_rt::spawn(async move { while (handler.next().await).is_some() {} });

            // Create page and wrap in ChaserPage
            let page = browser.new_page("about:blank").await?;
            let chaser = ChaserPage::new(page);

            // Apply the fingerprint profile.
            let profile = ChaserProfile::macos_arm().build();
            chaser.apply_profile(&profile).await?;

            self.chaser = Some((chaser, browser));
            Ok(&self.chaser.as_ref().unwrap().0)
        }
    }
}

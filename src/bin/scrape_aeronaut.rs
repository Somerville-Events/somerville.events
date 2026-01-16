/**
 * Aeronaut Brewing events scraper
 *
 * Aeronaut formerly hosted "*_events.json" directly on their domain, but they've
 * switched to a randomly generated cloudfront URL. To avoid hardcoding this, in case
 * it changes in the future, the approach here is:
 *
 *   1. Scrape the page to get all fetched JSON URLs
 *   2. Load the URLs and see which are active
 *   3. Assume the URL that loads fits their custom event schema
 *
 * We then map that schema back into our event format.
 *
 * Aeronaut uses Cloudflare, so we use chaser_oxide to bypass its checks.
 */

use chaser_oxide::{Browser, BrowserConfig, ChaserPage, ChaserProfile};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use actix_rt::System;
use chrono::{DateTime, Utc};
use somerville_events::models::{Event, EventType, EventSource};
use regex::Regex;
use awc;

// Aeronaut's JSON structure for events
#[derive(Debug, Serialize, Deserialize)]
pub struct AeronautEvent {
    pub category: String,
    pub date: String,
    pub description: String,
    pub end: String,
    pub extlink: String, // or url::Url
    pub featured: Option<bool>,
    pub img_url: String, // or url::Url
    pub name: String,
    pub start: String,
    pub tickets: String, // or url::Url
    pub venue_slug: String,
}

pub struct Scraper {
    http_client: awc::Client,
    chaser: ChaserPage,
}

impl Scraper {
    pub fn new(http_client: awc::Client, chaser: ChaserPage) -> Self {
        Scraper { http_client, chaser }
    }

    pub async fn scrape_events(&self) -> anyhow::Result<Vec<Event>> {
        // Navigate to the page
        self.chaser.goto("https://www.aeronautbrewing.com/visit/somerville/").await?;
        actix_rt::time::sleep(std::time::Duration::from_millis(1000)).await;

        // Extract script contents
        let elements = self.chaser.evaluate("[].slice.apply(document.querySelectorAll('script')).map((s) => s.innerText)").await?;

        let mut urls = Vec::new();
        if let Some(elements) = elements {
            if let Some(elements) = elements.as_array() {
                for element in elements {
                    let content = element.to_string();

                    let patterns = vec![
                        r#"getJSON\("([^"]+)"#,
                        r#"getJSON\('([^']+)"#,
                    ];

                    for pattern in &patterns {
                        let regex = Regex::new(pattern)?;
                        for cap in regex.captures_iter(&content) {
                            if let Some(url_match) = cap.get(1) {
                                let url = url_match.as_str();
                                if url.contains("public_event") {
                                    urls.push(url.to_string());
                                    eprintln!("Found URL in jQuery.getJSON: {}", url);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Download and parse JSON from each URL to find a valid one
        let mut events: Vec<AeronautEvent> = vec![];
        for url in urls {
            eprintln!("Processing URL: {}", url);
            let mut response = match self.http_client.get(&url).send().await {
                Ok(resp) => resp,
                Err(e) => {
                    eprintln!("Failed to fetch {}: {}", url, e);
                    continue;
                }
            };

            let status = response.status();
            if !status.is_success() {
                eprintln!("HTTP error {} for {}", status, url);
                continue;
            }

            let json_text = match response.body().await {
                Ok(body) => String::from_utf8(body.to_vec())?,
                Err(e) => {
                    eprintln!("Failed to read response body for {}: {}", url, e);
                    continue;
                }
            };

            match serde_json::from_str::<Vec<AeronautEvent>>(&json_text) {
                Ok(parsed_events) => {
                    eprintln!("Successfully parsed JSON from {}:", url);
                    eprintln!();
                    events = parsed_events;
                    break;
                }
                Err(e) => {
                    eprintln!("Failed to parse JSON from {}: {}", url, e);
                }
            }
        }

        Ok(events.iter().map(convert_to_external_event).collect())
    }
}

fn main() -> anyhow::Result<()> {
    // Use Actix runtime
    System::new().block_on(async {
        // Create HTTP client
        let http_client = awc::Client::default();

        // Launch browser
        let (browser, mut handler) = Browser::launch(
            BrowserConfig::builder().new_headless_mode().build()
                .map_err(|e| anyhow::anyhow!("{}", e))?,
        ).await?;

        actix_rt::spawn(async move {
            while let Some(_) = handler.next().await {}
        });

        // Create page and wrap in ChaserPage
        let page = browser.new_page("about:blank").await?;
        let chaser = ChaserPage::new(page);

        // Apply the fingerprint profile.
        let profile = ChaserProfile::macos_arm().build();
        chaser.apply_profile(&profile).await?;

        // Create scraper and scrape events
        let scraper = Scraper::new(http_client, chaser);
        let events = scraper.scrape_events().await?;

        println!("{:?}", events);

        // TODO ingest into SQLite

        Ok(())
    })
}

// All events are at their building, so hardcoding this:
const AERONAUT_STREET_ADDRESS: &str = "14 Tyler St
Somerville, MA
02143";

fn convert_to_external_event(event: &AeronautEvent) -> Event {
    // Parse datetime strings into chrono DateTime<Utc>
    let start_date = DateTime::parse_from_rfc3339(&event.start)
        .unwrap_or_else(|_| DateTime::parse_from_rfc3339("1970-01-01T00:00:00Z").unwrap())
        .with_timezone(&Utc);

    let end_date = DateTime::parse_from_rfc3339(&event.end)
        .unwrap_or_else(|_| DateTime::parse_from_rfc3339("1970-01-01T00:00:00Z").unwrap())
        .with_timezone(&Utc);

    // Determine event types from category
    let event_types = guess_event_types(&event.category);

    Event {
        name: event.name.clone(),
        description: event.description.clone(),
        full_text: event.description.clone(),
        start_date,
        end_date: Some(end_date),
        address: Some(AERONAUT_STREET_ADDRESS.to_string()),
        original_location: Some("Aeronaut Brewing".to_string()),
        google_place_id: None,
        location_name: Some("Aeronaut Brewing".to_string()),
        event_types,
        url: Some(event.extlink.clone()),
        confidence: 0.95, // High confidence for direct scraping
        id: None,
        age_restrictions: None,
        price: None,
        source: EventSource::AeronautBrewing,
        external_id: Some(format!("aeronaut-{}", event.name.replace(" ", "-").to_lowercase())),
    }
}

// Helper function to determine event types from Aeronaut's "category" label
fn guess_event_types(category: &str) -> Vec<EventType> {
    let category_lower = category.to_lowercase();

    match category_lower.as_str() {
        s if Regex::new(r"(music|live)").unwrap().is_match(s) => vec![EventType::Music],
        s if Regex::new(r"(food|drink)").unwrap().is_match(s) => vec![EventType::Food],
        s if Regex::new(r"(art|gallery)").unwrap().is_match(s) => vec![EventType::Art],
        s if Regex::new(r"(theater|performance)").unwrap().is_match(s) => vec![EventType::Theater],
        s if Regex::new(r"comedy").unwrap().is_match(s) => vec![EventType::Comedy],
        s if Regex::new(r"(market|farmers)").unwrap().is_match(s) => vec![EventType::Market],
        s if Regex::new(r"(workshop|class)").unwrap().is_match(s) => vec![EventType::Workshop],
        s if Regex::new(r"(film|movie)").unwrap().is_match(s) => vec![EventType::Film],
        s if Regex::new(r"(fundraiser|charity)").unwrap().is_match(s) => vec![EventType::Fundraiser],
        s if Regex::new(r"(holiday|seasonal)").unwrap().is_match(s) => vec![EventType::Holiday],
        s if Regex::new(r"(family|kids)").unwrap().is_match(s) => vec![EventType::ChildFriendly],
        _ => vec![EventType::Other],
    }
}

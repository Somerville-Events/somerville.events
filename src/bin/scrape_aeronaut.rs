use chaser_oxide::{Browser, BrowserConfig, ChaserPage, ChaserProfile};
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::time::{sleep, Duration};

// JSON structure that Aeronaut uses.
#[derive(Debug, Serialize, Deserialize)]
pub struct Event {
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

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
struct ExternalEvent {
    id: String,
    title: String,
    description: String,
    start_datetime: String,
    end_datetime: Option<String>,
    all_day: bool,
    venue_name: Option<String>,
    street_address: Option<String>,
    city: Option<String>,
    state: Option<String>,
    zip_code: Option<String>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    category: String,
    tags: Vec<String>,
    family_friendly: bool,
    age_restrictions: Option<String>,
    cost: Option<String>,
    registration_required: bool,
    source_url: Option<String>,
    source_name: String,
    scraped_at: String,
    last_updated: String,
    contact_email: Option<String>,
    contact_phone: Option<String>,
    website_url: Option<String>,
    image_url: Option<String>,
    recurring_pattern: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Launch browser
    let (browser, mut handler) = Browser::launch(
        BrowserConfig::builder().new_headless_mode().build()
            .map_err(|e| anyhow::anyhow!("{}", e))?,
    ).await?;

    tokio::spawn(async move {
        while let Some(_) = handler.next().await {}
    });

    // Create page and wrap in ChaserPage
    let page = browser.new_page("about:blank").await?;
    let chaser = ChaserPage::new(page);

    // Apply the fingerprint profile.
    let profile = ChaserProfile::macos_arm().build();
    chaser.apply_profile(&profile).await?;

    // Aeronaut brewing formerly hosted their own events.json, but it seems they've
    // switched to a randomly generated cloudfront URL. Rather than hardcode this,
    // just scrape the page to get the URL pull. We need to scrape due to the page
    // using Cloudflare for blocking any regular old bot.
    chaser.goto("https://www.aeronautbrewing.com/visit/somerville/").await?;
    sleep(Duration::from_millis(1000)).await;
    let elements = chaser.evaluate("[].slice.apply(document.querySelectorAll('script')).map((s) => s.innerText)").await?;

    let mut urls = Vec::new();
    if let Some(elements) = elements {
        if let Some(elements) = elements.as_array() {
            for element in elements {
                let content = element.to_string();

                let patterns = vec![
                    r#"getJSON\("([^"]+)""#,
                    r#"getJSON\('([^']+)'"#,
                ];

                for pattern in &patterns {
                    if let Ok(regex) = regex::Regex::new(pattern) {
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
    }

    // 6. Download and parse JSON from each URL to find a valid one
    let client = Client::new();
    let mut events: Vec<Event> = vec![];
    for url in urls {
        eprintln!("Processing URL: {}", url);
        let response = match client.get(&url).send().await {
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

        let json_text = match response.text().await {
            Ok(text) => text,
            Err(e) => {
                eprintln!("Failed to read response body for {}: {}", url, e);
                continue;
            }
        };

        // Parse as structured JSON (just print for now)
        match serde_json::from_str::<Vec<Event>>(&json_text) {
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

    let structured_events = events.iter().map(convert_to_external_event);

    println!("{:?}", structured_events);

    // TODO ingest into SQLite

    Ok(())
}

fn convert_to_external_event(event: &Event) -> ExternalEvent {
    ExternalEvent {
        id: String::new(), // Will need to generate this
        title: event.name.clone(),
        description: event.description.clone(),
        start_datetime: event.start.clone(),
        end_datetime: Some(event.end.clone()),
        all_day: false, // May want to add logic to determine this
        venue_name: None,
        street_address: None,
        city: None,
        state: None,
        zip_code: None,
        latitude: None,
        longitude: None,
        category: event.category.clone(),
        tags: vec![],
        family_friendly: false,
        age_restrictions: None,
        cost: None,
        registration_required: false,
        source_url: Some(event.extlink.clone()),
        source_name: "aeronautbrewing.com".to_string(),
        scraped_at: chrono::Utc::now().to_rfc3339(), // Current time as ISO 8601
        last_updated: event.date.clone(), // Using the date field from Event
        contact_email: None,
        contact_phone: None,
        website_url: Some(event.extlink.clone()),
        image_url: Some(event.img_url.clone()),
        recurring_pattern: None,
    }
}

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use somerville_events::{
    config::Config,
    database::save_event_to_db,
    geocoding::{canonicalize_address, GeocodedLocation},
    models::{Event, EventSource, EventType},
};
use sqlx::postgres::PgPoolOptions;
use std::collections::{HashMap, HashSet};
use std::env;

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

#[actix_web::main]
async fn main() -> Result<()> {
    // Initialize logger
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    // Check for dry-run flag
    let args: Vec<String> = env::args().collect();
    let dry_run = args.contains(&"--dry-run".to_string());

    if dry_run {
        log::info!("Running in DRY-RUN mode. No changes will be saved to DB and no Geocoding API calls will be made.");
    }

    // Load config
    let config = Config::from_env();
    let db_url = config.get_db_url();

    // Connect to database
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await
        .map_err(|e| anyhow!("Failed to connect to database: {}", e))?;

    log::info!("Connected to database");

    // Fetch existing external IDs to avoid re-processing and paying for geocoding
    // We fetch all external_ids that are not null.
    // Ideally we should filter by source if we knew it ahead of time, but we process
    // all sources from the feed.
    let existing_ids: HashSet<String> =
        sqlx::query!("SELECT external_id FROM app.events WHERE external_id IS NOT NULL")
            .fetch_all(&pool)
            .await?
            .into_iter()
            .filter_map(|r| r.external_id)
            .collect();

    log::info!("Found {} existing events in database", existing_ids.len());

    // Fetch events
    let url = "https://web-production-00281.up.railway.app/events?upcoming_only=true&limit=5000";
    log::info!("Fetching events from {}", url);

    let client = awc::Client::default();
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|e| anyhow!("Failed to fetch events: {}", e))?;

    if !response.status().is_success() {
        return Err(anyhow!("Request failed with status: {}", response.status()));
    }

    // We deserialize to Value first to handle individual errors gracefully
    let raw_events: Vec<serde_json::Value> = response
        .json()
        .limit(20 * 1024 * 1024) // 20MB limit
        .await
        .map_err(|e| anyhow!("Failed to parse JSON: {}", e))?;

    log::info!("Fetched {} raw events", raw_events.len());

    let mut valid_external_events = Vec::new();
    let mut error_count = 0;

    // Parse all events first
    for raw in raw_events {
        match serde_json::from_value::<ExternalEvent>(raw) {
            Ok(ext_event) => {
                // If event already exists in DB, skip it entirely
                if existing_ids.contains(&ext_event.id) {
                    continue;
                }
                valid_external_events.push(ext_event);
            }
            Err(e) => {
                log::warn!("Skipping invalid event schema: {}", e);
                error_count += 1;
            }
        }
    }

    log::info!(
        "Identified {} new/changed events to process ({} skipped as existing, {} schema errors)",
        valid_external_events.len(),
        existing_ids.len(), // Approximate since we didn't count overlaps exactly, but close enough
        error_count
    );

    // Deduplicate addresses for geocoding
    // Map Raw Address String -> Option<GeocodedLocation>
    let mut address_cache: HashMap<String, Option<GeocodedLocation>> = HashMap::new();
    let mut unique_addresses_to_geocode = HashSet::new();

    for ext in &valid_external_events {
        let raw_addr = build_raw_address(ext);
        if let Some(addr) = raw_addr {
            unique_addresses_to_geocode.insert(addr);
        }
    }

    log::info!(
        "Need to geocode {} unique addresses",
        unique_addresses_to_geocode.len()
    );

    if dry_run {
        log::info!(
            "DRY-RUN: Would geocode {} addresses",
            unique_addresses_to_geocode.len()
        );
        log::info!(
            "DRY-RUN: Would insert {} events",
            valid_external_events.len()
        );
        return Ok(());
    }

    // Geocode addresses
    for raw_addr in unique_addresses_to_geocode {
        match canonicalize_address(&client, &raw_addr, &config.google_maps_api_key).await {
            Ok(loc) => {
                if loc.is_none() {
                    log::warn!("Could not geocode address: {}", raw_addr);
                }
                address_cache.insert(raw_addr, loc);
            }
            Err(e) => {
                log::error!("Failed to geocode address '{}': {}", raw_addr, e);
                // Insert None to avoid retrying if we logic-looped, but here we just iterate set once
                address_cache.insert(raw_addr, None);
            }
        }
    }

    let mut success_count = 0;
    let mut db_error_count = 0;

    for ext_event in valid_external_events {
        let raw_addr = build_raw_address(&ext_event);
        let geocoded = raw_addr
            .as_ref()
            .and_then(|a| address_cache.get(a).cloned().flatten());

        match map_and_save_event(&pool, ext_event, geocoded).await {
            Ok(_) => success_count += 1,
            Err(e) => {
                log::error!("Failed to save event: {}", e);
                db_error_count += 1;
            }
        }
    }

    log::info!(
        "Ingestion complete. Success: {}, DB Errors: {}, Schema Errors: {}",
        success_count,
        db_error_count,
        error_count
    );

    Ok(())
}

fn build_raw_address(ext: &ExternalEvent) -> Option<String> {
    let mut address_parts = Vec::new();
    if let Some(venue) = &ext.venue_name {
        address_parts.push(venue.clone());
    }
    if let Some(street) = &ext.street_address {
        if !street.trim().is_empty() {
            address_parts.push(street.clone());
        }
    }
    if let Some(city) = &ext.city {
        address_parts.push(city.clone());
    }
    if let Some(state) = &ext.state {
        address_parts.push(state.clone());
    }
    if let Some(zip) = &ext.zip_code {
        address_parts.push(zip.clone());
    }

    if address_parts.is_empty() {
        None
    } else {
        Some(address_parts.join(", "))
    }
}

async fn map_and_save_event(
    pool: &sqlx::Pool<sqlx::Postgres>,
    ext: ExternalEvent,
    geocoded: Option<GeocodedLocation>,
) -> Result<()> {
    // Parse timestamps
    let start_date = DateTime::parse_from_rfc3339(&ext.start_datetime)
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| {
            use chrono::NaiveDateTime;
            use chrono::TimeZone;
            use chrono_tz::America::New_York;

            NaiveDateTime::parse_from_str(&ext.start_datetime, "%Y-%m-%dT%H:%M:%S")
                .map(|ndt| {
                    New_York
                        .from_local_datetime(&ndt)
                        .single()
                        .unwrap()
                        .with_timezone(&Utc)
                })
                .map_err(|e| anyhow!("Failed to parse start date '{}': {}", ext.start_datetime, e))
        })
        .map_err(|e| anyhow!("Date parsing error: {}", e))?;

    let end_date = if let Some(ref end_str) = ext.end_datetime {
        Some(
            DateTime::parse_from_rfc3339(end_str)
                .map(|dt| dt.with_timezone(&Utc))
                .or_else(|_| {
                    use chrono::NaiveDateTime;
                    use chrono::TimeZone;
                    use chrono_tz::America::New_York;

                    NaiveDateTime::parse_from_str(end_str, "%Y-%m-%dT%H:%M:%S")
                        .map(|ndt| {
                            New_York
                                .from_local_datetime(&ndt)
                                .single()
                                .unwrap()
                                .with_timezone(&Utc)
                        })
                        .map_err(|e| anyhow!("Failed to parse end date '{}': {}", end_str, e))
                })?,
        )
    } else {
        None
    };

    // Map source
    let source = map_source(&ext.source_name);

    // Map category to event types
    let mut event_types = Vec::new();
    if let Some(et) = map_category(&ext.category) {
        event_types.push(et);
    }
    if ext.family_friendly {
        event_types.push(EventType::ChildFriendly);
    }

    // Determine address fields based on geocoding result or fallback to raw
    let (address, google_place_id, location_name, original_location) = if let Some(geo) = geocoded {
        (
            Some(geo.formatted_address),
            Some(geo.place_id),
            // Use venue name from source if available, otherwise name from Google (which might be the venue name)
            ext.venue_name.clone().or(Some(geo.name)),
            build_raw_address(&ext), // Original location is the raw string we built
        )
    } else {
        // Fallback to raw construction
        let raw = build_raw_address(&ext);
        (raw.clone(), None, ext.venue_name.clone(), raw)
    };

    // Parse price
    let price = ext.cost.as_ref().and_then(|c| {
        // Simple extraction: remove '$' and parse float
        let cleaned = c.replace(['$', ','], "");
        cleaned.parse::<f64>().ok()
    });

    let event = Event {
        id: None, // Let DB assign ID
        name: ext.title,
        description: ext.description.clone(),
        full_text: "".to_string(),
        start_date,
        end_date,
        address,
        original_location,
        google_place_id,
        location_name,
        event_types,
        url: ext.source_url.or(ext.website_url),
        confidence: 1.0,
        age_restrictions: ext.age_restrictions,
        price,
        source,
        external_id: Some(ext.id),
    };

    save_event_to_db(pool, &event).await?;

    Ok(())
}

fn map_source(source_name: &str) -> EventSource {
    match source_name {
        "Aeronaut Brewing" => EventSource::AeronautBrewing,
        "American Repertory Theater" => EventSource::AmericanRepertoryTheater,
        "Arts at the Armory" => EventSource::ArtsAtTheArmory,
        "Boston Swing Central" => EventSource::BostonSwingCentral,
        "BostonShows.org" => EventSource::BostonShowsOrg,
        "Brattle Theatre" => EventSource::BrattleTheatre,
        "Central Square Theater" => EventSource::CentralSquareTheater,
        "City of Cambridge" => EventSource::CityOfCambridge,
        "Harvard Art Museums" => EventSource::HarvardArtMuseums,
        "Harvard Book Store" => EventSource::HarvardBookStore,
        "Lamplighter Brewing" => EventSource::LamplighterBrewing,
        "Porter Square Books" => EventSource::PorterSquareBooks,
        "Portico Brewing" => EventSource::PorticoBrewing,
        "Sanders Theatre" => EventSource::SandersTheatre,
        "Somerville Theatre" => EventSource::SomervilleTheatre,
        "The Comedy Studio" => EventSource::TheComedyStudio,
        "The Lily Pad" => EventSource::TheLilyPad,
        "First Parish in Cambridge" => EventSource::FirstParishInCambridge,
        "Grolier Poetry Book Shop" => EventSource::GrolierPoetryBookShop,
        "User Submitted" => EventSource::UserSubmitted,
        "The Middle East" => EventSource::TheMiddleEast,
        _ => {
            log::warn!(
                "Unknown source: '{}', defaulting to ImageUpload (which is used as fallback)",
                source_name
            );
            EventSource::ImageUpload
        }
    }
}

fn map_category(category: &str) -> Option<EventType> {
    match category.to_lowercase().as_str() {
        "music" => Some(EventType::Music),
        "arts and culture" => Some(EventType::Art),
        "food and drink" => Some(EventType::Food),
        "theater" => Some(EventType::Theater),
        "lectures" => Some(EventType::Workshop),
        "community" => Some(EventType::Meeting),
        "sports" => Some(EventType::Sports),
        "other" => Some(EventType::Other),
        _ => {
            log::debug!("Unknown category: '{}', mapping to Other", category);
            Some(EventType::Other)
        }
    }
}

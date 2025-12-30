use crate::models::{Event, EventType};
use actix_web::web;
use anyhow::{anyhow, Result};
use awc::Client;
use base64::{engine::general_purpose::STANDARD as b64, Engine as _};
use chrono::{DateTime, LocalResult, NaiveDateTime, TimeZone, Utc};
use chrono_tz::America::New_York;
use futures_util::future;
use image::{DynamicImage, ImageFormat, ImageReader};
use rxing::{
    common::HybridBinarizer, qrcode::QRCodeReader, BinaryBitmap, BufferedImageLuminanceSource,
    DecodeHintValue, DecodeHints, ImmutableReader,
};
use schemars::schema_for;
use serde_json::json;
use std::{
    io::Cursor,
    path::Path,
    sync::{Arc, LazyLock},
};
use url::Url;

static QR_READER: LazyLock<QRCodeReader> = LazyLock::new(QRCodeReader::default);

static SCHEMA_STR: LazyLock<String> = LazyLock::new(|| {
    let schema = schema_for!(ImageEventExtraction);
    serde_json::to_string_pretty(&schema).unwrap()
});

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SingleEventExtraction {
    pub name: Option<String>,
    pub description: Option<String>,
    /// Format: YYYY-MM-DDTHH:MM:SS (No timezone)
    pub start_date: Option<NaiveDateTime>,
    /// Format: YYYY-MM-DDTHH:MM:SS (No timezone)
    pub end_date: Option<NaiveDateTime>,
    pub location: Option<String>,
    /// "YardSale" | "Art" | "Music" | "Dance" | "Performance" | "Food" | "PersonalService" | "Meeting" | "Government" | "Volunteer" | "Fundraiser" | "Film" | "Theater" | "Comedy" | "Literature" | "Exhibition" | "Workshop" | "Fitness" | "Market" | "Sports" | "Family" | "Social" | "Holiday" | "Religious" | "ChildFriendly" | "Other"
    pub event_types: Option<Vec<String>>,
    pub url: Option<String>,
    /// Confidence level of the extraction (0.0 to 1.0)
    pub confidence: f64,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ImageEventExtraction {
    /// All readable text in the image
    pub full_text: Option<String>,
    pub events: Vec<SingleEventExtraction>,
}

pub async fn parse_image(image_path: &Path, client: &Client, api_key: &str) -> Result<Vec<Event>> {
    parse_image_with_now(image_path, Utc::now(), client, api_key).await
}

async fn parse_image_with_now(
    image_path: &Path,
    now: DateTime<Utc>,
    client: &Client,
    api_key: &str,
) -> Result<Vec<Event>> {
    let path = image_path.to_path_buf();

    // Offload blocking I/O (file read) to thread pool
    let bytes = web::block(move || std::fs::read(&path))
        .await
        .map_err(|e| anyhow!("Blocking task failed: {}", e))??;

    // Wrap the image in a reference counter to share it between tasks
    // without copying the image data. Saves us some memory and overhead.
    let bytes = Arc::new(bytes);

    let format = ImageReader::new(Cursor::new(bytes.as_slice()))
        .with_guessed_format()
        .map_err(|e| anyhow!("Failed to guess image format: {}", e))?
        .format()
        .ok_or_else(|| anyhow!("Unknown image format"))?;

    match format {
        ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::Gif | ImageFormat::WebP => {}
        _ => return Err(anyhow!("Image format must be jpg, png, gif, or webp")),
    };

    // Concurrently process image with
    //   A) QR Code extraction (CPU intensive)
    //   B) LLM (Network intensive)

    // Task A: QR Code Extraction (CPU intensive)
    let bytes_for_qr = bytes.clone();
    let qr_future = web::block(move || {
        let reader =
            ImageReader::new(Cursor::new(bytes_for_qr.as_slice())).with_guessed_format()?;
        let img = reader.decode()?;
        Ok::<Option<Url>, anyhow::Error>(extract_qr_url(img))
    });

    // Task B: LLM Extraction (Network intensive)
    let now_str = now.to_rfc3339();
    let mime_type = format.to_mime_type();
    let b64_data = b64.encode(bytes.as_slice());
    let data_url = format!("data:{mime_type};base64,{b64_data}");
    let payload = json!({
        "model": "gpt-4o-mini",
        "temperature": 0,
        "response_format": { "type": "json_object" },
        "messages": [
            {
                "role": "system",
                "content": format!(
                    r#"You are an expert at extracting event information from images.
                        You must respond with a JSON object that matches this exact schema:
                        {schema_str}
                        
                        Instructions:
                        - Extract all distinct events found in the image.
                        - If a poster lists multiple dates for the same event (e.g. a series), treat each date as a separate event in the `events` list.
                        - If you are uncertain about any fields, set them to null.
                        - The full_text field should contain all readable text from the image.
                        - The description field should be the description of the event.
                        - The confidence should be a number between 0.0 and 1.0 indicating how confident you are in the extraction.
                        - Focus on extracting event-related information like the name, date, time, location, url, and description.
                        - Try to always extract at least one event type in event_types.
                        - Today's date is {now_str}.
                        - The start_date and end_date must be formatted as ISO 8601 strings without timezone offset (e.g., "YYYY-MM-DDTHH:MM:SS").
                        - All events are in the Somerville/Cambridge/Boston area (America/New_York timezone).
                        - Assume the event is in the future unless the text clearly indicates it is in the past.
                        - If the date is ambiguous (e.g. "Friday"), assume it is the next occurrence after today's date ({now_str}).
                        - DO NOT default the date to {now_str} if no date is found; return null instead.
                        - Do not make up a URL. Only include a URL if it is explicitly written in the image.
                        - Do not attempt to decode QR codes. Only extract URLs that are visible as text.
                        - Be thorough but accurate. Return only valid JSON.
                        - Do not return the schema in your response.
                        "#
                , schema_str = *SCHEMA_STR)
            },
            {
                "role": "user",
                "content": [
                    { "type": "text", "text": "Extract all text and events from this image and return it in the specified JSON format." },
                    { "type": "image_url", "image_url": { "url": data_url } }
                ]
            }
        ]
    });
    let llm_future = client
        .post("https://api.openai.com/v1/chat/completions")
        .insert_header(("Authorization", format!("Bearer {api_key}")))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&payload);

    // Save some time by doing QR Parsing and making
    // a network request to the LLM at the same time
    let (qr_result, llm_result) = future::join(qr_future, llm_future).await;

    let mut resp = llm_result.map_err(|e| anyhow!("HTTP request failed: {e}"))?;

    let body = resp
        .body()
        .await
        .map_err(|e| anyhow!("Failed to read response body: {e}"))?;

    if !resp.status().is_success() {
        return Err(anyhow!(
            "OpenAI API error ({}): {}",
            resp.status(),
            String::from_utf8_lossy(&body)
        ));
    }

    let json: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|e| anyhow!("Failed to parse JSON response: {}", e))?;

    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();

    log::debug!("Extracted content: {}", content);

    let mut events = parse_and_validate_response(&content)?;

    let qr_url = qr_result.map_err(|e| anyhow!("QR task failed: {}", e))??;

    if let Some(qr_url) = qr_url {
        log::info!("QR code URL detected: {qr_url}");
        for event in &mut events {
            event.url = Some(qr_url.to_string());
        }
    }

    Ok(events)
}

fn datetime_from_naive(naive_local: NaiveDateTime) -> Option<DateTime<Utc>> {
    // Interpret `naive_local` as a *local wall-clock time* in the America/NewYork timezone.
    // All our event posters should be from the Camberville area so this is a
    // safe assumption for now.
    match New_York.from_local_datetime(&naive_local) {
        LocalResult::Single(datetime) => Some(datetime.with_timezone(&Utc)),
        LocalResult::Ambiguous(earlier, _later) => Some(earlier.with_timezone(&Utc)),
        LocalResult::None => None,
    }
}

fn parse_and_validate_response(content: &str) -> Result<Vec<Event>> {
    // Strip markdown code blocks if present.
    // LLMs like to surround code in them.
    let clean_content = if content.trim().starts_with("```") {
        content
            .lines()
            .skip(1) // Skip ```json
            .take_while(|l| !l.trim().starts_with("```"))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        content.to_string()
    };

    let extraction: ImageEventExtraction = serde_json::from_str(&clean_content)
        .map_err(|e| anyhow!("Failed to parse JSON: {} (Content: {})", e, clean_content))?;

    let full_text = extraction.full_text.unwrap_or_default();
    let mut valid_events = Vec::new();

    for extracted_event in extraction.events {
        let name = match extracted_event.name {
            Some(n) if !n.trim().is_empty() => n,
            _ => {
                log::info!("Skipping event extraction missing name");
                continue;
            }
        };

        let start_date = match extracted_event.start_date {
            Some(naive) => match datetime_from_naive(naive) {
                Some(dt) => dt,
                None => {
                    log::warn!("Invalid local time for start_date: {:?}", naive);
                    continue;
                }
            },
            None => {
                log::info!("Skipping event extraction missing start_date");
                continue;
            }
        };

        let end_date = match extracted_event.end_date {
            Some(naive) => match datetime_from_naive(naive) {
                Some(dt) => Some(dt),
                None => {
                    log::warn!("Invalid local time for end_date: {:?}", naive);
                    None
                }
            },
            None => None,
        };

        valid_events.push(Event {
            name,
            start_date,
            description: extracted_event.description.unwrap_or_default(),
            full_text: full_text.clone(),
            end_date,
            address: None,
            original_location: extracted_event.location,
            google_place_id: None,
            location_name: None,
            event_types: extracted_event
                .event_types
                .unwrap_or_default()
                .into_iter()
                .map(EventType::from)
                .collect(),
            url: extracted_event.url,
            confidence: extracted_event.confidence,
            id: None,
            age_restrictions: None, // Logic for extraction could be added here if schema supported it
            price: None,            // Logic for extraction could be added here if schema supported it
            source_name: None,
        });
    }

    Ok(valid_events)
}

fn extract_qr_url(image: DynamicImage) -> Option<Url> {
    let luminance = BufferedImageLuminanceSource::new(image);
    let binarizer = HybridBinarizer::new(luminance);
    let mut binary_image = BinaryBitmap::new(binarizer);
    let hints = DecodeHints::default().with(DecodeHintValue::TryHarder(true));

    match QR_READER.immutable_decode_with_hints(&mut binary_image, &hints) {
        Ok(result) => Url::parse(result.getText()).ok(),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    use super::*;
    use chrono::TimeZone;

    fn get_test_client() -> Client {
        awc::ClientBuilder::new()
            .timeout(std::time::Duration::from_secs(120))
            .finish()
    }

    #[actix_web::test]
    async fn test_parse_single_event() -> Result<()> {
        let config = Config::from_env();
        let client = get_test_client();

        let fixed_now_utc = Utc.with_ymd_and_hms(2025, 1, 15, 17, 0, 0).unwrap();
        let events = parse_image_with_now(
            Path::new("examples/dance_flyer.jpg"),
            fixed_now_utc,
            &client,
            &config.openai_api_key,
        )
        .await?;

        assert!(
            !events.is_empty(),
            "Expected at least one event to be parsed"
        );
        let event = &events[0];

        assert!(
            event.name.eq_ignore_ascii_case("Dance Therapy"),
            "Name mismatch: {}",
            event.name
        );
        assert_eq!(
            event.start_date,
            Utc.with_ymd_and_hms(2025, 6, 23, 4, 0, 0).unwrap()
        );

        Ok(())
    }

    #[actix_web::test]
    async fn test_parse_not_an_event_selfie() -> Result<()> {
        let config = Config::from_env();
        let client = get_test_client();

        let fixed_now_utc = Utc.with_ymd_and_hms(2025, 1, 15, 17, 0, 0).unwrap();

        // This image should NOT be parsed as an event
        let events = parse_image_with_now(
            Path::new("examples/selfie.jpg"),
            fixed_now_utc,
            &client,
            &config.openai_api_key,
        )
        .await?;

        assert!(
            events.is_empty(),
            "Expected empty list for selfie.jpg, but got {:?}",
            events
        );

        Ok(())
    }

    #[actix_web::test]
    async fn test_parse_not_an_event_soda_ad() -> Result<()> {
        let config = Config::from_env();
        let client = get_test_client();

        let fixed_now_utc = Utc.with_ymd_and_hms(2025, 1, 15, 17, 0, 0).unwrap();

        // This image should NOT be parsed as an event
        let events = parse_image_with_now(
            Path::new("examples/soda_ad.jpg"),
            fixed_now_utc,
            &client,
            &config.openai_api_key,
        )
        .await?;

        assert!(
            events.is_empty(),
            "Expected empty list for soda_ad.jpg, but got {:?}",
            events
        );

        Ok(())
    }

    #[actix_web::test]
    async fn test_flyer_with_qr_code() -> Result<()> {
        let config = Config::from_env();
        let client = get_test_client();

        let fixed_now_utc = Utc.with_ymd_and_hms(2024, 10, 1, 12, 0, 0).unwrap();

        let events = parse_image_with_now(
            Path::new("examples/pumpkin_smash.jpeg"),
            fixed_now_utc,
            &client,
            &config.openai_api_key,
        )
        .await?;

        assert!(!events.is_empty(), "Expected event to be parsed");
        let event = &events[0];

        let url = event.url.as_ref().expect("Failed to decode QR code");

        assert_eq!(
            url,
            "https://www.somervillema.gov/events/2025/11/08/pumpkin-smash",
        );

        // 10:30 AM EST = 15:30 UTC
        assert_eq!(
            event.start_date,
            Utc.with_ymd_and_hms(2025, 11, 8, 15, 30, 0).unwrap(),
            "Start date mismatch: {:?}",
            event.start_date
        );
        // 1:00 PM EST = 13:00 EST = 18:00 UTC
        assert_eq!(
            event.end_date,
            Some(Utc.with_ymd_and_hms(2025, 11, 8, 18, 0, 0).unwrap()),
            "End date mismatch: {:?}",
            event.end_date
        );

        Ok(())
    }

    #[test]
    fn test_qr_decode_poster() -> Result<()> {
        let img = image::open("examples/large_qr_code_poster.jpg")?;
        let url = extract_qr_url(img).expect("Failed to decode QR code");
        let expected = Url::parse("https://www.eastsomervillemainstreets.org/event-details/halloween-block-party-pet-spooktacular-2025-2")?;
        assert_eq!(url, expected);
        Ok(())
    }

    #[actix_web::test]
    async fn test_parse_multiple_events() -> Result<()> {
        let config = Config::from_env();
        let client = get_test_client();

        // Saturday, August 16th is in 2025
        let fixed_now_utc = Utc.with_ymd_and_hms(2025, 1, 1, 12, 0, 0).unwrap();

        let events = parse_image_with_now(
            Path::new("examples/dsnc_flyer.png"),
            fixed_now_utc,
            &client,
            &config.openai_api_key,
        )
        .await?;

        assert_eq!(events.len(), 3, "Expected 3 events");

        let expected_times = [
            (
                Utc.with_ymd_and_hms(2025, 8, 16, 13, 0, 0).unwrap(),
                Some(Utc.with_ymd_and_hms(2025, 8, 16, 17, 0, 0).unwrap()),
            ),
            (
                Utc.with_ymd_and_hms(2025, 8, 23, 13, 0, 0).unwrap(),
                Some(Utc.with_ymd_and_hms(2025, 8, 23, 17, 0, 0).unwrap()),
            ),
            (
                Utc.with_ymd_and_hms(2025, 8, 25, 13, 0, 0).unwrap(),
                Some(Utc.with_ymd_and_hms(2025, 8, 25, 22, 0, 0).unwrap()),
            ),
        ];

        for (i, event) in events.iter().enumerate() {
            assert!(event
                .name
                .to_lowercase()
                .contains("neighborhood council election"),);
            assert_eq!(event.start_date, expected_times[i].0,);
            assert_eq!(event.end_date, expected_times[i].1,);
            assert_eq!(
                event.original_location.as_deref(),
                Some("Somerville Library West Branch"),
            );
            assert_eq!(
                event.url,
                Some("https://sites.google.com/view/davissquarenc/elections".to_string())
            )
        }

        Ok(())
    }
}

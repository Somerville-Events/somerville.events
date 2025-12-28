use crate::models::{Event, EventType};
use actix_web::web;
use anyhow::{anyhow, Result};
use awc::Client;
use base64::{engine::general_purpose::STANDARD as b64, Engine as _};
use chrono::{DateTime, LocalResult, NaiveDateTime, TimeZone, Utc};
use chrono_tz::America::New_York;
use image::{DynamicImage, ImageFormat, ImageReader};
use rxing::{
    common::HybridBinarizer, qrcode::QRCodeReader, BinaryBitmap, BufferedImageLuminanceSource,
    DecodeHintValue, DecodeHints, ImmutableReader,
};
use schemars::schema_for;
use serde_json::json;
use std::{io::Cursor, path::Path, sync::LazyLock};
use url::Url;

static QR_READER: LazyLock<QRCodeReader> = LazyLock::new(QRCodeReader::default);

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ImageEventExtraction {
    pub name: Option<String>,
    pub full_description: Option<String>,
    /// Format: YYYY-MM-DDTHH:MM:SS (No timezone)
    pub start_date: Option<NaiveDateTime>,
    /// Format: YYYY-MM-DDTHH:MM:SS (No timezone)
    pub end_date: Option<NaiveDateTime>,
    pub location: Option<String>,
    /// "YardSale" | "Art" | "Music" | "Dance" | "Performance" | "Food" | "PersonalService" | "Meeting" | "Government" | "Volunteer" | "Fundraiser" | "Film" | "Theater" | "Comedy" | "Literature" | "Exhibition" | "Workshop" | "Fitness" | "Market" | "Sports" | "Family" | "Social" | "Holiday" | "Religious" | "Other"
    pub event_type: Option<String>,
    pub url: Option<String>,
    /// Confidence level of the extraction (0.0 to 1.0)
    pub confidence: f64,
}

pub async fn parse_image(
    image_path: &Path,
    client: Client,
    api_key: &str,
) -> Result<Option<Event>> {
    parse_image_with_now(image_path, Utc::now(), client, api_key).await
}

async fn parse_image_with_now(
    image_path: &Path,
    now: DateTime<Utc>,
    client: Client,
    api_key: &str,
) -> Result<Option<Event>> {
    let path = image_path.to_path_buf();

    // Offload blocking I/O (file read) and CPU intensive task (image decoding + QR extraction) to thread pool
    // Note: We return ImageFormat and bytes separately to avoid re-encoding or re-reading
    let (format, image_bytes, qr_url) = web::block(move || {
        let bytes = std::fs::read(&path)?;
        let reader = ImageReader::new(Cursor::new(&bytes)).with_guessed_format()?;

        let fmt = match reader.format() {
            Some(
                f @ (ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::Gif | ImageFormat::WebP),
            ) => f,
            _ => return Err(anyhow!("Image format must be jpg, png, gif, or webp")),
        };

        let img = reader.decode()?;
        // Try to deterministically extract a url for the event out of
        // any QR code that may be in the image before we toss the whole
        // image into a multi-modal machine learning model.
        let url = extract_qr_url(img);
        Ok((fmt, bytes, url))
    })
    .await
    .map_err(|e| anyhow!("Blocking task failed: {}", e))??;

    // Base64 encode -> data URL
    let mime_type = format.to_mime_type();
    let b64_data = b64.encode(&image_bytes);
    let data_url = format!("data:{mime_type};base64,{b64_data}");
    let schema = schema_for!(ImageEventExtraction);
    let schema_str = serde_json::to_string_pretty(&schema).unwrap();
    let now_str = now.to_rfc3339();

    // Build Chat Completions payload with instructor format
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
                        - Extract as much information as possible from the image.
                        - If you are uncertain about any fields, set them to null.
                        - The full_description field should contain all readable text from the image.
                        - The confidence should be a number between 0.0 and 1.0 indicating how confident you are in the extraction.
                        - Focus on extracting event-related information like the name, date, time, location, url, and description.
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
                )
            },
            {
                "role": "user",
                "content": [
                    { "type": "text", "text": "Extract all text from this image and return it in the specified JSON format." },
                    { "type": "image_url", "image_url": { "url": data_url } }
                ]
            }
        ]
    });

    // Send request with the shared Actix client
    let mut resp = client
        .post("https://api.openai.com/v1/chat/completions")
        .insert_header(("Authorization", format!("Bearer {api_key}")))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&payload)
        .await
        .map_err(|e| anyhow!("HTTP request failed: {e}"))?;

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
    let mut event = parse_and_validate_response(&content)?;

    if let Some(qr_url) = qr_url {
        log::info!("QR code URL detected; overriding parsed URL with {qr_url}");
        if let Some(event) = event.as_mut() {
            event.url = Some(qr_url.to_string());
        }
    }

    Ok(event)
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

fn parse_and_validate_response(content: &str) -> Result<Option<Event>> {
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

    let name = match extraction.name {
        Some(n) if !n.trim().is_empty() => n,
        _ => {
            log::info!("Extraction missing name, treating as no event");
            return Ok(None);
        }
    };

    let start_date = match extraction.start_date {
        Some(naive) => match datetime_from_naive(naive) {
            Some(dt) => dt,
            None => {
                log::warn!("Invalid local time for start_date: {:?}", naive);
                return Ok(None);
            }
        },
        None => {
            log::info!("Extraction missing start_date, treating as no event");
            return Ok(None);
        }
    };

    let end_date = match extraction.end_date {
        Some(naive) => match datetime_from_naive(naive) {
            Some(dt) => Some(dt),
            None => {
                log::warn!("Invalid local time for end_date: {:?}", naive);
                None
            }
        },
        None => None,
    };

    Ok(Some(Event {
        name,
        start_date,
        full_description: extraction.full_description.unwrap_or_default(),
        end_date,
        address: None,
        original_location: extraction.location,
        google_place_id: None,
        location_name: None,
        event_type: extraction.event_type.map(EventType::from),
        url: extraction.url,
        confidence: extraction.confidence,
        id: None,
    }))
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
    async fn test_parse_image() -> Result<()> {
        let config = Config::from_env();
        let client = get_test_client();

        let fixed_now_utc = Utc.with_ymd_and_hms(2025, 1, 15, 17, 0, 0).unwrap();
        let event_opt = parse_image_with_now(
            Path::new("examples/dance_flyer.jpg"),
            fixed_now_utc,
            client,
            &config.openai_api_key,
        )
        .await?;
        let event = event_opt.expect("Expected an event to be parsed");
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
        let event_opt = parse_image_with_now(
            Path::new("examples/selfie.jpg"),
            fixed_now_utc,
            client,
            &config.openai_api_key,
        )
        .await?;

        assert!(
            event_opt.is_none(),
            "Expected None for selfie.jpg, but got {:?}",
            event_opt
        );

        Ok(())
    }

    #[actix_web::test]
    async fn test_parse_not_an_event_soda_ad() -> Result<()> {
        let config = Config::from_env();
        let client = get_test_client();

        let fixed_now_utc = Utc.with_ymd_and_hms(2025, 1, 15, 17, 0, 0).unwrap();

        // This image should NOT be parsed as an event
        let event_opt = parse_image_with_now(
            Path::new("examples/soda_ad.jpg"),
            fixed_now_utc,
            client,
            &config.openai_api_key,
        )
        .await?;

        assert!(
            event_opt.is_none(),
            "Expected None for soda_ad.jpg, but got {:?}",
            event_opt
        );

        Ok(())
    }

    #[actix_web::test]
    async fn test_flyer_with_qr_code() -> Result<()> {
        let config = Config::from_env();
        let client = get_test_client();

        let fixed_now_utc = Utc.with_ymd_and_hms(2024, 10, 1, 12, 0, 0).unwrap();

        let event = parse_image_with_now(
            Path::new("examples/pumpkin_smash.jpeg"),
            fixed_now_utc,
            client,
            &config.openai_api_key,
        )
        .await?
        .expect("Event was not parsed");

        print!("{event:?}");

        let url = event.url.expect("Failed to decode QR code");

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
}

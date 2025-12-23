use crate::models::Event;
use anyhow::{anyhow, Result};
use awc::Client;
use base64::{engine::general_purpose::STANDARD as b64, Engine as _};
use chrono::{DateTime, Utc};
use schemars::schema_for;
use serde_json::json;
use std::{fs, path::Path};
use url::Url;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ImageEventExtraction {
    pub name: Option<String>,
    pub full_description: Option<String>,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub location: Option<String>,
    pub event_type: Option<String>,
    pub url: Option<String>,
    pub confidence: f64,
}

pub struct AiService {
    pub client: Client,
    pub api_key: String,
}

impl AiService {
    pub fn new(client: Client, api_key: String) -> Self {
        Self { client, api_key }
    }

    pub async fn parse_image(&self, image_path: &Path) -> Result<Option<Event>> {
        self.parse_image_with_now(image_path, Utc::now()).await
    }

    pub async fn parse_image_with_now(
        &self,
        image_path: &Path,
        now: DateTime<Utc>,
    ) -> Result<Option<Event>> {
        // Read file
        let bytes = fs::read(image_path)
            .map_err(|e| anyhow!("Failed to read file: {image_path:?} - {e}"))?;
        let qr_url = extract_qr_url(&bytes);

        // Guess MIME type
        let mime_type = mime_guess::from_path(image_path)
            .first_raw()
            .map(|m| m.to_string())
            .unwrap_or_else(|| "image/png".to_string());

        // Base64 encode -> data URL
        let b64_data = b64.encode(&bytes);
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
                        - The full_description field should contain all readable text from the image.
                        - The confidence should be a number between 0.0 and 1.0 indicating how confident you are in the extraction.
                        - Focus on extracting event-related information like the name, date, time, location, url, and description.
                        - Today's date is {now_str}.
                        - The start_date and end_date must be RFC 3339 formatted date and time strings.
                        - Assume the event is in the future unless the text clearly indicates it is in the past.
                        - Assume the event is in the timezone of the location if provided.
                        - If the date is ambiguous (e.g. "Friday"), assume it is the next occurrence after today's date ({now_str}).
                        - DO NOT default the date to {now_str} if no date is found; return null instead.
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
        let mut resp = self.client
            .post("https://api.openai.com/v1/chat/completions")
            .insert_header(("Authorization", format!("Bearer {}", self.api_key)))
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
}

fn parse_and_validate_response(content: &str) -> Result<Option<Event>> {
    let extraction: ImageEventExtraction = match serde_json::from_str(content) {
        Ok(e) => e,
        Err(_e) => {
            let json: serde_json::Value = serde_json::from_str(content)
                .map_err(|e| anyhow!("Failed to parse JSON: {}", e))?;
            serde_json::from_value(json)
                .map_err(|e| anyhow!("Failed to parse into ImageEventExtraction: {}", e))?
        }
    };

    let name = match extraction.name {
        Some(n) if !n.trim().is_empty() => n,
        _ => {
            log::info!("Extraction missing name, treating as no event");
            return Ok(None);
        }
    };

    let start_date = match extraction.start_date {
        Some(d) => d,
        None => {
            log::info!("Extraction missing start_date, treating as no event");
            return Ok(None);
        }
    };

    Ok(Some(Event {
        name,
        start_date,
        full_description: extraction.full_description.unwrap_or_default(),
        end_date: extraction.end_date,
        location: extraction.location,
        event_type: extraction.event_type,
        url: extraction.url,
        confidence: extraction.confidence,
        id: None,
    }))
}

fn extract_qr_url(bytes: &[u8]) -> Option<Url> {
    let img = image::load_from_memory(bytes).ok()?.to_luma8();
    let mut prepared = rqrr::PreparedImage::prepare(img);

    prepared.detect_grids().into_iter().find_map(|grid| {
        grid.decode()
            .ok()
            .and_then(|(_, content)| Url::parse(&content).ok())
    })
}


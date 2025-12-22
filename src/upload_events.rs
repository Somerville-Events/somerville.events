use crate::common_ui::COMMON_STYLES;
use crate::models::Event;
use crate::AppState;
use actix_multipart::form::{tempfile::TempFile, MultipartForm};
use actix_web::{http::header::ContentType, web::Data, HttpResponse};
use anyhow::{anyhow, Result};
use awc::Client;
use base64::{engine::general_purpose::STANDARD as b64, Engine as _};
use chrono::{DateTime, Utc};
use schemars::schema_for;
use serde_json::json;
use std::{fs, path::Path};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ImageEventExtraction {
    pub name: Option<String>,
    pub full_description: Option<String>,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub location: Option<String>,
    pub event_type: Option<String>,
    pub confidence: f64,
}

#[derive(Debug, MultipartForm)]
pub struct Upload {
    pub image: TempFile,
    pub idempotency_key: actix_multipart::form::text::Text<uuid::Uuid>,
}

pub async fn index() -> HttpResponse {
    let idempotency_key = uuid::Uuid::new_v4();
    HttpResponse::Ok()
        .content_type(ContentType::html())
        .body(format!(
            r#"<!doctype html>
        <html lang="en">
        <head>
            <meta name="color-scheme" content="light dark">
            <meta name="viewport" content="width=device-width, minimum-scale=1, initial-scale=1">
            <title>Somerville Events Upload</title>
            <script type="module" src="/static/upload.js"></script>
            <style>
                {common_styles}

                /* Camera Mode Layout (applied when camera is active) */
                body:not(.no-camera) {{
                    margin: 0;
                    padding: 0;
                    width: 100%;
                    height: 100dvh;
                    max-width: none;
                    display: flex;
                    flex-direction: column;
                    background-color: #000;
                    overflow: hidden;
                }}

                /* Hide standard headers in camera mode */
                body:not(.no-camera) h1, 
                body:not(.no-camera) p {{
                    display: none;
                }}
                
                /* Fallback mode uses default body styles from common_styles */

                /* Main container takes available space */
                .camera-ui {{
                    flex: 1;
                    display: flex;
                    flex-direction: column;
                    position: relative;
                    background: #000;
                    overflow: hidden;
                }}

                /* Video fills the available space, reserving bottom for controls */
                /* Container for video/preview to manage flex layout correctly */
                .viewport-container {{
                    flex: 1;
                    position: relative;
                    width: 100%;
                    overflow: hidden;
                    background: #000;
                    display: flex;
                    justify-content: center;
                    align-items: center;
                }}

                video {{
                    width: 100%;
                    height: 100%;
                    object-fit: contain;
                    display: block;
                }}
                
                video.loading {{
                    opacity: 0;
                }}

                /* Skeleton Loader */
                .skeleton {{
                    position: absolute;
                    top: 0;
                    left: 0;
                    width: 100%;
                    height: 100%;
                    background: #1a1a1a;
                    display: flex;
                    justify-content: center;
                    align-items: center;
                    z-index: 10;
                }}
                
                .skeleton::after {{
                    content: "";
                    width: 40px;
                    height: 40px;
                    border: 3px solid rgba(255,255,255,0.1);
                    border-top-color: rgba(255,255,255,0.5);
                    border-radius: 50%;
                    animation: spin 1s ease-in-out infinite;
                }}
                
                /* Hide skeleton when not loading */
                .skeleton.hidden {{
                    display: none;
                }}

                /* Controls bar at bottom - now static, not absolute */
                .controls-bar {{
                    width: 100%;
                    padding: 20px;
                    box-sizing: border-box;
                    background: #000; /* Solid black background */
                    display: flex;
                    justify-content: center;
                    align-items: center;
                    min-height: 100px; /* Explicit space reserved */
                    gap: 1rem;
                    z-index: 20;
                }}

                /* Upload Form (Hidden if JS active and camera works) */
                form {{
                    display: none;
                    gap: 1.5rem;
                    margin-top: 1rem;
                }}

                /* State: No Camera / No JS */
                body.no-camera {{
                    background-color: canvas; /* Reset to default */
                    height: auto;
                    display: block;
                }}
                body.no-camera .camera-ui {{
                    display: none;
                }}
                body.no-camera form {{
                    display: flex;
                    flex-direction: column;
                    align-items: flex-start;
                }}

                /* Spinner */
                .spinner {{
                    display: inline-block;
                    width: 1em;
                    height: 1em;
                    border: 3px solid rgba(255,255,255,0.3);
                    border-radius: 50%;
                    border-top-color: #fff;
                    animation: spin 1s ease-in-out infinite;
                    margin-right: 0.5rem;
                }}
                @keyframes spin {{ to {{ transform: rotate(360deg); }} }}
                
                /* File Input Styling */
                /* Use ::file-selector-button for modern native styling */
                form input[type=file] {{
                    display: block; /* Ensure it's visible */
                    width: 100%;
                    cursor: pointer;
                    font-family: system-ui, sans-serif;
                    font-size: 1rem;
                }}

                /* Style the button part specifically */
                form input[type=file]::file-selector-button {{
                    margin-right: 1rem;
                }}

                /* Image Preview (No JS specific, but if JS fails to load camera) */
                form img {{
                    max-width: 100%;
                    margin-top: 1rem;
                    display: none;
                    border-radius: 4px;
                }}

            </style>
        </head>
        <body class="no-camera"> <!-- Default to no-camera, upgraded by JS -->
            <h1>Upload Event Flyer</h1>
            <p>Upload an image of a flyer or event poster.</p>
            
            <!-- Full Screen Camera UI -->
            <div class="camera-ui">
                <div class="viewport-container">
                    <div class="skeleton"></div>
                    <video class="loading" autoplay playsinline muted></video>
                </div>
                
                <div class="controls-bar">
                    <button type="button">Choose Photo</button>
                    <button type="button" class="primary">Take Photo</button>
                </div>
            </div>

            <!-- Hidden canvas for capture -->
            <canvas style="display: none;"></canvas>

            <!-- Actual Form (Visible, hidden when Camera UI active) -->
            <form action="/upload" method="post" enctype="multipart/form-data">
                <input type="hidden" name="idempotency_key" value="{idempotency_key}">
                
                <input type="file" name="image" accept="image/*" required>

                <img alt="Selected Image Preview">

                <button type="submit">Upload</button>
            </form>
        </body>
        </html>"#,
            common_styles = COMMON_STYLES,
            idempotency_key = idempotency_key
        ))
}

pub async fn save(
    state: Data<AppState>,
    MultipartForm(req): MultipartForm<Upload>,
) -> HttpResponse {
    let idempotency_key = req.idempotency_key.0;

    // Check for idempotency
    match state
        .events_repo
        .claim_idempotency_key(idempotency_key)
        .await
    {
        Ok(true) => {
            // New request, proceed
        }
        Ok(false) => {
            // Duplicate request
            log::warn!(
                "Duplicate upload attempt blocked for key: {}",
                idempotency_key
            );
            return HttpResponse::Conflict().body("Upload already in progress or completed.");
        }
        Err(e) => {
            log::error!("Database error checking idempotency: {e}");
            return HttpResponse::InternalServerError().body("Database error");
        }
    }

    let temp_dir = std::env::temp_dir();
    let file_name = format!("{}.jpg", idempotency_key);
    let dest_path = temp_dir.join(&file_name);

    if let Err(e) = req.image.file.persist(&dest_path) {
        log::error!("Failed to persist uploaded file: {e}");
        return HttpResponse::InternalServerError().body("Failed to save uploaded file");
    }

    let state = state.into_inner();
    let dest_path_clone = dest_path.clone();

    actix_web::rt::spawn(async move {
        match parse_image(&dest_path_clone, &state.client, &state.api_key).await {
            Ok(Some(event)) => match state.events_repo.insert(&event).await {
                Ok(id) => {
                    log::info!("Saved event to database with id: {}", id);
                }
                Err(e) => {
                    log::error!("Failed to save event to database: {e:#}");
                }
            },
            Ok(None) => {
                log::info!("Image processed but no event found (or missing date)");
            }
            Err(e) => {
                log::error!("parse_image failed: {e:#}");
            }
        }

        // Cleanup
        if let Err(e) = fs::remove_file(&dest_path_clone) {
            log::warn!("Failed to remove temp file {:?}: {}", dest_path_clone, e);
        }
    });

    HttpResponse::SeeOther()
        .insert_header((actix_web::http::header::LOCATION, "/upload-success"))
        .finish()
}

pub async fn success() -> HttpResponse {
    HttpResponse::Ok()
        .content_type(ContentType::html())
        .body(format!(
            r#"<!doctype html>
        <html lang="en">
        <head>
            <meta name="color-scheme" content="light dark">
            <meta name="viewport" content="width=device-width, minimum-scale=1, initial-scale=1">
            <title>Upload Successful - Somerville Events</title>
            <style>
                {common_styles}
            </style>
        </head>
        <body>
            <h1>Upload Successful!</h1>
            <p>Your photo has been uploaded and is being processed in the background.</p>
            <p>Please check the events page in a few moments to see your event.</p>
            <br>
            <a href="/" class="button primary">Back to Events</a>
        </body>
        </html>"#,
            common_styles = COMMON_STYLES
        ))
}

pub async fn parse_image(
    image_path: &Path,
    client: &Client,
    api_key: &str,
) -> Result<Option<Event>> {
    parse_image_with_now(image_path, client, api_key, Utc::now()).await
}

pub async fn parse_image_with_now(
    image_path: &Path,
    client: &Client,
    api_key: &str,
    now: DateTime<Utc>,
) -> Result<Option<Event>> {
    // Read file
    let bytes =
        fs::read(image_path).map_err(|e| anyhow!("Failed to read file: {image_path:?} - {e}"))?;

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
                    - Focus on extracting event-related information like the name, date, time, location, and description.
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
    let event = parse_and_validate_response(&content)?;
    Ok(event)
}

fn parse_and_validate_response(content: &str) -> Result<Option<Event>> {
    let extraction: ImageEventExtraction = match serde_json::from_str(content) {
        Ok(e) => e,
        Err(_e) => {
            // If serde_json::from_str::<ImageEventExtraction>(content) fails, the error message
            // can sometimes be ambiguous about whether the input was garbage text or just a
            // mismatched field. This block is mostly useful for getting slightly more granular error
            // messages for debugging.
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
        confidence: extraction.confidence,
        id: None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use awc::Connector;
    use chrono::TimeZone;
    use dotenvy::dotenv;
    use std::env;
    use std::sync::{Arc, OnceLock};

    static TLS_CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();

    fn init_tls_once() -> Arc<rustls::ClientConfig> {
        use rustls_platform_verifier::ConfigVerifierExt as _;

        // Ignore error if already installed (e.g. by another test)
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

        let client_config = rustls::ClientConfig::with_platform_verifier()
            .expect("Failed to create TLS client config.");
        Arc::new(client_config)
    }

    #[actix_web::test]
    async fn test_parse_image() -> Result<()> {
        dotenv().ok();
        let api_key = env::var("OPENAI_API_KEY")?;
        let tls_config = TLS_CONFIG.get_or_init(init_tls_once).clone();

        let client: Client = awc::ClientBuilder::new()
            .timeout(std::time::Duration::from_secs(120))
            .connector(Connector::new().rustls_0_23(tls_config))
            .finish();

        let fixed_now_utc = Utc.with_ymd_and_hms(2025, 1, 15, 17, 0, 0).unwrap();
        let event_opt = parse_image_with_now(
            Path::new("examples/dance_flyer.jpg"),
            &client,
            &api_key,
            fixed_now_utc,
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
            Utc.with_ymd_and_hms(2025, 6, 23, 0, 0, 0).unwrap()
        );

        Ok(())
    }

    #[actix_web::test]
    async fn test_parse_not_an_event_selfie() -> Result<()> {
        dotenv().ok();
        let api_key = env::var("OPENAI_API_KEY")?;
        let tls_config = TLS_CONFIG.get_or_init(init_tls_once).clone();

        let client: Client = awc::ClientBuilder::new()
            .timeout(std::time::Duration::from_secs(120))
            .connector(Connector::new().rustls_0_23(tls_config))
            .finish();

        let fixed_now_utc = Utc.with_ymd_and_hms(2025, 1, 15, 17, 0, 0).unwrap();

        // This image should NOT be parsed as an event
        let event_opt = parse_image_with_now(
            Path::new("examples/selfie.jpg"),
            &client,
            &api_key,
            fixed_now_utc,
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
        dotenv().ok();
        let api_key = env::var("OPENAI_API_KEY")?;
        let tls_config = TLS_CONFIG.get_or_init(init_tls_once).clone();

        let client: Client = awc::ClientBuilder::new()
            .timeout(std::time::Duration::from_secs(120))
            .connector(Connector::new().rustls_0_23(tls_config))
            .finish();

        let fixed_now_utc = Utc.with_ymd_and_hms(2025, 1, 15, 17, 0, 0).unwrap();

        // This image should NOT be parsed as an event
        let event_opt = parse_image_with_now(
            Path::new("examples/soda_ad.jpg"),
            &client,
            &api_key,
            fixed_now_utc,
        )
        .await?;

        assert!(
            event_opt.is_none(),
            "Expected None for soda_ad.jpg, but got {:?}",
            event_opt
        );

        Ok(())
    }
}

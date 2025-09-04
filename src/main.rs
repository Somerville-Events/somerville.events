use actix_multipart::form::tempfile::TempFile;
use actix_multipart::form::MultipartForm;
use actix_web::{
    dev::ServiceRequest,
    error::ErrorUnauthorized,
    http::header::ContentType,
    middleware,
    web::{self, Data},
    App, Error, HttpResponse, HttpServer,
};
use actix_web_httpauth::{extractors::basic::BasicAuth, middleware::HttpAuthentication};
use anyhow::{anyhow, Result};
use awc::{Client, Connector};
use base64::{engine::general_purpose::STANDARD as b64, Engine as _};
use dotenvy::dotenv;
use rustls;
use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    env, fs,
    path::Path,
    sync::{Arc, OnceLock},
};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct Event {
    /// The name of the event
    name: String,
    /// The full description of the event or content
    full_description: String,
    /// The date and time of the event (ISO 8601 format: YYYY-MM-DDTHH:MM:SSZ)
    start_date: Option<Vec<String>>,
    /// The end date of the event (ISO 8601 format: YYYY-MM-DDTHH:MM:SSZ)
    end_date: Option<Vec<String>>,
    /// The location of the event
    location: Option<String>,
    /// Type of event (e.g., "YardSale", "Art", "Dance", "Performance", "Food", "PersonalService", "CivicEvent", "Other")
    event_type: Option<String>,
    /// Any additional relevant details
    additional_details: Option<Vec<String>>,
    /// Confidence level of the extraction (0.0 to 1.0)
    confidence: f64,
}

#[derive(Debug, MultipartForm)]
struct Upload {
    image: TempFile,
}

#[derive(Clone)]
struct AppState {
    api_key: String,
    client: Client,
    username: String,
    password: String,
}

static TLS_CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();

fn init_tls_once() -> Arc<rustls::ClientConfig> {
    use rustls_platform_verifier::ConfigVerifierExt as _;

    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .unwrap();

    // The benefits of the platform verifier are clear; see:
    // https://github.com/rustls/rustls-platform-verifier#readme
    let client_config = rustls::ClientConfig::with_platform_verifier()
        .expect("Failed to create TLS client config.");
    Arc::new(client_config)
}

async fn basic_auth_validator(
    req: ServiceRequest,
    credentials: BasicAuth,
) -> Result<ServiceRequest, (Error, ServiceRequest)> {
    let state = req
        .app_data::<Data<AppState>>()
        .expect("AppState missing; did you register .app_data(Data::new(AppState{...}))?");

    let username = credentials.user_id();
    let password = credentials.password().unwrap_or_default();

    if username == state.username && password == state.password {
        Ok(req)
    } else {
        Err((ErrorUnauthorized("Invalid credentials"), req))
    }
}

#[actix_web::main]
async fn main() -> Result<()> {
    // Load .env file if present
    dotenv().ok();
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    // Read env once
    let api_key: String = env::var("OPENAI_API_KEY").expect("Set env var: OPENAI_API_KEY");
    let username = env::var("BASIC_AUTH_USER").expect("Set env var: BASIC_AUTH_USER");
    let password = env::var("BASIC_AUTH_PASS").expect("Set env var: BASIC_AUTH_PASS");

    // TLS config once
    let tls_config = TLS_CONFIG.get_or_init(init_tls_once).clone();

    log::info!("Starting server at http://localhost:8080");
    HttpServer::new(move || {
        // Build a single client per worker with shared TLS config
        let client: Client = awc::ClientBuilder::new()
            .timeout(std::time::Duration::from_secs(120))
            .connector(Connector::new().rustls_0_23(tls_config.clone()))
            .finish();

        let state = AppState {
            api_key: api_key.clone(),
            username: username.clone(),
            password: password.clone(),
            client,
        };

        let auth_middleware = HttpAuthentication::basic(basic_auth_validator);

        App::new()
            .app_data(Data::new(state))
            .wrap(middleware::Logger::default())
            .route("/", web::get().to(index))
            .service(
                web::resource("/upload")
                    .wrap(auth_middleware)
                    .route(web::get().to(upload_ui))
                    .route(web::post().to(upload)),
            )
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await?;
    Ok(())
}

async fn index() -> HttpResponse {
    HttpResponse::Ok().content_type(ContentType::html()).body(
        r#"<!doctype html>
        <html lang="en">
        <meta name="color-scheme" content="light dark">
        <meta name="viewport" content="width=device-width, minimum-scale=1, initial-scale=1">
        <title>Somerville Events</title>
        <h1>Somerville Events</h1>"#,
    )
}

async fn upload_ui() -> HttpResponse {
    HttpResponse::Ok().content_type(ContentType::html()).body(
        r#"<!doctype html>
        <html lang="en">
        <meta name="color-scheme" content="light dark">
        <meta name="viewport" content="width=device-width, minimum-scale=1, initial-scale=1">
        <title>Somerville Events Upload</title>
        <style>
        html, body {
            height: 100%;
            margin: 0;
        }
        form {
            display: flex;
            flex-direction: column;
            height: 100%;
        }
        input[type=file] {
            flex: 1;
            border: none;
        }
        button {
            flex: 0;
        }
        </style>

        <form action="/upload" method="post" enctype="multipart/form-data">
        <input type="file" name="image" accept="image/*" required>
        <button>Upload</button>
        </form>
        "#,
    )
}

async fn upload(state: Data<AppState>, MultipartForm(req): MultipartForm<Upload>) -> HttpResponse {
    match parse_image(req.image.file.path(), &state.client, &state.api_key).await {
        Ok(event) => HttpResponse::Ok().json(event),
        Err(e) => {
            log::error!("parse_image failed: {e:#}");
            HttpResponse::InternalServerError().body("Parsing failed")
        }
    }
}

async fn parse_image(image_path: &Path, client: &Client, api_key: &str) -> Result<Event> {
    // Read file
    let bytes =
        fs::read(image_path).map_err(|e| anyhow!("Failed to read file: {image_path:?} - {e}"))?;

    // Guess MIME type
    let mime_type = mime_guess::from_path(&image_path)
        .first_raw()
        .map(|m| m.to_string())
        .unwrap_or_else(|| "image/png".to_string());

    // Base64 encode -> data URL
    let b64_data = b64.encode(&bytes);
    let data_url = format!("data:{mime_type};base64,{b64_data}");

    let schema = schema_for!(Event);
    let schema_str = serde_json::to_string_pretty(&schema).unwrap();
    log::debug!("{schema_str}");

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
                    {schema:#?}
                    The text field should contain all readable text from the image. 
                    The confidence should be a number between 0.0 and 1.0 indicating how confident you are in the extraction. 
                    Focus on extracting event-related information like the name, date, time, location, and description.
                    Never return multiple locations.
                    Never return multiple event types.
                    Be thorough but accurate. Return only valid JSON.
                    Do not return the schema in your response.
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

    log::debug!("Debug: Extracted content: {}", content);
    // Parse and validate the structured response
    let event = parse_and_validate_response(&content)?;
    Ok(event)
}

fn parse_and_validate_response(content: &str) -> Result<Event> {
    // First try to parse as the exact struct
    if let Ok(result) = serde_json::from_str::<Event>(content) {
        return Ok(result);
    }

    // If that fails, parse as generic JSON and clean it up
    let mut json: serde_json::Value =
        serde_json::from_str(content).map_err(|e| anyhow!("Failed to parse JSON: {}", e))?;

    // Remove any extra fields not in our struct
    let allowed_fields = [
        "name",
        "full_description",
        "start_date",
        "end_date",
        "location",
        "event_type",
        "additional_details",
        "confidence",
    ];

    if let Some(obj) = json.as_object_mut() {
        // Keep only allowed fields
        let keys: Vec<String> = obj.keys().cloned().collect();
        for key in keys {
            if !allowed_fields.contains(&key.as_str()) {
                obj.remove(&key);
            }
        }

        // Convert lists to single values where appropriate
        if let Some(location) = obj.get("location") {
            if location.is_array() {
                if let Some(arr) = location.as_array() {
                    if !arr.is_empty() {
                        obj.insert("location".to_string(), arr[0].clone());
                    }
                }
            }
        }

        if let Some(event_type) = obj.get("event_type") {
            if event_type.is_array() {
                if let Some(arr) = event_type.as_array() {
                    if !arr.is_empty() {
                        obj.insert("event_type".to_string(), arr[0].clone());
                    }
                }
            }
        }
    }

    // Now try to parse the cleaned JSON
    serde_json::from_value(json).map_err(|e| anyhow!("Failed to parse cleaned response: {}", e))
}

#[actix_web::test]
async fn test_parse_image() -> Result<()> {
    // Load .env file if present
    dotenv().ok();
    let api_key = env::var("OPENAI_API_KEY")?;

    // Ensure TLS is initialized for the test too
    let tls_config = TLS_CONFIG.get_or_init(init_tls_once).clone();

    // Build a client for the test context
    let client: Client = awc::ClientBuilder::new()
        .timeout(std::time::Duration::from_secs(120))
        .connector(Connector::new().rustls_0_23(tls_config))
        .finish();

    let state = AppState {
        api_key,
        client,
        password: "password".to_string(),
        username: "username".to_string(),
    };

    // Actix runtime entrypoint
    let event = parse_image(
        Path::new("examples/dance_flyer.jpg"),
        &state.client,
        &state.api_key,
    )
    .await?;
    assert_eq!(event.name, "Dance Therapy");
    Ok(())
}

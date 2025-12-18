use actix_files::Files;
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
use chrono::{DateTime, Utc};
use dotenvy::dotenv;
use icalendar::{Calendar, Component, Event as IcalEvent, EventLike};
use rustls;
use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use std::{
    env, fs,
    path::Path,
    sync::{Arc, OnceLock},
};

#[derive(Debug, Serialize, Deserialize, JsonSchema, PartialEq, Clone)]
struct Event {
    /// The name of the event
    name: String,
    /// The full description of the event or content
    full_description: String,
    /// The date and time of the event
    start_date: Option<DateTime<Utc>>,
    /// The end date of the event
    end_date: Option<DateTime<Utc>>,
    /// The location of the event
    location: Option<String>,
    /// Type of event (e.g., "YardSale", "Art", "Dance", "Performance", "Food", "PersonalService", "CivicEvent", "Other")
    event_type: Option<String>,
    /// Any additional relevant details
    additional_details: Option<Vec<String>>,
    /// Confidence level of the extraction (0.0 to 1.0)
    confidence: f64,
    /// Path to the event flyer image
    // schemars(skip): Do not include this field in the JSON schema sent to the LLM.
    // The LLM should not attempt to extract or invent a file path.
    #[schemars(skip)]
    // serde(default): When parsing the LLM's JSON response (which won't have this field),
    // automatically fill it with the default value (String::new() i.e., "") instead of erroring.
    #[serde(default)]
    image_path: String,
    /// Database ID (optional)
    #[serde(skip, default)]
    #[schemars(skip)]
    id: Option<i64>,
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
    db_connection_pool: sqlx::Pool<sqlx::Postgres>,
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
    let api_key: String = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY");
    let username = env::var("BASIC_AUTH_USER").expect("BASIC_AUTH_USER");
    let password = env::var("BASIC_AUTH_PASS").expect("BASIC_AUTH_PASS");
    let db_user = env::var("DB_APP_USER").expect("DB_APP_USER");
    let db_password = env::var("DB_APP_USER_PASS").expect("DB_APP_USER_PASS");
    let db_name = env::var("DB_NAME").expect("DB_NAME");

    // TLS config once
    let tls_config = TLS_CONFIG.get_or_init(init_tls_once).clone();

    // Create the database connection pool once
    let db_url = format!("postgres://{db_user}:{db_password}@localhost/{db_name}");
    let db_connection_pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(db_url.as_str())
        .await?;

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
            db_connection_pool: db_connection_pool.clone(),
            client,
        };

        let auth_middleware = HttpAuthentication::basic(basic_auth_validator);

        App::new()
            .app_data(Data::new(state))
            .wrap(middleware::Logger::default())
            .route("/", web::get().to(index))
            .route("/event/{id}.html", web::get().to(event_details))
            .route("/event/{id}.ical", web::get().to(event_ical))
            .service(Files::new("/static", "./static").show_files_listing())
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

async fn index(state: Data<AppState>) -> HttpResponse {
    let events = sqlx::query_as!(
        Event,
        r#"
        SELECT
            id,
            name,
            full_description,
            start_date,
            end_date,
            location,
            event_type,
            additional_details,
            confidence,
            image_path
        FROM app.events
        ORDER BY start_date ASC NULLS LAST
        "#
    )
    .fetch_all(&state.db_connection_pool)
    .await;

    match events {
        Ok(events) => {
            let mut events_html = String::new();
            for event in events {
                events_html.push_str(&format!(
                    r#"
                    <div class="event">
                        <h2><a href="/event/{}.html">{}</a></h2>
                        <p><strong>Date:</strong> {}</p>
                        <p><strong>Location:</strong> {}</p>
                        <p>{}</p>
                        {}
                        <p><a href="/event/{}.ical">📅 Add to Calendar</a></p>
                    </div>
                    <hr>
                    "#,
                    // Use unwrap_or_default for ID, though it should exist from DB
                    event.id.unwrap_or_default(),
                    html_escape::encode_text(&event.name),
                    event
                        .start_date
                        .map(|d| d.format("%A, %B %d, %Y at %I:%M %p").to_string())
                        .unwrap_or_else(|| "TBD".to_string()),
                    html_escape::encode_text(&event.location.unwrap_or_default()),
                    html_escape::encode_text(&event.full_description),
                    format!(
                        r#"<a href="{}"><img src="{}" alt="Event Flyer" style="max-width: 100%; max-height: 300px; display: block; margin: 1rem 0;"></a>"#,
                        event.image_path, event.image_path
                    ),
                    event.id.unwrap_or_default()
                ));
            }

            HttpResponse::Ok().content_type(ContentType::html()).body(format!(
                r#"<!doctype html>
                <html lang="en">
                <head>
                    <meta name="color-scheme" content="light dark">
                    <meta name="viewport" content="width=device-width, minimum-scale=1, initial-scale=1">
                    <title>Somerville Events</title>
                    <style>
                        body {{ font-family: system-ui, sans-serif; max-width: 800px; margin: 0 auto; padding: 1rem; line-height: 1.5; }}
                        .event {{ margin-bottom: 2rem; }}
                        h1 {{ margin-bottom: 1rem; }}
                        a {{ display: inline-block; margin-bottom: 2rem; }}
                    </style>
                </head>
                <body>
                    <h1>Somerville Events</h1>
                    <a href="/upload">Upload New Event</a>
                    <hr>
                    {}
                </body>
                </html>"#,
                events_html
            ))
        }
        Err(e) => {
            log::error!("Failed to fetch events: {e}");
            HttpResponse::InternalServerError().body("Failed to fetch events")
        }
    }
}

async fn event_details(state: Data<AppState>, path: web::Path<i64>) -> HttpResponse {
    let id = path.into_inner();
    let event = sqlx::query_as!(
        Event,
        r#"
        SELECT
            id,
            name,
            full_description,
            start_date,
            end_date,
            location,
            event_type,
            additional_details,
            confidence,
            image_path
        FROM app.events
        WHERE id = $1
        "#,
        id
    )
    .fetch_optional(&state.db_connection_pool)
    .await;

    match event {
        Ok(Some(event)) => {
            HttpResponse::Ok().content_type(ContentType::html()).body(format!(
                r#"<!doctype html>
                <html lang="en">
                <head>
                    <meta name="color-scheme" content="light dark">
                    <meta name="viewport" content="width=device-width, minimum-scale=1, initial-scale=1">
                    <title>{} - Somerville Events</title>
                    <style>
                        body {{ font-family: system-ui, sans-serif; max-width: 800px; margin: 0 auto; padding: 1rem; line-height: 1.5; }}
                        a {{ display: inline-block; margin-bottom: 2rem; }}
                        .ical-link {{ margin-left: 1rem; }}
                    </style>
                </head>
                <body>
                    <a href="/">&larr; Back to Events</a>
                    <h1>{}</h1>
                    <p><strong>Date:</strong> {}</p>
                    <p><strong>Location:</strong> {}</p>
                    <p>{}</p>
                    {}
                    <p>
                        <a href="/event/{}.ical" class="ical-link">Add to Calendar (.ics)</a>
                    </p>
                </body>
                </html>"#,
                html_escape::encode_text(&event.name),
                html_escape::encode_text(&event.name),
                event
                    .start_date
                    .map(|d| d.format("%A, %B %d, %Y at %I:%M %p").to_string())
                    .unwrap_or_else(|| "TBD".to_string()),
                html_escape::encode_text(&event.location.unwrap_or_default()),
                html_escape::encode_text(&event.full_description),
                format!(
                    r#"<a href="{}"><img src="{}" alt="Event Flyer" style="max-width: 100%; display: block; margin: 2rem 0;"></a>"#,
                    event.image_path, event.image_path
                ),
                id
            ))
        }
        Ok(None) => HttpResponse::NotFound().body("Event not found"),
        Err(e) => {
            log::error!("Failed to fetch event: {e}");
            HttpResponse::InternalServerError().body("Failed to fetch event")
        }
    }
}

async fn event_ical(state: Data<AppState>, path: web::Path<i64>) -> HttpResponse {
    let id = path.into_inner();
    let event_res = sqlx::query_as!(
        Event,
        r#"
        SELECT
            id,
            name,
            full_description,
            start_date,
            end_date,
            location,
            event_type,
            additional_details,
            confidence,
            image_path
        FROM app.events
        WHERE id = $1
        "#,
        id
    )
    .fetch_optional(&state.db_connection_pool)
    .await;

    match event_res {
        Ok(Some(event)) => {
            let mut ical_event = IcalEvent::new();
            ical_event
                .summary(&event.name)
                .description(&event.full_description);

            if let Some(location) = event.location {
                ical_event.location(&location);
            }

            if let Some(start) = event.start_date {
                ical_event.starts(start);
                if let Some(end) = event.end_date {
                    ical_event.ends(end);
                } else {
                    // Default to 1 hour duration if no end date
                    ical_event.ends(start + chrono::Duration::hours(1));
                }
            }

            let calendar = Calendar::new().push(ical_event).done();

            HttpResponse::Ok()
                .content_type("text/calendar")
                .insert_header((
                    "Content-Disposition",
                    format!("attachment; filename=\"event-{}.ics\"", id),
                ))
                .body(calendar.to_string())
        }
        Ok(None) => HttpResponse::NotFound().body("Event not found"),
        Err(e) => {
            log::error!("Failed to fetch event: {e}");
            HttpResponse::InternalServerError().body("Failed to fetch event")
        }
    }
}

async fn upload_ui() -> HttpResponse {
    HttpResponse::Ok().content_type(ContentType::html()).body(
        r#"<!doctype html>
        <html lang="en">
        <head>
            <meta name="color-scheme" content="light dark">
            <meta name="viewport" content="width=device-width, minimum-scale=1, initial-scale=1">
            <title>Somerville Events Upload</title>
            <style>
                body {
                    font-family: system-ui, sans-serif;
                    max-width: 800px;
                    margin: 0 auto;
                    padding: 1rem;
                    line-height: 1.5;
                }
                form {
                    display: flex;
                    flex-direction: column;
                    gap: 1rem;
                    border: 1px solid #ccc;
                    padding: 2rem;
                    border-radius: 8px;
                }
                input[type=file] {
                    font-size: 1rem;
                }
                button {
                    padding: 0.75rem;
                    font-size: 1rem;
                    background-color: #007bff;
                    color: white;
                    border: none;
                    border-radius: 4px;
                    cursor: pointer;
                }
                button:hover {
                    background-color: #0056b3;
                }
                a {
                    display: inline-block;
                    margin-bottom: 1rem;
                }
            </style>
        </head>
        <body>
            <a href="/">&larr; Back to Events</a>
            <h1>Upload Event Flyer</h1>
            <p>Upload an image of a flyer or event poster. We'll extract the details automatically.</p>
            
            <form action="/upload" method="post" enctype="multipart/form-data">
                <label for="image">Take Photo or Choose File</label>
                <input type="file" id="image" name="image" accept="image/*" capture="environment" required>
                <button type="submit">Upload and Process</button>
            </form>
        </body>
        </html>"#,
    )
}

async fn upload(state: Data<AppState>, MultipartForm(req): MultipartForm<Upload>) -> HttpResponse {
    let temp_path = req.image.file.path().to_owned();
    match parse_image(&temp_path, &state.client, &state.api_key).await {
        Ok(mut event) => {
            // Generate a unique filename
            let extension = req
                .image
                .file_name
                .as_ref()
                .and_then(|n| Path::new(n).extension())
                .and_then(|e| e.to_str())
                .unwrap_or("jpg");
            let new_filename = format!("{}.{}", Utc::now().timestamp_micros(), extension);
            let new_path = Path::new("static/images").join(&new_filename);

            // Save the file
            if let Err(e) = fs::copy(&temp_path, &new_path) {
                log::error!("Failed to save image: {}", e);
                return HttpResponse::InternalServerError().body("Failed to save image");
            }

            event.image_path = format!("/static/images/{}", new_filename);

            match save_event_to_db(&state.db_connection_pool, &event).await {
                Ok(id) => {
                    log::info!("Saved event to database with id: {}", id);
                    // Update the returned event with the ID
                    let mut returned_event = event.clone();
                    returned_event.id = Some(id);
                    HttpResponse::Ok().json(returned_event)
                }
                Err(e) => {
                    log::error!("Failed to save event to database: {e:#}");
                    HttpResponse::InternalServerError().body("Failed to save event to database")
                }
            }
        }
        Err(e) => {
            log::error!("parse_image failed: {e:#}");
            HttpResponse::InternalServerError().body("Parsing failed")
        }
    }
}

async fn save_event_to_db<'e, E>(executor: E, event: &Event) -> Result<i64>
where
    E: sqlx::PgExecutor<'e>,
{
    let id = sqlx::query_scalar!(
        r#"
        INSERT INTO app.events (
            name,
            full_description,
            start_date,
            end_date,
            location,
            event_type,
            additional_details,
            confidence,
            image_path
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        RETURNING id
        "#,
        event.name,
        event.full_description,
        event.start_date,
        event.end_date,
        event.location,
        event.event_type,
        event.additional_details.as_deref(),
        event.confidence,
        event.image_path
    )
    .fetch_one(executor)
    .await
    .map_err(|e| anyhow!("Database insert failed: {e}"))?;

    Ok(id)
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
    let now = Utc::now();
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
                    The text field should contain all readable text from the image.
                    The confidence should be a number between 0.0 and 1.0 indicating how confident you are in the extraction.
                    Focus on extracting event-related information like the name, date, time, location, and description.
                    Never return multiple locations.
                    Never return multiple event types.
                    Today's date is {now_str}.
                    The start_date and end_date must be RFC 3339 formatted date and time strings.
                    Assume the event is in the future unless the text clearly indicates it is in the past.
                    Assume the event is in the timezone of the location if provided.
                    Assume the event is nearest to today's date if the date is ambiguous in any way.
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

    // Create the database connection pool for testing
    let db_user = env::var("DB_APP_USER").expect("DB_APP_USER");
    let db_password = env::var("DB_APP_USER_PASS").expect("DB_APP_USER_PASS");
    let db_name = env::var("DB_NAME").expect("DB_NAME");
    let db_url = format!("postgres://{db_user}:{db_password}@localhost/{db_name}");

    let db_connection_pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await?;

    let state = AppState {
        api_key,
        client,
        password: "password".to_string(),
        username: "username".to_string(),
        db_connection_pool,
    };

    // Actix runtime entrypoint
    let event = parse_image(
        Path::new("examples/dance_flyer.jpg"),
        &state.client,
        &state.api_key,
    )
    .await?;
    assert_eq!(event.name, "Dance Therapy");

    // Test saving to the database using a transaction
    let mut tx = state.db_connection_pool.begin().await?;
    let id = save_event_to_db(&mut *tx, &event).await?;
    log::info!("Test saved event with id: {}", id);

    // Verify the save worked
    let saved_event = sqlx::query_as!(
        Event,
        r#"
        SELECT
            id,
            name,
            full_description,
            start_date,
            end_date,
            location,
            event_type,
            additional_details,
            confidence,
            image_path
        FROM app.events
        WHERE id = $1
        "#,
        id
    )
    .fetch_one(&mut *tx)
    .await?;

    let mut expected_event = event.clone();
    expected_event.id = Some(id);
    assert_eq!(saved_event, expected_event);

    // Rollback the transaction so we don't pollute the database
    tx.rollback().await?;

    Ok(())
}

#[actix_web::test]
async fn test_index() -> Result<()> {
    dotenv().ok();

    let db_user = env::var("DB_APP_USER").expect("DB_APP_USER");
    let db_password = env::var("DB_APP_USER_PASS").expect("DB_APP_USER_PASS");
    let db_name = env::var("DB_NAME").expect("DB_NAME");
    let db_url = format!("postgres://{db_user}:{db_password}@localhost/{db_name}");

    let db_connection_pool = PgPoolOptions::new().connect(&db_url).await?;

    // Dummy client since index doesn't use it
    let client = awc::Client::default();

    let state = AppState {
        api_key: "dummy".to_string(),
        client,
        username: "user".to_string(),
        password: "pass".to_string(),
        db_connection_pool,
    };

    let resp = index(Data::new(state)).await;
    assert_eq!(resp.status(), actix_web::http::StatusCode::OK);

    use actix_web::body::to_bytes;
    let body = to_bytes(resp.into_body())
        .await
        .map_err(|e| anyhow!("Error reading body: {}", e))?;
    let body_str = std::str::from_utf8(&body)?;
    assert!(body_str.contains("Somerville Events"));

    Ok(())
}

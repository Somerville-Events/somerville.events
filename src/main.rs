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
use async_trait::async_trait;
use awc::{Client, Connector};
use base64::{engine::general_purpose::STANDARD as b64, Engine as _};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use chrono_tz::America::New_York;
use dotenvy::dotenv;
use icalendar::{Calendar, CalendarDateTime, Component, Event as IcalEvent, EventLike};
use rustls;
use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use std::{
    collections::BTreeMap,
    env, fs,
    path::Path,
    sync::{Arc, OnceLock},
};

fn format_datetime_in_somerville_tz(dt: DateTime<Utc>) -> String {
    // Somerville, MA observes DST, so we use a real TZ database instead of a fixed offset.
    dt.with_timezone(&New_York)
        .format("%A, %B %d, %Y at %I:%M %p")
        .to_string()
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, PartialEq, Clone, sqlx::FromRow)]
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
    /// Database ID (optional)
    #[serde(skip, default)]
    #[schemars(skip)]
    id: Option<i64>,
}

#[derive(Debug, MultipartForm)]
struct Upload {
    image: TempFile,
    idempotency_key: actix_multipart::form::text::Text<uuid::Uuid>,
}

struct AppState {
    api_key: String,
    client: Client,
    username: String,
    password: String,
    events_repo: Box<dyn EventsRepo>,
}

static TLS_CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();

#[async_trait]
trait EventsRepo: Send + Sync {
    async fn list(&self) -> Result<Vec<Event>>;
    async fn get(&self, id: i64) -> Result<Option<Event>>;
    async fn claim_idempotency_key(&self, idempotency_key: uuid::Uuid) -> Result<bool>;
    async fn insert(&self, event: &Event) -> Result<i64>;
}

struct EventsDatabase {
    pool: sqlx::Pool<sqlx::Postgres>,
}

#[async_trait]
impl EventsRepo for EventsDatabase {
    async fn list(&self) -> Result<Vec<Event>> {
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
                confidence
            FROM app.events
            ORDER BY start_date ASC NULLS LAST
            "#
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(events)
    }

    async fn get(&self, id: i64) -> Result<Option<Event>> {
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
                confidence
            FROM app.events
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(event)
    }

    async fn claim_idempotency_key(&self, idempotency_key: uuid::Uuid) -> Result<bool> {
        let insert_result = sqlx::query!(
            r#"
            INSERT INTO app.idempotency_keys (idempotency_key)
            VALUES ($1)
            ON CONFLICT DO NOTHING
            RETURNING idempotency_key
            "#,
            idempotency_key
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(insert_result.is_some())
    }

    async fn insert(&self, event: &Event) -> Result<i64> {
        save_event_to_db(&self.pool, event).await
    }
}

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
    let host = env::var("HOST").expect("HOST");
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
            events_repo: Box::new(EventsDatabase {
                pool: db_connection_pool.clone(),
            }),
            client,
        };

        let auth_middleware = HttpAuthentication::basic(basic_auth_validator);

        App::new()
            .app_data(Data::new(state))
            .wrap(middleware::Logger::default())
            .route("/", web::get().to(index))
            .route("/event/{id}.html", web::get().to(event_details))
            .route("/event/{id}.ical", web::get().to(event_ical))
            .service(
                web::resource("/upload")
                    .wrap(auth_middleware)
                    .route(web::get().to(upload_ui))
                    .route(web::post().to(upload)),
            )
            .route("/upload-success", web::get().to(upload_success))
    })
    .bind((host, 8080))?
    .run()
    .await?;
    Ok(())
}

async fn index(state: Data<AppState>) -> HttpResponse {
    index_with_now(state, Utc::now()).await
}

async fn index_with_now(state: Data<AppState>, now_utc: DateTime<Utc>) -> HttpResponse {
    let events = state.events_repo.list().await;

    match events {
        Ok(events) => {
            // We normally hide anything too far in the past, but we allow a small look-back
            // window so "no end date" events from yesterday still show up.
            let earliest_day_to_render: NaiveDate = (now_utc - Duration::days(1))
                .with_timezone(&New_York)
                .date_naive();

            let mut events_by_day: BTreeMap<NaiveDate, Vec<Event>> = BTreeMap::new();

            for event in events {
                // If we don't have a start date, we can't show it on a calendar-like "by day" view.
                let Some(start) = event.start_date else {
                    continue;
                };

                let start_day = start.with_timezone(&New_York).date_naive();
                let (end_day, visibility_end) = match event.end_date {
                    // Events without an end date render only once (on their start day), but they
                    // should remain visible for up to 24h after start (so "yesterday" can show).
                    None => (start_day, start + Duration::days(1)),
                    Some(end) => (end.with_timezone(&New_York).date_naive(), end),
                };

                // "End time is in the past" is our most reliable signal. For missing end dates,
                // we approximate an end for visibility only (see above).
                if visibility_end < now_utc {
                    continue;
                }

                let (mut day, last_day) = if start_day <= end_day {
                    (start_day, end_day)
                } else {
                    (end_day, start_day)
                };

                loop {
                    if day >= earliest_day_to_render {
                        // Show spanning events multiple times: once per day they touch.
                        events_by_day.entry(day).or_default().push(event.clone());
                    }

                    if day == last_day {
                        break;
                    }
                    day = day.succ_opt().expect("date overflow");
                }
            }

            for day_events in events_by_day.values_mut() {
                day_events.sort_by(|a, b| {
                    a.start_date
                        .cmp(&b.start_date)
                        .then_with(|| a.name.cmp(&b.name))
                });
            }

            let mut events_html = String::new();

            for (day, day_events) in events_by_day {
                let day_id = format!("day-{}", day.format("%Y-%m-%d"));
                events_html.push_str(&format!(
                    r#"<section class="day" aria-labelledby="{day_id}">
                        <h2 id="{day_id}">{}</h2>"#,
                    day.format("%A, %B %d, %Y")
                ));

                for event in day_events {
                    let when = match (event.start_date, event.end_date) {
                        (Some(start), Some(end)) => format!(
                            "{} – {}",
                            format_datetime_in_somerville_tz(start),
                            format_datetime_in_somerville_tz(end)
                        ),
                        (Some(start), None) => format_datetime_in_somerville_tz(start),
                        (None, Some(end)) => format_datetime_in_somerville_tz(end),
                        (None, None) => "TBD".to_string(),
                    };

                    let when_html = match (event.start_date, event.end_date) {
                        (Some(start), Some(end)) => format!(
                            r#"<time datetime="{start_dt}">{start_label}</time> – <time datetime="{end_dt}">{end_label}</time>"#,
                            start_dt = html_escape::encode_double_quoted_attribute(
                                &start.with_timezone(&New_York).to_rfc3339()
                            ),
                            start_label =
                                html_escape::encode_text(&format_datetime_in_somerville_tz(start)),
                            end_dt = html_escape::encode_double_quoted_attribute(
                                &end.with_timezone(&New_York).to_rfc3339()
                            ),
                            end_label =
                                html_escape::encode_text(&format_datetime_in_somerville_tz(end)),
                        ),
                        (Some(start), None) => format!(
                            r#"<time datetime="{start_dt}">{start_label}</time>"#,
                            start_dt = html_escape::encode_double_quoted_attribute(
                                &start.with_timezone(&New_York).to_rfc3339()
                            ),
                            start_label =
                                html_escape::encode_text(&format_datetime_in_somerville_tz(start)),
                        ),
                        _ => html_escape::encode_text(&when).to_string(),
                    };

                    events_html.push_str(&format!(
                        r#"
                        <article class="event">
                            <header>
                                <h3><a href="/event/{id}.html">{name}</a></h3>
                            </header>
                            <dl class="event-meta">
                                <dt>When</dt>
                                <dd>{when_html}</dd>
                                <dt>Location</dt>
                                <dd>{location}</dd>
                            </dl>
                            <p class="event-description">{description}</p>
                            <p class="event-actions"><a href="/event/{id}.ical">Add to calendar (.ics)</a></p>
                        </article>
                        "#,
                        id = event.id.unwrap_or_default(),
                        name = html_escape::encode_text(&event.name),
                        when_html = when_html,
                        location = html_escape::encode_text(&event.location.unwrap_or_default()),
                        description = html_escape::encode_text(&event.full_description),
                    ));
                }

                events_html.push_str("</section>");
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
                        h1 {{ margin-bottom: 1rem; }}
                        h2 {{ margin-top: 2.5rem; }}
                        header.site-header {{ display: flex; align-items: baseline; justify-content: space-between; gap: 1rem; }}
                        nav.site-nav a {{ display: inline-block; }}
                        section.day {{ margin-bottom: 2.5rem; }}
                        article.event {{ padding: 1rem 0; border-top: 1px solid color-mix(in srgb, currentColor 15%, transparent); }}
                        article.event:first-child {{ border-top: 0; }}
                        dl.event-meta {{ margin: 0.5rem 0 0.75rem 0; display: grid; grid-template-columns: 7rem 1fr; gap: 0.25rem 1rem; }}
                        dl.event-meta dt {{ font-weight: 600; }}
                        dl.event-meta dd {{ margin: 0; }}
                        p.event-description {{ margin: 0.75rem 0; }}
                        p.event-actions {{ margin: 0.5rem 0 0; }}
                    </style>
                </head>
                <body>
                    <header class="site-header">
                        <h1>Somerville Events</h1>
                        <nav class="site-nav" aria-label="Site">
                            <a href="/upload">Upload new event</a>
                        </nav>
                    </header>
                    <main>
                        {}
                    </main>
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
    let event = state.events_repo.get(id).await;

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
                    <p>
                        <a href="/event/{}.ical" class="ical-link">Add to Calendar (.ics)</a>
                    </p>
                </body>
                </html>"#,
                html_escape::encode_text(&event.name),
                html_escape::encode_text(&event.name),
                event
                    .start_date
                    .map(format_datetime_in_somerville_tz)
                    .unwrap_or_else(|| "TBD".to_string()),
                html_escape::encode_text(&event.location.unwrap_or_default()),
                html_escape::encode_text(&event.full_description),
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
    let event_res = state.events_repo.get(id).await;

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
                let start_et = start.with_timezone(&New_York);
                ical_event.starts(CalendarDateTime::from_date_time(start_et));
                if let Some(end) = event.end_date {
                    ical_event.ends(CalendarDateTime::from_date_time(
                        end.with_timezone(&New_York),
                    ));
                } else {
                    // Default to 1 hour duration if no end date
                    ical_event.ends(CalendarDateTime::from_date_time(
                        start_et + chrono::Duration::hours(1),
                    ));
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

async fn upload_success() -> HttpResponse {
    HttpResponse::Ok().content_type(ContentType::html()).body(
        r#"<!doctype html>
        <html lang="en">
        <head>
            <meta name="color-scheme" content="light dark">
            <meta name="viewport" content="width=device-width, minimum-scale=1, initial-scale=1">
            <title>Upload Successful - Somerville Events</title>
            <style>
                body {
                    font-family: system-ui, sans-serif;
                    max-width: 800px;
                    margin: 0 auto;
                    padding: 1rem;
                    line-height: 1.5;
                    text-align: center;
                }
                a {
                    display: inline-block;
                    margin-top: 2rem;
                    padding: 0.75rem 1.5rem;
                    background-color: #28a745;
                    color: white;
                    text-decoration: none;
                    border-radius: 6px;
                    font-weight: bold;
                }
                a:hover {
                    background-color: #218838;
                }
            </style>
        </head>
        <body>
            <h1>Upload Successful!</h1>
            <p>Your photo has been uploaded and is being processed in the background.</p>
            <p>Please check the events page in a few moments to see your event.</p>
            <a href="/">Back to Events</a>
        </body>
        </html>"#,
    )
}

async fn upload_ui() -> HttpResponse {
    let idempotency_key = uuid::Uuid::new_v4();
    HttpResponse::Ok().content_type(ContentType::html()).body(
        format!(
            r#"<!doctype html>
        <html lang="en">
        <head>
            <meta name="color-scheme" content="light dark">
            <meta name="viewport" content="width=device-width, minimum-scale=1, initial-scale=1">
            <title>Somerville Events Upload</title>
            <style>
                body {{
                    font-family: system-ui, sans-serif;
                    max-width: 800px;
                    margin: 0 auto;
                    padding: 1rem;
                    line-height: 1.5;
                }}
                form {{
                    display: flex;
                    flex-direction: column;
                    gap: 1.5rem;
                    border: 1px solid #ccc;
                    padding: 2rem;
                    border-radius: 8px;
                    align-items: center;
                }}

                /* File Input Styling */
                /* Hide the actual input but keep it accessible/validatable */
                input[type=file] {{
                    opacity: 0;
                    width: 0.1px;
                    height: 0.1px;
                    position: absolute;
                    z-index: -1;
                }}

                /* Prominent Take Photo Button (Label) */
                .file-label {{
                    display: flex;
                    align-items: center;
                    justify-content: center;
                    padding: 1.5rem;
                    font-size: 1.5rem;
                    background-color: #28a745;
                    color: white;
                    border-radius: 8px;
                    cursor: pointer;
                    width: 100%;
                    text-align: center;
                    transition: all 0.2s;
                    box-sizing: border-box;
                    border: 2px solid transparent;
                }}
                .file-label:hover {{
                    background-color: #218838;
                }}
                .file-label:active {{
                    background-color: #1e7e34;
                    transform: scale(0.98);
                }}

                /* When file is selected (valid), change label appearance */
                input[type=file]:valid + .file-label {{
                    background-color: #1e7e34;
                    border-color: #155724;
                }}
                /* Use ::after to change text content based on state is tricky without attr() support for arbitrary strings in all browsers,
                   but we can use a checkmark. */
                input[type=file]:valid + .file-label::after {{
                    content: " ✅";
                    margin-left: 0.5rem;
                }}

                /* Prominent Upload Button */
                #upload-btn {{
                    padding: 1.5rem;
                    font-size: 1.5rem;
                    background-color: #007bff;
                    color: white;
                    border: none;
                    border-radius: 8px;
                    cursor: pointer;
                    width: 100%;
                    transition: all 0.2s;
                    /* Initially hidden/disabled look if desired, or just always there */
                    opacity: 0.5;
                    pointer-events: none;
                }}
                
                /* Enable upload button when file is selected */
                input[type=file]:valid ~ #upload-btn {{
                    opacity: 1;
                    pointer-events: auto;
                    background-color: #007bff;
                }}
                
                input[type=file]:valid ~ #upload-btn:hover {{
                    background-color: #0056b3;
                }}

                /* Disabled state (e.g. during upload) overrides the valid state */
                input[type=file]:valid ~ #upload-btn:disabled {{
                    opacity: 0.8;
                    pointer-events: none;
                    cursor: wait;
                    background-color: #007bff; /* keep blue */
                }}

                /* Loading Spinner */
                .spinner {{
                    display: inline-block;
                    width: 1em;
                    height: 1em;
                    border: 3px solid rgba(255,255,255,0.3);
                    border-radius: 50%;
                    border-top-color: #fff;
                    animation: spin 1s ease-in-out infinite;
                    margin-right: 0.5rem;
                    vertical-align: middle;
                }}

                @keyframes spin {{
                    to {{ transform: rotate(360deg); }}
                }}

                a {{
                    display: inline-block;
                    margin-bottom: 1rem;
                }}
            </style>
        </head>
        <body>
            <a href="/">&larr; Back to Events</a>
            <h1>Upload Event Flyer</h1>
            <p>Upload an image of a flyer or event poster. We'll extract the details automatically.</p>
            
            <form action="/upload" method="post" enctype="multipart/form-data">
                <input type="hidden" name="idempotency_key" value="{}">
                <!-- Input must be before label/button for sibling selectors to work -->
                <input type="file" id="image" name="image" accept="image/*" capture="environment" required>
                
                <label for="image" class="file-label">
                    📸 Take Photo / Choose File
                </label>

                <button type="submit" id="upload-btn">Upload</button>
            </form>

            <script>
                document.querySelector('form').addEventListener('submit', function(e) {{
                    var btn = document.getElementById('upload-btn');
                    btn.disabled = true;
                    btn.innerHTML = '<span class="spinner"></span> Uploading...';
                }});
            </script>
        </body>
        </html>"#,
            idempotency_key
        ),
    )
}

async fn upload(state: Data<AppState>, MultipartForm(req): MultipartForm<Upload>) -> HttpResponse {
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
            Ok(event) => match state.events_repo.insert(&event).await {
                Ok(id) => {
                    log::info!("Saved event to database with id: {}", id);
                }
                Err(e) => {
                    log::error!("Failed to save event to database: {e:#}");
                }
            },
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
            confidence
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING id
        "#,
        event.name,
        event.full_description,
        event.start_date,
        event.end_date,
        event.location,
        event.event_type,
        event.additional_details.as_deref(),
        event.confidence
    )
    .fetch_one(executor)
    .await
    .map_err(|e| anyhow!("Database insert failed: {e}"))?;

    Ok(id)
}

async fn parse_image(image_path: &Path, client: &Client, api_key: &str) -> Result<Event> {
    parse_image_with_now(image_path, client, api_key, Utc::now()).await
}

async fn parse_image_with_now(
    image_path: &Path,
    client: &Client,
    api_key: &str,
    now: DateTime<Utc>,
) -> Result<Event> {
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
    use chrono::TimeZone;

    dotenv().ok();
    let api_key = env::var("OPENAI_API_KEY")?;
    let tls_config = TLS_CONFIG.get_or_init(init_tls_once).clone();

    let client: Client = awc::ClientBuilder::new()
        .timeout(std::time::Duration::from_secs(120))
        .connector(Connector::new().rustls_0_23(tls_config))
        .finish();

    #[derive(Clone, Default)]
    struct InMemoryEventsRepo {
        inserted: Arc<std::sync::Mutex<Vec<Event>>>,
        next_id: Arc<std::sync::Mutex<i64>>,
    }

    #[async_trait]
    impl EventsRepo for InMemoryEventsRepo {
        async fn list(&self) -> Result<Vec<Event>> {
            Ok(self.inserted.lock().unwrap().clone())
        }

        async fn get(&self, id: i64) -> Result<Option<Event>> {
            Ok(self
                .inserted
                .lock()
                .unwrap()
                .iter()
                .find(|e| e.id == Some(id))
                .cloned())
        }

        async fn claim_idempotency_key(&self, _idempotency_key: uuid::Uuid) -> Result<bool> {
            Ok(true)
        }

        async fn insert(&self, event: &Event) -> Result<i64> {
            let mut id_guard = self.next_id.lock().unwrap();
            *id_guard += 1;
            let id = *id_guard;

            let mut stored = event.clone();
            stored.id = Some(id);
            self.inserted.lock().unwrap().push(stored);
            Ok(id)
        }
    }

    let repo = InMemoryEventsRepo {
        inserted: Arc::new(std::sync::Mutex::new(Vec::new())),
        next_id: Arc::new(std::sync::Mutex::new(0)),
    };

    let state = AppState {
        api_key,
        client,
        password: "password".to_string(),
        username: "username".to_string(),
        events_repo: Box::new(repo.clone()),
    };

    // Actix runtime entrypoint
    let fixed_now_utc = Utc.with_ymd_and_hms(2025, 1, 15, 17, 0, 0).unwrap();
    let event = parse_image_with_now(
        Path::new("examples/dance_flyer.jpg"),
        &state.client,
        &state.api_key,
        fixed_now_utc,
    )
    .await?;
    assert_eq!(event.name, "Dance Therapy");

    // "Database" behavior: verify we can "persist" it via the repo without touching Postgres.
    let id = state.events_repo.insert(&event).await?;
    let saved_event = state.events_repo.get(id).await?.expect("saved event");
    let mut expected_event = event.clone();
    expected_event.id = Some(id);
    assert_eq!(saved_event, expected_event);

    Ok(())
}

#[actix_web::test]
async fn test_index() -> Result<()> {
    use actix_web::{test, App};
    use chrono::{NaiveDateTime, NaiveTime, TimeZone};
    use scraper::{Html, Selector};

    // Ensure the rustls process-level CryptoProvider is installed for tests too.
    // Otherwise, awc/rustls can panic when it first touches TLS-related internals.
    let tls_config = TLS_CONFIG.get_or_init(init_tls_once).clone();

    #[derive(Clone)]
    struct FakeEventsRepo {
        events: Arc<Vec<Event>>,
    }

    #[async_trait]
    impl EventsRepo for FakeEventsRepo {
        async fn list(&self) -> Result<Vec<Event>> {
            Ok(self.events.as_ref().clone())
        }

        async fn get(&self, id: i64) -> Result<Option<Event>> {
            Ok(self.events.iter().find(|e| e.id == Some(id)).cloned())
        }

        async fn claim_idempotency_key(&self, _idempotency_key: uuid::Uuid) -> Result<bool> {
            Ok(true)
        }

        async fn insert(&self, _event: &Event) -> Result<i64> {
            Ok(1)
        }
    }

    let now_utc = Utc.with_ymd_and_hms(2025, 1, 15, 17, 0, 0).unwrap();
    let today_local = now_utc.with_timezone(&New_York).date_naive();
    let yesterday_local = today_local.pred_opt().unwrap();
    let tomorrow_local = today_local.succ_opt().unwrap();
    let day_after_tomorrow_local = tomorrow_local.succ_opt().unwrap();

    let mk_local = |d: NaiveDateTime| New_York.from_local_datetime(&d).single().unwrap();
    let local_dt = |date, h, m| NaiveDateTime::new(date, NaiveTime::from_hms_opt(h, m, 0).unwrap());

    let past_event = Event {
        id: Some(1),
        name: "Past Event".to_string(),
        full_description: "Should not render".to_string(),
        start_date: Some(mk_local(local_dt(yesterday_local, 10, 0)).with_timezone(&Utc)),
        end_date: Some(mk_local(local_dt(yesterday_local, 11, 0)).with_timezone(&Utc)),
        location: Some("Somewhere".to_string()),
        event_type: None,
        additional_details: None,
        confidence: 1.0,
    };

    // No end_date: should render only on its start day.
    let ongoing_no_end = Event {
        id: Some(2),
        name: "Ongoing No End".to_string(),
        full_description: "Should render once".to_string(),
        start_date: Some(mk_local(local_dt(today_local, 9, 0)).with_timezone(&Utc)),
        end_date: None,
        location: Some("Somerville".to_string()),
        event_type: None,
        additional_details: None,
        confidence: 1.0,
    };

    // No end_date from yesterday (within the last 24h) should still render, and should
    // cause a "yesterday" heading to appear.
    let yesterday_no_end = Event {
        id: Some(7),
        name: "Yesterday No End".to_string(),
        full_description: "Should render under yesterday".to_string(),
        start_date: Some(mk_local(local_dt(yesterday_local, 15, 0)).with_timezone(&Utc)),
        end_date: None,
        location: Some("Somerville".to_string()),
        event_type: None,
        additional_details: None,
        confidence: 1.0,
    };

    // Two distinct events on the same local day should both render under the same day section.
    let same_day_1 = Event {
        id: Some(5),
        name: "Same Day 1".to_string(),
        full_description: "First event on the same day".to_string(),
        start_date: Some(mk_local(local_dt(today_local, 10, 0)).with_timezone(&Utc)),
        // No end_date so this test doesn't become time-of-day dependent.
        end_date: None,
        location: Some("Union".to_string()),
        event_type: None,
        additional_details: None,
        confidence: 1.0,
    };

    let same_day_2 = Event {
        id: Some(6),
        name: "Same Day 2".to_string(),
        full_description: "Second event on the same day".to_string(),
        start_date: Some(mk_local(local_dt(today_local, 12, 0)).with_timezone(&Utc)),
        // No end_date so this test doesn't become time-of-day dependent.
        end_date: None,
        location: Some("Magoun".to_string()),
        event_type: None,
        additional_details: None,
        confidence: 1.0,
    };

    // Explicit multi-day: should appear under each day.
    let multi_day = Event {
        id: Some(3),
        name: "Multi Day".to_string(),
        full_description: "Spans multiple days".to_string(),
        start_date: Some(mk_local(local_dt(tomorrow_local, 12, 0)).with_timezone(&Utc)),
        end_date: Some(mk_local(local_dt(day_after_tomorrow_local, 13, 0)).with_timezone(&Utc)),
        location: Some("Davis".to_string()),
        event_type: None,
        additional_details: None,
        confidence: 1.0,
    };

    // Missing start: should be excluded entirely.
    let missing_start = Event {
        id: Some(4),
        name: "Missing Start".to_string(),
        full_description: "Not an event".to_string(),
        start_date: None,
        end_date: Some(mk_local(local_dt(tomorrow_local, 10, 0)).with_timezone(&Utc)),
        location: None,
        event_type: None,
        additional_details: None,
        confidence: 1.0,
    };

    // Intentionally shuffled to ensure server-side sorting/grouping is doing the work.
    let fake_repo = FakeEventsRepo {
        events: Arc::new(vec![
            multi_day,
            missing_start,
            past_event,
            same_day_2,
            ongoing_no_end,
            same_day_1,
            yesterday_no_end,
        ]),
    };

    let state = AppState {
        api_key: "dummy".to_string(),
        client: awc::ClientBuilder::new()
            .connector(Connector::new().rustls_0_23(tls_config))
            .finish(),
        username: "user".to_string(),
        password: "pass".to_string(),
        events_repo: Box::new(fake_repo),
    };

    let fixed_now_utc = now_utc;
    let app = test::init_service(App::new().app_data(Data::new(state)).route(
        "/",
        web::get().to(move |state: Data<AppState>| index_with_now(state, fixed_now_utc.clone())),
    ))
    .await;

    let req = test::TestRequest::get().uri("/").to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), actix_web::http::StatusCode::OK);

    let body = test::read_body(resp).await;
    let body_str = std::str::from_utf8(&body)?;

    assert!(body_str.contains("Somerville Events"));
    assert!(!body_str.contains("Missing Start"));
    assert!(!body_str.contains("Past Event"));

    let document = Html::parse_document(body_str);
    let day_sections_sel = Selector::parse("section.day").unwrap();
    let event_link_sel = Selector::parse("article.event h3 a").unwrap();

    let day_ids: Vec<String> = document
        .select(&day_sections_sel)
        .filter_map(|s| s.value().attr("aria-labelledby").map(|v| v.to_string()))
        .collect();

    // We should have headings for today, tomorrow, and the day after tomorrow.
    // No heading for yesterday (past-only).
    assert!(
        day_ids.contains(&format!("day-{}", today_local.format("%Y-%m-%d"))),
        "Missing today's heading id; got day_ids={day_ids:?}"
    );
    assert!(
        day_ids.contains(&format!("day-{}", tomorrow_local.format("%Y-%m-%d"))),
        "Missing tomorrow's heading id; got day_ids={day_ids:?}"
    );
    assert!(
        day_ids.contains(&format!(
            "day-{}",
            day_after_tomorrow_local.format("%Y-%m-%d")
        )),
        "Missing day-after-tomorrow heading id; got day_ids={day_ids:?}"
    );
    assert!(
        day_ids.contains(&format!("day-{}", yesterday_local.format("%Y-%m-%d"))),
        "Expected yesterday heading due to a no-end event within 24h; got day_ids={day_ids:?}"
    );

    // No end_date events should only render once (on their start day).
    let occurrences_ongoing = body_str.matches("Ongoing No End").count();
    assert_eq!(occurrences_ongoing, 1);
    let occurrences_yesterday_no_end = body_str.matches("Yesterday No End").count();
    assert_eq!(occurrences_yesterday_no_end, 1);

    // Multiple events on the same day should show up under the same day section.
    let today_id = format!("day-{}", today_local.format("%Y-%m-%d"));
    let today_section_sel =
        Selector::parse(&format!("section.day[aria-labelledby=\"{today_id}\"]"))
            .expect("selector parse");
    let today_section = document
        .select(&today_section_sel)
        .next()
        .expect("today section");

    let today_articles: Vec<_> = today_section
        .select(&Selector::parse("article.event").unwrap())
        .collect();
    assert!(
        today_articles.len() >= 2,
        "Expected at least two events under today's section"
    );
    let today_text = today_section.text().collect::<String>();
    assert!(today_text.contains("Same Day 1"));
    assert!(today_text.contains("Same Day 2"));

    // "Multi Day" spans tomorrow -> day after tomorrow, so it should appear twice.
    let occurrences_multi = body_str.matches("Multi Day").count();
    assert_eq!(occurrences_multi, 2);

    // Basic sanity: links are present and use expected routes.
    let links: Vec<String> = document
        .select(&event_link_sel)
        .filter_map(|a| a.value().attr("href").map(|s| s.to_string()))
        .collect();
    assert!(links.iter().any(|h| h == "/event/2.html"));
    assert!(links.iter().any(|h| h == "/event/3.html"));

    // Best-effort check that sections contain articles (semantic structure).
    assert!(
        document.select(&day_sections_sel).any(|s| {
            s.select(&Selector::parse("article.event").unwrap())
                .next()
                .is_some()
        }),
        "Expected section.day to contain article.event"
    );

    Ok(())
}

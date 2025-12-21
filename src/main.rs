mod db;
mod models;
mod upload_events;
mod view_events;

use actix_web::{
    dev::ServiceRequest,
    error::ErrorUnauthorized,
    middleware,
    web::{self, Data},
    App, Error, HttpServer,
};
use actix_web_httpauth::{extractors::basic::BasicAuth, middleware::HttpAuthentication};
use anyhow::Result;
use awc::{Client, Connector};
use db::{EventsDatabase, EventsRepo};
use dotenvy::dotenv;
use rustls;
use sqlx::postgres::PgPoolOptions;
use std::{
    env,
    sync::{Arc, OnceLock},
};
use upload_events::{upload, upload_success, upload_ui};
use view_events::{event_details, event_ical, index};

pub const COMMON_STYLES: &str = r#"
    :root {
        --link-color: light-dark(rgb(27, 50, 100),rgb(125, 148, 197));
        
        /* Button Colors - Adjusted for dark mode */
        --button-bg: light-dark(#e0e0e0, #333);
        --button-text: light-dark(#333, #eee);
        --button-shadow-light: light-dark(rgba(255, 255, 255, 0.8), rgba(255, 255, 255, 0.1));
        --button-shadow-dark: rgba(0, 0, 0, 0.1);
        --button-border: light-dark(#a0a0a0, #555);
        
        --primary-bg: #d13a26;
        --primary-text: #fff;
        --primary-shadow: #8c2415;
    }

    body {
        font-family: system-ui, sans-serif;
        max-width: 800px;
        margin: 0 auto;
        padding: 1rem;
        line-height: 1.5;
    }

    a {
        text-decoration: none;
        color: var(--link-color);
    }

    a:hover {
        text-decoration: underline;
    }

    header {
        display: flex;
        align-items: baseline;
        justify-content: space-between;
        gap: 1rem;
        flex-wrap: wrap;
        margin-bottom: 2rem;
    }

    header h1 {
        margin: 0;
        font-size: 2em; 
    }

    h1 { margin-bottom: 1rem; }
    h2 { margin-top: 2.5rem; }

    section {
        margin-bottom: 2.5rem;
    }

    article {
        padding: 1rem 0;
        border-top: 1px solid color-mix(in srgb, currentColor 15%, transparent);
    }

    article:first-child {
        border-top: 0;
    }

    article dl {
        margin: 0.5rem 0 0.75rem 0;
        display: grid;
        grid-template-columns: 7rem 1fr;
        gap: 0.25rem 1rem;
    }

    article dt {
        font-weight: 600;
    }
    
    article dd {
        margin: 0;
    }

    article p {
        margin: 0.75rem 0;
    }

    /* Button Styling */
    button, 
    .button, 
    input[type=file]::file-selector-button {
        display: inline-block;
        padding: 0.8rem 1.4rem;
        font-family: system-ui, sans-serif;
        font-size: 1rem;
        font-weight: 600;
        text-decoration: none;
        text-align: center;
        color: var(--button-text);
        background-color: var(--button-bg);
        border: none;
        border-radius: 4px;
        box-shadow: 
            inset 1px 1px 0px var(--button-shadow-light),
            inset -1px -1px 0px var(--button-shadow-dark),
            0 4px 0 var(--button-border),
            0 5px 8px rgba(0,0,0,0.2);
        cursor: pointer;
        transition: transform 0.1s, box-shadow 0.1s;
    }

    button:active,
    .button:active,
    input[type=file]::file-selector-button:active {
        transform: translateY(4px);
        box-shadow: 
            inset 2px 2px 5px rgba(0, 0, 0, 0.1),
            0 0 0 var(--button-border);
    }

    .button.primary, button.primary, button[type=submit] {
        background-color: var(--primary-bg);
        color: var(--primary-text);
        box-shadow: 
            inset 1px 1px 0px rgba(255, 255, 255, 0.2),
            inset -1px -1px 0px rgba(0, 0, 0, 0.2),
            0 4px 0 var(--primary-shadow),
            0 5px 8px rgba(0,0,0,0.3);
    }

    .button.primary:active, button.primary:active, button[type=submit]:active {
        box-shadow: 
            inset 2px 2px 5px rgba(0, 0, 0, 0.2),
            0 0 0 var(--primary-shadow);
    }

    .hidden {
        display: none !important;
    }
    "#;

pub struct AppState {
    pub api_key: String,
    pub client: Client,
    pub username: String,
    pub password: String,
    pub events_repo: Box<dyn EventsRepo>,
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
    let host = env::var("HOST").expect("HOST");
    let api_key: String = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY");
    let username = env::var("BASIC_AUTH_USER").expect("BASIC_AUTH_USER");
    let password = env::var("BASIC_AUTH_PASS").expect("BASIC_AUTH_PASS");
    let db_user = env::var("DB_APP_USER").expect("DB_APP_USER");
    let db_password = env::var("DB_APP_USER_PASS").expect("DB_APP_USER_PASS");
    let db_name = env::var("DB_NAME").expect("DB_NAME");
    let static_file_dir = env::var("STATIC_FILE_DIR").unwrap_or_else(|_| "./static".to_string());

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
            .service(actix_files::Files::new("/static", &static_file_dir).show_files_listing())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::EventsRepo;
    use crate::models::Event;
    use actix_web::test;
    use async_trait::async_trait;
    use chrono::{NaiveDateTime, NaiveTime, TimeZone, Utc};
    use chrono_tz::America::New_York;
    use scraper::{Html, Selector};
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    #[actix_web::test]
    async fn test_parse_image() -> Result<()> {
        dotenv().ok();
        let api_key = env::var("OPENAI_API_KEY")?;
        let tls_config = TLS_CONFIG.get_or_init(init_tls_once).clone();

        let client: Client = awc::ClientBuilder::new()
            .timeout(std::time::Duration::from_secs(120))
            .connector(Connector::new().rustls_0_23(tls_config))
            .finish();

        #[derive(Clone, Default)]
        struct InMemoryEventsRepo {
            inserted: Arc<Mutex<Vec<Event>>>,
            next_id: Arc<Mutex<i64>>,
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
            inserted: Arc::new(Mutex::new(Vec::new())),
            next_id: Arc::new(Mutex::new(0)),
        };

        let state = AppState {
            api_key: api_key.clone(),
            client: client.clone(),
            password: "password".to_string(),
            username: "username".to_string(),
            events_repo: Box::new(repo.clone()),
        };

        // Actix runtime entrypoint
        let fixed_now_utc = Utc.with_ymd_and_hms(2025, 1, 15, 17, 0, 0).unwrap();
        // Use the public function from upload module
        let event = crate::upload_events::parse_image_with_now(
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
        let local_dt =
            |date, h, m| NaiveDateTime::new(date, NaiveTime::from_hms_opt(h, m, 0).unwrap());

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
            web::get().to(move |state: Data<AppState>| {
                crate::view_events::index_with_now(state, fixed_now_utc.clone())
            }),
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
        let day_sections_sel = Selector::parse("section").unwrap();
        let event_link_sel = Selector::parse("article h3 a").unwrap();

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
            Selector::parse(&format!("section[aria-labelledby=\"{today_id}\"]"))
                .expect("selector parse");
        let today_section = document
            .select(&today_section_sel)
            .next()
            .expect("today section");

        let today_articles: Vec<_> = today_section
            .select(&Selector::parse("article").unwrap())
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
                s.select(&Selector::parse("article").unwrap())
                    .next()
                    .is_some()
            }),
            "Expected section to contain article"
        );

        Ok(())
    }
}

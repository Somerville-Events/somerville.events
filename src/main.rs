mod config;
mod database;
mod features;
pub mod geocoding;
mod image_processing;
mod models;

use actix_web::{
    dev::ServiceRequest,
    error::ErrorUnauthorized,
    middleware,
    web::{self, Data},
    App, Error, HttpServer,
};
use actix_web_httpauth::{extractors::basic::BasicAuth, middleware::HttpAuthentication};
use actix_web_query_method_middleware::QueryMethod;
use anyhow::Result;
use config::Config;
use database::EventsRepo;
use sqlx::postgres::PgPoolOptions;

pub struct AppState {
    pub openai_api_key: String,
    pub google_maps_api_key: String,
    pub username: String,
    pub password: String,
    pub events_repo: Box<dyn EventsRepo>,
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
    let config = Config::from_env();
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    let db_url = config.get_db_url();

    let db_connection_pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await?;

    log::info!("Starting server at http://localhost:8080");

    let host = config.host.clone();
    let static_file_dir = config.static_file_dir.clone();

    let state = AppState {
        openai_api_key: config.openai_api_key.clone(),
        google_maps_api_key: config.google_maps_api_key.clone(),
        username: config.username.clone(),
        password: config.password.clone(),
        events_repo: Box::new(db_connection_pool),
    };
    let app_state = Data::new(state);

    HttpServer::new(move || {
        let auth_middleware = HttpAuthentication::basic(basic_auth_validator);

        let client = awc::ClientBuilder::new()
            .timeout(std::time::Duration::from_secs(120))
            .finish();

        App::new()
            .app_data(app_state.clone())
            .app_data(Data::new(client))
            .wrap(QueryMethod::default())
            .wrap(middleware::Logger::default())
            .service(actix_files::Files::new("/static", &static_file_dir).show_files_listing())
            .route("/", web::get().to(features::view::index))
            .route("/event/{id}.ical", web::get().to(features::view::ical))
            .route("/event/{id}", web::get().to(features::view::show))
            .service(
                web::resource("/upload")
                    .wrap(auth_middleware.clone())
                    .route(web::get().to(features::upload::index))
                    .route(web::post().to(features::upload::save)),
            )
            .service(
                web::resource("/event/{id}")
                    .wrap(auth_middleware.clone())
                    .route(web::delete().to(features::edit::delete)),
            )
            .service(
                web::scope("/edit")
                    .wrap(auth_middleware)
                    .route("", web::get().to(features::edit::index)),
            )
            .route("/upload-success", web::get().to(features::upload::success))
    })
    .bind((host, 8080))?
    .run()
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::database::EventsRepo;
    use super::features::view::IndexQuery;
    use super::models::{Event, EventType};
    use super::AppState;
    use actix_web::web::Data;
    use actix_web::{test, web, App};
    use anyhow::Result;
    use async_trait::async_trait;
    use chrono::{DateTime, NaiveDateTime, NaiveTime, TimeZone, Timelike, Utc};
    use chrono_tz::America::New_York;
    use scraper::{Html, Selector};
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    pub struct MockEventsRepo {
        pub events: Arc<Mutex<Vec<Event>>>,
        pub next_id: Arc<Mutex<i64>>,
    }

    impl MockEventsRepo {
        pub fn new(events: Vec<Event>) -> Self {
            let max_id = events.iter().filter_map(|e| e.id).max().unwrap_or(0);
            Self {
                events: Arc::new(Mutex::new(events)),
                next_id: Arc::new(Mutex::new(max_id)),
            }
        }
    }

    #[async_trait]
    impl EventsRepo for MockEventsRepo {
        async fn list(
            &self,
            category: Option<String>,
            since: Option<DateTime<Utc>>,
            until: Option<DateTime<Utc>>,
        ) -> Result<Vec<Event>> {
            let events = self.events.lock().unwrap().clone();
            Ok(events
                .into_iter()
                .filter(|e| {
                    let cat_match = if let Some(cat) = &category {
                        e.event_type
                            .as_ref()
                            .map(|c| c.to_string().eq_ignore_ascii_case(cat))
                            .unwrap_or(false)
                    } else {
                        true
                    };
                    let since_match = if let Some(since_dt) = since {
                        e.start_date >= since_dt
                    } else {
                        true
                    };
                    let until_match = if let Some(until_dt) = until {
                        e.start_date <= until_dt
                    } else {
                        true
                    };
                    cat_match && since_match && until_match
                })
                .collect())
        }

        async fn get(&self, id: i64) -> Result<Option<Event>> {
            Ok(self
                .events
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
            self.events.lock().unwrap().push(stored);
            Ok(id)
        }

        async fn delete(&self, id: i64) -> Result<()> {
            let mut events = self.events.lock().unwrap();
            let len_before = events.len();
            events.retain(|e| e.id != Some(id));
            if events.len() == len_before {
                return Err(anyhow::anyhow!("Event not found"));
            }
            Ok(())
        }
    }

    #[actix_web::test]
    async fn test_index_filters_by_category() -> Result<()> {
        // 2025-01-15 17:00:00 UTC = 12:00:00 EST
        let now_utc = Utc.with_ymd_and_hms(2025, 1, 15, 17, 0, 0).unwrap();

        // Helper to create a NY datetime
        let mk_ny = |d, h, m| New_York.with_ymd_and_hms(2025, 1, d, h, m, 0).unwrap();

        let art_event = Event {
            id: Some(1),
            name: "Art Show".to_string(),
            description: "Paintings galore".to_string(),
            full_text: "Paintings galore".to_string(),
            start_date: mk_ny(15, 11, 0).with_timezone(&Utc),
            end_date: None,
            address: Some("Gallery".to_string()),
            original_location: Some("Gallery".to_string()),
            google_place_id: None,
            location_name: None,
            event_type: Some(EventType::Art),
            url: None,
            confidence: 1.0,
        };

        let music_event = Event {
            id: Some(2),
            name: "Music Night".to_string(),
            description: "Jazz and blues".to_string(),
            full_text: "Jazz and blues".to_string(),
            start_date: mk_ny(15, 19, 0).with_timezone(&Utc),
            end_date: None,
            address: Some("Club".to_string()),
            original_location: Some("Club".to_string()),
            google_place_id: None,
            location_name: None,
            event_type: Some(EventType::Music),
            url: None,
            confidence: 1.0,
        };

        let state = AppState {
            openai_api_key: "dummy".to_string(),
            google_maps_api_key: "dummy".to_string(),
            username: "user".to_string(),
            password: "pass".to_string(),
            events_repo: Box::new(MockEventsRepo::new(vec![art_event.clone(), music_event])),
        };

        let fixed_now_utc = now_utc;
        let filter = Some("Art".to_string());
        let app = test::init_service(App::new().app_data(Data::new(state)).route(
            "/",
            web::get().to(move |state: Data<AppState>| {
                crate::features::view::index_with_now(
                    state,
                    fixed_now_utc,
                    IndexQuery {
                        category: filter.clone(),
                        past: None,
                    },
                )
            }),
        ))
        .await;

        let req = test::TestRequest::get().uri("/?category=Art").to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), actix_web::http::StatusCode::OK);

        let body = test::read_body(resp).await;
        let body_str = std::str::from_utf8(&body)?;

        assert!(body_str.contains("Art Show"));
        assert!(!body_str.contains("Music Night"));
        assert!(body_str.contains(r#"<a href="/?category=Art">Art</a>"#));
        assert!(body_str.contains("Somerville Art Events"));
        assert!(body_str.contains(r#"<a class="button" href="/">Show all events</a>"#));

        Ok(())
    }

    #[actix_web::test]
    async fn test_index() -> Result<()> {
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
            description: "Should not render".to_string(),
            full_text: "Should not render".to_string(),
            start_date: mk_local(local_dt(yesterday_local, 10, 0)).with_timezone(&Utc),
            end_date: Some(mk_local(local_dt(yesterday_local, 11, 0)).with_timezone(&Utc)),
            address: Some("Somewhere".to_string()),
            original_location: Some("Somewhere".to_string()),
            google_place_id: None,
            location_name: None,
            event_type: None,
            url: None,
            confidence: 1.0,
        };

        // No end_date: should render only on its start day.
        let ongoing_no_end = Event {
            id: Some(2),
            name: "Ongoing No End".to_string(),
            description: "Should render once".to_string(),
            full_text: "Should render once".to_string(),
            start_date: mk_local(local_dt(today_local, 9, 0)).with_timezone(&Utc),
            end_date: None,
            address: Some("Somerville".to_string()),
            original_location: Some("Somerville".to_string()),
            google_place_id: None,
            location_name: None,
            event_type: None,
            url: None,
            confidence: 1.0,
        };

        // No end_date from yesterday (within the last 24h) should still render, and should
        // cause a "yesterday" heading to appear.
        let yesterday_no_end = Event {
            id: Some(7),
            name: "Yesterday No End".to_string(),
            description: "Should render under yesterday".to_string(),
            full_text: "Should render under yesterday".to_string(),
            start_date: mk_local(local_dt(yesterday_local, 15, 0)).with_timezone(&Utc),
            end_date: None,
            address: Some("Somerville".to_string()),
            original_location: Some("Somerville".to_string()),
            google_place_id: None,
            location_name: None,
            event_type: None,
            url: None,
            confidence: 1.0,
        };

        // Two distinct events on the same local day should both render under the same day section.
        let same_day_1 = Event {
            id: Some(5),
            name: "Same Day 1".to_string(),
            description: "First event on the same day".to_string(),
            full_text: "First event on the same day".to_string(),
            start_date: mk_local(local_dt(today_local, 10, 0)).with_timezone(&Utc),
            // No end_date so this test doesn't become time-of-day dependent.
            end_date: None,
            address: Some("Union".to_string()),
            original_location: Some("Union".to_string()),
            google_place_id: None,
            location_name: None,
            event_type: None,
            url: None,
            confidence: 1.0,
        };

        let same_day_2 = Event {
            id: Some(6),
            name: "Same Day 2".to_string(),
            description: "Second event on the same day".to_string(),
            full_text: "Second event on the same day".to_string(),
            start_date: mk_local(local_dt(today_local, 12, 0)).with_timezone(&Utc),
            // No end_date so this test doesn't become time-of-day dependent.
            end_date: None,
            address: Some("Magoun".to_string()),
            original_location: Some("Magoun".to_string()),
            google_place_id: None,
            location_name: None,
            event_type: None,
            url: None,
            confidence: 1.0,
        };

        // Explicit multi-day: should appear under each day.
        let multi_day = Event {
            id: Some(3),
            name: "Multi Day".to_string(),
            description: "Spans multiple days".to_string(),
            full_text: "Spans multiple days".to_string(),
            start_date: mk_local(local_dt(tomorrow_local, 12, 0)).with_timezone(&Utc),
            end_date: Some(mk_local(local_dt(day_after_tomorrow_local, 13, 0)).with_timezone(&Utc)),
            address: Some("Davis".to_string()),
            original_location: Some("Davis".to_string()),
            google_place_id: None,
            location_name: None,
            event_type: None,
            url: None,
            confidence: 1.0,
        };

        // Intentionally shuffled to ensure server-side sorting/grouping is doing the work.
        let mock_repo = MockEventsRepo::new(vec![
            multi_day,
            past_event,
            same_day_2,
            ongoing_no_end,
            same_day_1,
            yesterday_no_end,
        ]);

        let state = AppState {
            openai_api_key: "dummy".to_string(),
            google_maps_api_key: "dummy".to_string(),
            username: "user".to_string(),
            password: "pass".to_string(),
            events_repo: Box::new(mock_repo),
        };

        let fixed_now_utc = now_utc;
        let app = test::init_service(App::new().app_data(Data::new(state)).route(
            "/",
            web::get().to(move |state: Data<AppState>| {
                crate::features::view::index_with_now(
                    state,
                    fixed_now_utc,
                    IndexQuery {
                        category: None,
                        past: None,
                    },
                )
            }),
        ))
        .await;

        let req = test::TestRequest::get().uri("/").to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), actix_web::http::StatusCode::OK);

        let body = test::read_body(resp).await;
        let body_str = std::str::from_utf8(&body)?;

        assert!(body_str.contains("Somerville Events"));
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
        assert!(links.iter().any(|h| h == "/event/2"));
        assert!(links.iter().any(|h| h == "/event/3"));

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

    #[actix_web::test]
    async fn test_ical_endpoint() -> Result<()> {
        let today_start = New_York.with_ymd_and_hms(2025, 1, 15, 0, 0, 0).unwrap();

        let event = Event {
            id: Some(1),
            name: "ICal Event".to_string(),
            description: "Description for ICal".to_string(),
            full_text: "Description for ICal".to_string(),
            start_date: today_start.with_hour(10).unwrap().with_timezone(&Utc),
            end_date: Some(today_start.with_hour(11).unwrap().with_timezone(&Utc)),
            address: Some("Virtual".to_string()),
            original_location: Some("Virtual".to_string()),
            google_place_id: None,
            location_name: None,
            event_type: None,
            url: Some("http://example.com/event".to_string()),
            confidence: 1.0,
        };

        let state = AppState {
            openai_api_key: "dummy".to_string(),
            google_maps_api_key: "dummy".to_string(),
            username: "user".to_string(),
            password: "pass".to_string(),
            events_repo: Box::new(MockEventsRepo::new(vec![event])),
        };

        let app = test::init_service(App::new().app_data(Data::new(state)).route(
            "/event/{id}.ical",
            web::get().to(crate::features::view::ical),
        ))
        .await;

        let req = test::TestRequest::get().uri("/event/1.ical").to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), actix_web::http::StatusCode::OK);

        let headers = resp.headers();
        assert_eq!(headers.get("Content-Type").unwrap(), "text/calendar");
        assert!(headers
            .get("Content-Disposition")
            .unwrap()
            .to_str()?
            .contains("filename=\"event-1.ics\""));

        let body = test::read_body(resp).await;
        let body_str = std::str::from_utf8(&body)?;

        assert!(body_str.contains("BEGIN:VCALENDAR"));
        assert!(body_str.contains("SUMMARY:ICal Event"));
        assert!(body_str.contains("DESCRIPTION:Description for ICal"));
        assert!(body_str.contains("LOCATION:Virtual"));
        assert!(body_str.contains("URL:http://example.com/event"));

        // Date verification
        // 2025-01-15 10:00:00 EST -> 20250115T100000
        // 2025-01-15 11:00:00 EST -> 20250115T110000
        // We verify that DTSTART is associated with the start time and DTEND with the end time
        // by checking that they appear on the same line or in the expected format.
        // The icalendar crate output format is typically: DTSTART;TZID=America/New_York:20250115T100000

        let start_line = body_str
            .lines()
            .find(|l| l.starts_with("DTSTART"))
            .expect("DTSTART missing");
        assert!(
            start_line.contains("20250115T100000"),
            "DTSTART line does not contain expected start time: {}",
            start_line
        );

        let end_line = body_str
            .lines()
            .find(|l| l.starts_with("DTEND"))
            .expect("DTEND missing");
        assert!(
            end_line.contains("20250115T110000"),
            "DTEND line does not contain expected end time: {}",
            end_line
        );

        assert!(body_str.contains("END:VCALENDAR"));

        Ok(())
    }

    #[actix_web::test]
    async fn test_event_time_display_timezone() -> Result<()> {
        let event = Event {
            id: Some(1),
            name: "Pumpkin Smash".to_string(),
            description: "Smash pumpkins".to_string(),
            full_text: "Smash pumpkins".to_string(),
            // Correctly stored UTC time for 10:30 AM EST is 15:30 UTC.
            start_date: Utc.with_ymd_and_hms(2025, 11, 8, 15, 30, 0).unwrap(),
            end_date: Some(Utc.with_ymd_and_hms(2025, 11, 8, 18, 0, 0).unwrap()),
            address: Some("Somerville".to_string()),
            original_location: Some("Somerville".to_string()),
            google_place_id: None,
            location_name: None,
            event_type: None,
            url: None,
            confidence: 1.0,
        };

        let state = AppState {
            openai_api_key: "dummy".to_string(),
            google_maps_api_key: "dummy".to_string(),
            username: "user".to_string(),
            password: "pass".to_string(),
            events_repo: Box::new(MockEventsRepo::new(vec![event])),
        };

        let fixed_now = Utc.with_ymd_and_hms(2025, 11, 8, 8, 0, 0).unwrap();
        let app = test::init_service(App::new().app_data(Data::new(state)).route(
            "/",
            web::get().to(move |state: Data<AppState>| {
                // We use fixed_now to ensure the event is considered upcoming
                crate::features::view::index_with_now(
                    state,
                    fixed_now,
                    IndexQuery {
                        category: None,
                        past: None,
                    },
                )
            }),
        ))
        .await;

        let req = test::TestRequest::get().uri("/").to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), actix_web::http::StatusCode::OK);

        let body = test::read_body(resp).await;
        let body_str = std::str::from_utf8(&body)?;

        assert!(
            body_str.contains("10:30 AM"),
            "Body did not contain '10:30 AM'. Content: {}",
            body_str
        );
        Ok(())
    }
}

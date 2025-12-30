use rand::Rng;
use somerville_events::models::{Event, EventType};
use sqlx::{Connection, Executor, PgConnection, PgPool};
use std::env;
use uuid::Uuid;

pub async fn prepare_db() -> PgPool {
    dotenvy::dotenv().ok();
    let db_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    // Parse the connection string to get the base URL (without db name) and the db name
    let (base_url, db_name) = split_db_url(&db_url);
    let bench_db_name = format!("{}_bench_{}", db_name, Uuid::new_v4().simple());

    // Connect to default DB to create the benchmark DB
    let mut conn = PgConnection::connect(&base_url)
        .await
        .expect("Failed to connect to Postgres");
    
    conn.execute(format!(r#"CREATE DATABASE "{}""#, bench_db_name).as_str())
        .await
        .expect("Failed to create benchmark database");

    let bench_db_url = format!("{}/{}", base_url, bench_db_name);
    let pool = PgPool::connect(&bench_db_url)
        .await
        .expect("Failed to connect to benchmark database");

    // Run migrations
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    pool
}

pub async fn cleanup_db(pool: PgPool) {
    let opts = pool.connect_options();
    let db_name = opts.get_database().unwrap();
    
    pool.close().await;

    // Attempt to drop the DB using the original DATABASE_URL if present
    if let Ok(initial_db_url) = env::var("DATABASE_URL") {
        let (base_url, _) = split_db_url(&initial_db_url);
        if let Ok(mut conn) = PgConnection::connect(&base_url).await {
             let _ = conn.execute(format!(r#"DROP DATABASE IF EXISTS "{}""#, db_name).as_str()).await;
        }
    }
}

fn split_db_url(db_url: &str) -> (String, String) {
    let parts: Vec<&str> = db_url.rsplitn(2, '/').collect();
    if parts.len() == 2 {
        (parts[1].to_string(), parts[0].to_string())
    } else {
        panic!("Invalid DB URL format");
    }
}

pub async fn seed_events(pool: &PgPool, count: usize) {
    let mut rng = rand::rng();
    let event_types = [
        EventType::Music,
        EventType::Art,
        EventType::Comedy,
        EventType::Food,
        EventType::Government,
    ];

    for i in 0..count {
        let event = Event {
            id: None,
            name: format!("Benchmark Event {}", i),
            description: "A very interesting event happening in Somerville.".to_string(),
            full_text: "Full text content of the flyer...".to_string(),
            start_date: chrono::Utc::now() + chrono::Duration::days(rng.random_range(0..30)),
            end_date: Some(chrono::Utc::now() + chrono::Duration::days(rng.random_range(0..30)) + chrono::Duration::hours(2)),
            address: Some(format!("{} Highland Ave", rng.random_range(1..200))),
            original_location: Some("Somerville".to_string()),
            google_place_id: Some(Uuid::new_v4().to_string()),
            location_name: Some("Some Place".to_string()),
            event_type: Some(event_types[rng.random_range(0..event_types.len())].clone()),
            url: Some("https://example.com".to_string()),
            confidence: 0.9,
        };

        somerville_events::database::save_event_to_db(pool, &event)
            .await
            .expect("Failed to insert seed event");
    }
}


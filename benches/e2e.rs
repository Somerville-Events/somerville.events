use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use reqwest::multipart;
use somerville_events::config::Config;
use somerville_events::startup;
use std::net::TcpListener;
use std::path::Path;
use std::time::Duration;
use tokio::runtime::Runtime;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod fixtures;

async fn spawn_app(pool: sqlx::PgPool) -> (String, MockServer, MockServer) {
    let openai_server = MockServer::start().await;
    let google_server = MockServer::start().await;

    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind random port");
    let port = listener.local_addr().unwrap().port();
    let address = format!("http://127.0.0.1:{}", port);

    let mut config = Config::from_env().clone();
    config.openai_base_url = openai_server.uri();
    config.google_maps_base_url = google_server.uri();
    
    // We need to set db_name to match the pool. 
    // However, config has db_user, db_pass, db_name. 
    // The pool is connected to a random DB.
    // We can parse the DB name from the pool connection options? 
    // Or just manually set it since we know how fixtures generates it.
    // fixtures returns the pool.
    let opts = pool.connect_options();
    config.db_name = opts.get_database().unwrap().to_string();

    let server = startup::run(listener, config.clone())
        .await
        .expect("Failed to bind address");

    tokio::spawn(server);

    (address, openai_server, google_server)
}

fn benchmark_page_load(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let pool = rt.block_on(fixtures::prepare_db());
    
    rt.block_on(fixtures::seed_events(&pool, 1000));

    let (app_address, _openai, _google) = rt.block_on(spawn_app(pool.clone()));
    let client = reqwest::Client::new();

    let mut group = c.benchmark_group("Page Load");
    group.throughput(Throughput::Elements(1));
    group.measurement_time(Duration::from_secs(10));

    group.bench_function("GET /", |b| {
        b.to_async(&rt).iter(|| async {
            let response = client.get(&app_address).send().await.unwrap();
            assert!(response.status().is_success());
        })
    });

    group.finish();
}

fn benchmark_upload_pipeline(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let pool = rt.block_on(fixtures::prepare_db());
    let (app_address, openai, google) = rt.block_on(spawn_app(pool.clone()));

    // Mock OpenAI (Delayed)
    rt.block_on(async {
        let response = ResponseTemplate::new(200)
            .set_body_json(serde_json::json!({
                "choices": [{
                    "message": {
                        "content": r#"
                        {
                            "full_text": "Sample Event",
                            "events": [{
                                "name": "Sample Event",
                                "start_date": "2025-01-01T10:00:00",
                                "confidence": 1.0
                            }]
                        }
                        "#
                    }
                }]
            }))
            .set_delay(Duration::from_millis(500)); // Lower delay for bench throughput

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(response)
            .mount(&openai)
            .await;

        let google_response = ResponseTemplate::new(200)
            .set_body_json(serde_json::json!({
                "places": []
            }))
            .set_delay(Duration::from_millis(50));

        Mock::given(method("POST"))
            .and(path("/places:searchText"))
            .respond_with(google_response)
            .mount(&google)
            .await;
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();

    let mut group = c.benchmark_group("Upload Pipeline");
    group.measurement_time(Duration::from_secs(15));
    group.sample_size(10); // Heavy operation

    // Sequential Upload
    group.bench_function("Sequential Upload (Mocked)", |b| {
        b.to_async(&rt).iter(|| async {
            let file_bytes = tokio::fs::read("examples/dance_flyer.jpg").await.unwrap();
            let part = multipart::Part::bytes(file_bytes).file_name("dance_flyer.jpg");
            let form = multipart::Form::new().part("image", part);

            let response = client.post(format!("{}/upload", app_address))
                .basic_auth("user", Some("pass"))
                .multipart(form)
                .send()
                .await
                .unwrap();
            
            assert!(response.status().is_success() || response.status().is_redirection());
        })
    });

    // Concurrent Upload (Stress Test)
    // We measure time to complete N requests
    let concurrent_requests = 10;
    group.throughput(Throughput::Elements(concurrent_requests as u64));
    group.bench_function("Concurrent Upload x10", |b| {
        b.to_async(&rt).iter(|| async {
            let mut handles = Vec::new();
            // Read file once to save IO time in bench loop if desired, but Part::bytes takes ownership.
            // So we have to read or clone each time. Cloning bytes is fast.
            let file_bytes_base = tokio::fs::read("examples/dance_flyer.jpg").await.unwrap();

            for _ in 0..concurrent_requests {
                let client = client.clone();
                let addr = app_address.clone();
                let file_bytes = file_bytes_base.clone();
                handles.push(tokio::spawn(async move {
                    let part = multipart::Part::bytes(file_bytes).file_name("dance_flyer.jpg");
                    let form = multipart::Form::new().part("image", part);
                    
                    client.post(format!("{}/upload", addr))
                        .basic_auth("user", Some("pass"))
                        .multipart(form)
                        .send()
                        .await
                        .unwrap()
                }));
            }
            futures_util::future::join_all(handles).await;
        })
    });

    group.finish();
}

criterion_group!(benches, benchmark_page_load, benchmark_upload_pipeline);
criterion_main!(benches);

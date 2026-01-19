use actix_web::http::header::HeaderMap;
use actix_web::HttpServer;
use actix_web::{test, web, App, HttpRequest, HttpResponse};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::Value;
use somerville_events::database::EventsRepo;
use somerville_events::features::activitypub::{deliver_event_to_followers, inbox};
use somerville_events::models::{
    ActivityPubComment, ActivityPubFollower, ActivityPubInboxActivityInsert, ActivityPubSummary,
    Event, EventSource, EventType, LocationOption, NewEvent, SimpleEvent,
};
use somerville_events::AppState;
use std::net::TcpListener;
use std::sync::{Arc, Mutex, Once};
use tokio::sync::mpsc;

const TEST_PRIVATE_KEY: &str = include_str!("fixtures/activitypub_test_private.pem");
const TEST_PUBLIC_KEY: &str = include_str!("fixtures/activitypub_test_public.pem");

fn init_env() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        std::env::set_var("OPENAI_API_KEY", "test");
        std::env::set_var("GOOGLE_MAPS_API_KEY", "test");
        std::env::set_var("BASIC_AUTH_USER", "user");
        std::env::set_var("BASIC_AUTH_PASS", "pass");
        std::env::set_var("DB_APP_USER_PASS", "pass");
        std::env::set_var("DB_NAME", "test_db");
        std::env::set_var("STATIC_FILE_DIR", "static");
        std::env::set_var("PUBLIC_URL", "http://localhost");
        std::env::set_var("ACTIVITYPUB_PRIVATE_KEY_PEM", TEST_PRIVATE_KEY);
        std::env::set_var("ACTIVITYPUB_PUBLIC_KEY_PEM", TEST_PUBLIC_KEY);
    });
}

#[derive(Default)]
struct TestRepo {
    followers: Arc<Mutex<Vec<ActivityPubFollower>>>,
    inbox_activities: Arc<Mutex<Vec<ActivityPubInboxActivityInsert>>>,
    events: Arc<Mutex<Vec<Event>>>,
    rsvps: Arc<Mutex<Vec<(i64, String, String)>>>,
}

impl TestRepo {
    fn with_event(event: Event) -> Self {
        Self {
            followers: Arc::new(Mutex::new(Vec::new())),
            inbox_activities: Arc::new(Mutex::new(Vec::new())),
            events: Arc::new(Mutex::new(vec![event])),
            rsvps: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl EventsRepo for TestRepo {
    async fn list(
        &self,
        _query: somerville_events::features::view::IndexQuery,
        _since: Option<chrono::DateTime<chrono::Utc>>,
        _until: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<Vec<SimpleEvent>> {
        Err(anyhow!("not implemented"))
    }

    async fn list_full(
        &self,
        _query: somerville_events::features::view::IndexQuery,
        _since: Option<chrono::DateTime<chrono::Utc>>,
        _until: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<Vec<Event>> {
        Err(anyhow!("not implemented"))
    }

    async fn list_full_unfiltered(
        &self,
        _since: Option<chrono::DateTime<chrono::Utc>>,
        _until: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<Vec<Event>> {
        Err(anyhow!("not implemented"))
    }

    async fn list_full_unfiltered_paged(&self, _limit: i64, _offset: i64) -> Result<Vec<Event>> {
        Err(anyhow!("not implemented"))
    }

    async fn count_unfiltered(&self) -> Result<i64> {
        Ok(0)
    }

    async fn get_distinct_locations(&self) -> Result<Vec<LocationOption>> {
        Err(anyhow!("not implemented"))
    }

    async fn get(&self, id: i64) -> Result<Option<Event>> {
        let events = self.events.lock().unwrap();
        Ok(events.iter().find(|event| event.id == id).cloned())
    }

    async fn claim_idempotency_key(&self, _idempotency_key: uuid::Uuid) -> Result<bool> {
        Err(anyhow!("not implemented"))
    }

    async fn insert(&self, _event: &NewEvent) -> Result<i64> {
        Err(anyhow!("not implemented"))
    }

    async fn delete(&self, _id: i64) -> Result<()> {
        Err(anyhow!("not implemented"))
    }

    async fn upsert_activitypub_follower(
        &self,
        actor_id: &str,
        actor_url: &str,
        inbox_url: &str,
        shared_inbox_url: Option<&str>,
        public_key_pem: Option<&str>,
    ) -> Result<()> {
        let mut followers = self.followers.lock().unwrap();
        followers.push(ActivityPubFollower {
            actor_id: actor_id.to_string(),
            actor_url: actor_url.to_string(),
            inbox_url: inbox_url.to_string(),
            shared_inbox_url: shared_inbox_url.map(|value| value.to_string()),
            public_key_pem: public_key_pem.map(|value| value.to_string()),
        });
        Ok(())
    }

    async fn remove_activitypub_follower(&self, actor_id: &str) -> Result<()> {
        let mut followers = self.followers.lock().unwrap();
        followers.retain(|follower| follower.actor_id != actor_id);
        Ok(())
    }

    async fn list_activitypub_followers(&self) -> Result<Vec<ActivityPubFollower>> {
        let followers = self.followers.lock().unwrap();
        Ok(followers.clone())
    }

    async fn insert_activitypub_inbox_activity(
        &self,
        activity: &ActivityPubInboxActivityInsert,
    ) -> Result<()> {
        let mut activities = self.inbox_activities.lock().unwrap();
        activities.push(activity.clone());
        Ok(())
    }

    async fn upsert_activitypub_rsvp(
        &self,
        event_id: i64,
        actor_id: &str,
        rsvp_type: &str,
        _activity_id: &str,
        _object_id: Option<&str>,
        _payload: serde_json::Value,
    ) -> Result<()> {
        let mut rsvps = self.rsvps.lock().unwrap();
        rsvps.push((event_id, actor_id.to_string(), rsvp_type.to_string()));
        Ok(())
    }

    async fn get_activitypub_summary(&self, _event_id: i64) -> Result<ActivityPubSummary> {
        Ok(ActivityPubSummary {
            likes: 0,
            boosts: 0,
            replies: 0,
            rsvp_yes: 0,
            rsvp_maybe: 0,
            rsvp_no: 0,
        })
    }

    async fn list_activitypub_comments(
        &self,
        _event_id: i64,
        _limit: i64,
    ) -> Result<Vec<ActivityPubComment>> {
        Ok(Vec::new())
    }
}

fn build_actor_response(actor_url: &str, inbox_url: &str) -> Value {
    serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": actor_url,
        "type": "Person",
        "inbox": inbox_url,
        "endpoints": { "sharedInbox": inbox_url },
        "publicKey": {
            "id": format!("{}#main-key", actor_url),
            "owner": actor_url,
            "publicKeyPem": TEST_PUBLIC_KEY
        }
    })
}

async fn spawn_remote_actor_server(
    received: Arc<Mutex<Vec<Value>>>,
    received_headers: Arc<Mutex<Vec<HeaderMap>>>,
) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let server = HttpServer::new(move || {
        let received = received.clone();
        let received_headers = received_headers.clone();
        App::new()
            .route(
                "/actor",
                web::get().to(|req: HttpRequest| async move {
                    let actor_url = format!("http://{}{}", req.connection_info().host(), "/actor");
                    let inbox_url = format!("http://{}{}", req.connection_info().host(), "/inbox");
                    HttpResponse::Ok().json(build_actor_response(&actor_url, &inbox_url))
                }),
            )
            .route(
                "/inbox",
                web::post().to(move |req: HttpRequest, body: web::Bytes| {
                    let received = received.clone();
                    let received_headers = received_headers.clone();
                    async move {
                        let payload: Value = serde_json::from_slice(&body).unwrap();
                        received.lock().unwrap().push(payload);
                        received_headers.lock().unwrap().push(req.headers().clone());
                        HttpResponse::Ok().finish()
                    }
                }),
            )
    })
    .listen(listener)
    .unwrap()
    .run();

    actix_rt::spawn(server);
    format!("http://{}", addr)
}

async fn spawn_inbox_only_server(received: Arc<Mutex<Vec<Value>>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = HttpServer::new(move || {
        let received = received.clone();
        App::new().route(
            "/inbox",
            web::post().to(move |body: web::Bytes| {
                let received = received.clone();
                async move {
                    let payload: Value = serde_json::from_slice(&body).unwrap();
                    received.lock().unwrap().push(payload);
                    HttpResponse::Ok().finish()
                }
            }),
        )
    })
    .listen(listener)
    .unwrap()
    .run();
    actix_rt::spawn(server);
    format!("http://{}", addr)
}

#[actix_rt::test]
async fn inbox_follow_stores_follower_and_accepts() {
    init_env();

    let received = Arc::new(Mutex::new(Vec::new()));
    let received_headers = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();
    let headers_clone = received_headers.clone();

    let remote_base = spawn_remote_actor_server(received_clone, headers_clone).await;
    let actor_url = format!("{}/actor", remote_base);
    let inbox_url = format!("{}/inbox", remote_base);

    let repo = TestRepo::default();
    let (sender, _receiver) = mpsc::channel(10);
    let (image_sender, _image_receiver) = mpsc::channel(10);
    let state = web::Data::new(AppState {
        openai_api_key: "test".to_string(),
        google_maps_api_key: "test".to_string(),
        username: "user".to_string(),
        password: "pass".to_string(),
        events_repo: Box::new(repo),
        activitypub_sender: sender,
        image_processing_sender: image_sender,
    });
    let client = awc::Client::default();

    let app = test::init_service(
        App::new()
            .app_data(state.clone())
            .app_data(web::Data::new(client.clone()))
            .route("/activitypub/inbox", web::post().to(inbox)),
    )
    .await;

    let follow = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": "https://example.com/follow/1",
        "type": "Follow",
        "actor": actor_url,
        "object": "http://localhost/activitypub/actor"
    });

    let req = test::TestRequest::post()
        .uri("/activitypub/inbox")
        .set_json(&follow)
        .to_request();
    let response = test::call_service(&app, req).await;
    assert!(response.status().is_success());

    let followers = state
        .events_repo
        .list_activitypub_followers()
        .await
        .unwrap();
    assert_eq!(followers.len(), 1);
    assert_eq!(followers[0].actor_id, actor_url);
    assert_eq!(followers[0].inbox_url, inbox_url);

    let delivered = received.lock().unwrap();
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0]["type"], "Accept");
}

#[actix_rt::test]
async fn delivery_sends_create_to_remote_inbox() {
    init_env();

    let received = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();
    let remote_base = spawn_inbox_only_server(received_clone).await;

    let repo = TestRepo::with_event(Event {
        id: 42,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        name: "Test Event".to_string(),
        description: "Test".to_string(),
        full_text: "Test".to_string(),
        start_date: chrono::Utc::now(),
        end_date: None,
        address: None,
        original_location: None,
        google_place_id: None,
        location_name: None,
        event_types: vec![EventType::Music],
        url: None,
        confidence: 1.0,
        age_restrictions: None,
        price: None,
        source: EventSource::ImageUpload,
        external_id: None,
    });

    repo.upsert_activitypub_follower(
        "https://remote.example/actor",
        "https://remote.example/actor",
        &format!("{}/inbox", remote_base),
        None,
        Some(TEST_PUBLIC_KEY),
    )
    .await
    .unwrap();

    let (sender, _receiver) = mpsc::channel(10);
    let (image_sender, _image_receiver) = mpsc::channel(10);
    let state = AppState {
        openai_api_key: "test".to_string(),
        google_maps_api_key: "test".to_string(),
        username: "user".to_string(),
        password: "pass".to_string(),
        events_repo: Box::new(repo),
        activitypub_sender: sender,
        image_processing_sender: image_sender,
    };

    deliver_event_to_followers(&state, &awc::Client::default(), 42)
        .await
        .unwrap();

    let delivered = received.lock().unwrap();
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0]["type"], "Create");
}

#[actix_rt::test]
async fn inbox_records_like_boost_reply_and_rsvp() {
    init_env();

    let repo = TestRepo::with_event(Event {
        id: 42,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        name: "Test Event".to_string(),
        description: "Test".to_string(),
        full_text: "Test".to_string(),
        start_date: chrono::Utc::now(),
        end_date: None,
        address: None,
        original_location: None,
        google_place_id: None,
        location_name: None,
        event_types: vec![EventType::Music],
        url: None,
        confidence: 1.0,
        age_restrictions: None,
        price: None,
        source: EventSource::ImageUpload,
        external_id: None,
    });
    let repo_clone = repo.inbox_activities.clone();
    let rsvp_clone = repo.rsvps.clone();

    let (sender, _receiver) = mpsc::channel(10);
    let (image_sender, _image_receiver) = mpsc::channel(10);
    let state = web::Data::new(AppState {
        openai_api_key: "test".to_string(),
        google_maps_api_key: "test".to_string(),
        username: "user".to_string(),
        password: "pass".to_string(),
        events_repo: Box::new(repo),
        activitypub_sender: sender,
        image_processing_sender: image_sender,
    });
    let client = awc::Client::default();

    let app = test::init_service(
        App::new()
            .app_data(state.clone())
            .app_data(web::Data::new(client.clone()))
            .route("/activitypub/inbox", web::post().to(inbox)),
    )
    .await;

    let like = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": "https://example.com/like/1",
        "type": "Like",
        "actor": "https://example.com/users/alice",
        "object": "http://localhost/event/42"
    });
    let boost = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": "https://example.com/announce/1",
        "type": "Announce",
        "actor": "https://example.com/users/alice",
        "object": "http://localhost/event/42"
    });
    let reply = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": "https://example.com/reply/1",
        "type": "Create",
        "actor": "https://example.com/users/alice",
        "object": {
            "id": "https://example.com/notes/1",
            "type": "Note",
            "content": "Looks great!",
            "inReplyTo": "http://localhost/event/42"
        }
    });
    let rsvp = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": "https://example.com/rsvp/1",
        "type": "Accept",
        "actor": "https://example.com/users/alice",
        "object": "http://localhost/event/42"
    });

    for payload in [like, boost, reply, rsvp] {
        let req = test::TestRequest::post()
            .uri("/activitypub/inbox")
            .set_json(&payload)
            .to_request();
        let response = test::call_service(&app, req).await;
        assert!(response.status().is_success());
    }

    let activities = repo_clone.lock().unwrap();
    assert!(activities.iter().any(|a| a.activity_type == "Like"));
    assert!(activities.iter().any(|a| a.activity_type == "Announce"));
    assert!(activities.iter().any(|a| a.activity_type == "Create"));
    assert!(activities.iter().any(|a| a.activity_type == "Accept"));

    let rsvps = rsvp_clone.lock().unwrap();
    assert!(rsvps
        .iter()
        .any(|(event_id, _actor, rsvp_type)| { *event_id == 42 && rsvp_type == "Accept" }));
}

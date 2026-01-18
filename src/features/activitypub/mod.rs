use crate::config::Config;
use crate::models::{ActivityPubFollower, Event};
use crate::AppState;
use actix_web::{web, HttpResponse, Responder};
use awc::Client;
use base64::engine::general_purpose;
use base64::Engine;
use httpdate::fmt_http_date;
use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::DecodePrivateKey;
use serde_json::Value;
use sha2::{Digest, Sha256};
use rsa::signature::{SignatureEncoding, Signer};
use std::collections::HashSet;
use std::time::SystemTime;
use serde::{Deserialize, Serialize};
use url::Url;

const ACTIVITYPUB_USERNAME: &str = "events";
const ACTIVITYPUB_PUBLIC: &str = "https://www.w3.org/ns/activitystreams#Public";
const ACTIVITYPUB_SECURITY_CONTEXT: &str = "https://w3id.org/security/v1";

#[derive(Deserialize)]
pub struct WebfingerQuery {
    resource: String,
}

#[derive(Deserialize)]
pub struct OutboxQuery {
    page: Option<String>,
}

const OUTBOX_PAGE_SIZE: i64 = 100;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ActivityPubActor {
    #[serde(rename = "@context")]
    context: Vec<&'static str>,
    id: String,
    #[serde(rename = "type")]
    kind: &'static str,
    name: String,
    summary: String,
    inbox: String,
    outbox: String,
    preferred_username: String,
    url: String,
    public_key: ActivityPubPublicKey,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OrderedCollection<T> {
    #[serde(rename = "@context")]
    context: Vec<&'static str>,
    id: String,
    #[serde(rename = "type")]
    kind: &'static str,
    total_items: usize,
    ordered_items: Vec<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    first: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last: Option<String>,
}

#[derive(Serialize)]
struct Activity<T> {
    id: String,
    #[serde(rename = "type")]
    kind: &'static str,
    actor: String,
    published: String,
    to: Vec<&'static str>,
    object: T,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OrderedCollectionPage<T> {
    #[serde(rename = "@context")]
    context: Vec<&'static str>,
    id: String,
    #[serde(rename = "type")]
    kind: &'static str,
    part_of: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    next: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prev: Option<String>,
    ordered_items: Vec<T>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ActivityPubEvent {
    id: String,
    #[serde(rename = "type")]
    kind: &'static str,
    name: String,
    summary: String,
    content: String,
    media_type: &'static str,
    start_time: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    location: Option<ActivityPubPlace>,
    url: String,
    published: String,
    updated: String,
    attributed_to: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tag: Vec<ActivityPubTag>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ActivityPubPublicKey {
    id: String,
    owner: String,
    public_key_pem: String,
}

#[derive(Serialize)]
struct ActivityPubPlace {
    #[serde(rename = "type")]
    kind: &'static str,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    address: Option<String>,
}

#[derive(Serialize)]
struct ActivityPubTag {
    #[serde(rename = "type")]
    kind: &'static str,
    name: String,
}

#[derive(Serialize)]
struct WebfingerResponse {
    subject: String,
    aliases: Vec<String>,
    links: Vec<WebfingerLink>,
}

#[derive(Serialize)]
struct WebfingerLink {
    rel: String,
    #[serde(rename = "type")]
    kind: String,
    href: String,
}

fn activitypub_context() -> Vec<&'static str> {
    vec!["https://www.w3.org/ns/activitystreams", ACTIVITYPUB_SECURITY_CONTEXT]
}

fn base_url() -> String {
    Config::from_env()
        .public_url
        .trim_end_matches('/')
        .to_string()
}

fn actor_url(base_url: &str) -> String {
    format!("{}/activitypub/actor", base_url)
}

fn public_key_id(base_url: &str) -> String {
    format!("{}#main-key", actor_url(base_url))
}

fn outbox_url(base_url: &str) -> String {
    format!("{}/activitypub/outbox", base_url)
}

fn outbox_page_url(base_url: &str, page: i64) -> String {
    if page <= 1 {
        format!("{}/activitypub/outbox?page=true", base_url)
    } else {
        format!("{}/activitypub/outbox?page={}", base_url, page)
    }
}

fn inbox_url(base_url: &str) -> String {
    format!("{}/activitypub/inbox", base_url)
}

fn activity_url(base_url: &str, event_id: i64) -> String {
    format!("{}/activitypub/activity/{}", base_url, event_id)
}

fn event_object_url(base_url: &str, event_id: i64) -> String {
    format!("{}/activitypub/event/{}", base_url, event_id)
}

fn event_page_url(base_url: &str, event_id: i64) -> String {
    format!("{}/event/{}", base_url, event_id)
}

fn public_host(base_url: &str) -> String {
    Url::parse(base_url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .or_else(|| {
            Url::parse(&format!("https://{}", base_url))
                .ok()
                .and_then(|u| u.host_str().map(str::to_string))
        })
        .unwrap_or_else(|| base_url.to_string())
}

fn event_id_from_url(raw_url: &str) -> Option<i64> {
    let parsed = Url::parse(raw_url).ok()?;
    let segments: Vec<&str> = parsed.path_segments()?.collect();
    if segments.len() >= 2 && segments[segments.len() - 2] == "event" {
        return segments.last()?.parse::<i64>().ok();
    }
    if segments.len() >= 3
        && segments[segments.len() - 3] == "activitypub"
        && segments[segments.len() - 2] == "event"
    {
        return segments.last()?.parse::<i64>().ok();
    }

    None
}

fn event_location(event: &Event) -> Option<ActivityPubPlace> {
    if let (Some(name), Some(address)) = (&event.location_name, &event.address) {
        return Some(ActivityPubPlace {
            kind: "Place",
            name: name.clone(),
            address: Some(address.clone()),
        });
    }

    let fallback = event
        .address
        .clone()
        .or(event.original_location.clone())
        .unwrap_or_default();

    if fallback.is_empty() {
        None
    } else {
        Some(ActivityPubPlace {
            kind: "Place",
            name: fallback,
            address: None,
        })
    }
}

fn activitypub_event(event: &Event, base_url: &str) -> ActivityPubEvent {
    let summary = if event.description.is_empty() {
        event.name.clone()
    } else {
        event.description.clone()
    };
    let content = if event.full_text.is_empty() {
        summary.clone()
    } else {
        event.full_text.clone()
    };

    ActivityPubEvent {
        id: event_object_url(base_url, event.id),
        kind: "Event",
        name: event.name.clone(),
        summary,
        content,
        media_type: "text/plain",
        start_time: event.start_date.to_rfc3339(),
        end_time: event.end_date.map(|dt| dt.to_rfc3339()),
        location: event_location(event),
        url: event_page_url(base_url, event.id),
        published: event.created_at.to_rfc3339(),
        updated: event.updated_at.to_rfc3339(),
        attributed_to: actor_url(base_url),
        tag: event
            .event_types
            .iter()
            .map(|event_type| ActivityPubTag {
                kind: "Hashtag",
                name: format!("#{}", event_type),
            })
            .collect(),
    }
}

fn activitypub_response<T: Serialize>(payload: &T) -> HttpResponse {
    match serde_json::to_string(payload) {
        Ok(body) => HttpResponse::Ok()
            .content_type("application/activity+json; charset=utf-8")
            .body(body),
        Err(e) => {
            log::error!("Failed to serialize ActivityPub response: {e}");
            HttpResponse::InternalServerError().body("Failed to render ActivityPub response")
        }
    }
}

fn parse_outbox_page(query: &OutboxQuery) -> Result<Option<i64>, HttpResponse> {
    match query.page.as_deref() {
        None => Ok(None),
        Some("true") => Ok(Some(1)),
        Some("false") => Ok(None),
        Some(raw) => match raw.parse::<i64>() {
            Ok(page) if page >= 1 => Ok(Some(page)),
            _ => Err(HttpResponse::BadRequest().body("Invalid page value")),
        },
    }
}

fn value_as_string(value: &Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        return Some(s.to_string());
    }
    value.get("id").and_then(|v| v.as_str()).map(|s| s.to_string())
}

fn parse_datetime(value: &Value) -> Option<chrono::DateTime<chrono::Utc>> {
    value
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc))
}

async fn fetch_remote_actor(
    client: &Client,
    actor_id: &str,
) -> Result<ActivityPubFollower, HttpResponse> {
    let mut response = client
        .get(actor_id)
        .insert_header(("Accept", "application/activity+json"))
        .send()
        .await
        .map_err(|e| {
            log::error!("Failed to fetch actor {actor_id}: {e}");
            HttpResponse::BadRequest().body("Failed to fetch actor")
        })?;

    let bytes = response.body().await.map_err(|e| {
        log::error!("Failed to read actor response {actor_id}: {e}");
        HttpResponse::BadRequest().body("Failed to read actor response")
    })?;

    let payload: Value = serde_json::from_slice(&bytes).map_err(|e| {
        log::error!("Failed to parse actor response {actor_id}: {e}");
        HttpResponse::BadRequest().body("Invalid actor response")
    })?;

    let actor_url = payload
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or(actor_id)
        .to_string();
    let inbox_url = payload
        .get("inbox")
        .and_then(|v| v.as_str())
        .ok_or_else(|| HttpResponse::BadRequest().body("Actor inbox missing"))?
        .to_string();
    let shared_inbox_url = payload
        .get("endpoints")
        .and_then(|v| v.get("sharedInbox"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let public_key_pem = payload
        .get("publicKey")
        .and_then(|v| v.get("publicKeyPem"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Ok(ActivityPubFollower {
        actor_id: actor_id.to_string(),
        actor_url,
        inbox_url,
        shared_inbox_url,
        public_key_pem,
    })
}

fn canonical_request_target(url: &Url) -> String {
    if let Some(query) = url.query() {
        format!("{}?{}", url.path(), query)
    } else {
        url.path().to_string()
    }
}

fn sign_activity(
    inbox_url: &str,
    body: &str,
    private_key_pem: &str,
    key_id: &str,
) -> Result<Vec<(String, String)>, HttpResponse> {
    let url = Url::parse(inbox_url)
        .map_err(|_| HttpResponse::BadRequest().body("Invalid inbox URL"))?;
    let host = url
        .host_str()
        .ok_or_else(|| HttpResponse::BadRequest().body("Invalid inbox host"))?;
    let date = fmt_http_date(SystemTime::now());

    let mut hasher = sha2::Sha256::new();
    hasher.update(body.as_bytes());
    let digest = hasher.finalize();
    let digest_header = format!("SHA-256={}", general_purpose::STANDARD.encode(digest));

    let signing_string = format!(
        "(request-target): post {}\nhost: {}\ndate: {}\ndigest: {}",
        canonical_request_target(&url),
        host,
        date,
        digest_header
    );

    let key = rsa::RsaPrivateKey::from_pkcs8_pem(private_key_pem).map_err(|e| {
        log::error!("Failed to parse ActivityPub private key: {e}");
        HttpResponse::InternalServerError().body("Invalid ActivityPub private key")
    })?;
    let signer = SigningKey::<Sha256>::new_unprefixed(key);
    let signature = signer.sign(signing_string.as_bytes());
    let signature_b64 = general_purpose::STANDARD.encode(signature.to_vec());

    let signature_header = format!(
        "keyId=\"{}\",algorithm=\"rsa-sha256\",headers=\"(request-target) host date digest\",signature=\"{}\"",
        key_id, signature_b64
    );

    Ok(vec![
        ("Host".to_string(), host.to_string()),
        ("Date".to_string(), date),
        ("Digest".to_string(), digest_header),
        ("Signature".to_string(), signature_header),
    ])
}

async fn deliver_signed_activity(
    client: &Client,
    inbox_url: &str,
    activity: &Value,
) -> Result<(), HttpResponse> {
    let config = Config::from_env();
    let key_id = public_key_id(config.public_url.trim_end_matches('/'));
    let body = serde_json::to_string(activity).map_err(|e| {
        log::error!("Failed to serialize ActivityPub activity: {e}");
        HttpResponse::InternalServerError().body("Failed to serialize activity")
    })?;
    let signed_headers = sign_activity(inbox_url, &body, &config.activitypub_private_key_pem, &key_id)?;

    let mut request = client
        .post(inbox_url)
        .insert_header(("Content-Type", "application/activity+json"))
        .insert_header(("Accept", "application/activity+json"));

    for (name, value) in signed_headers {
        request = request.insert_header((name, value));
    }

    let response = request.send_body(body).await.map_err(|e| {
        log::error!("Failed to deliver ActivityPub activity to {inbox_url}: {e}");
        HttpResponse::BadRequest().body("Failed to deliver activity")
    })?;

    if !response.status().is_success() {
        log::warn!(
            "ActivityPub delivery to {inbox_url} failed with status {}",
            response.status()
        );
    }

    Ok(())
}

pub async fn actor() -> impl Responder {
    let base_url = base_url();
    let config = Config::from_env();
    let actor = ActivityPubActor {
        context: activitypub_context(),
        id: actor_url(&base_url),
        kind: "Service",
        name: "Somerville Events".to_string(),
        summary: "Local events in Camberville, curated from flyers and community sources."
            .to_string(),
        inbox: inbox_url(&base_url),
        outbox: outbox_url(&base_url),
        preferred_username: ACTIVITYPUB_USERNAME.to_string(),
        url: base_url.clone(),
        public_key: ActivityPubPublicKey {
            id: public_key_id(&base_url),
            owner: actor_url(&base_url),
            public_key_pem: config.activitypub_public_key_pem.clone(),
        },
    };

    activitypub_response(&actor)
}

pub async fn outbox(
    state: web::Data<AppState>,
    query: actix_web_lab::extract::Query<OutboxQuery>,
) -> impl Responder {
    let base_url = base_url();
    let actor_id = actor_url(&base_url);

    let page = match parse_outbox_page(&query) {
        Ok(page) => page,
        Err(response) => return response,
    };

    let total_items = match state.events_repo.count_unfiltered().await {
        Ok(count) => count,
        Err(e) => {
            log::error!("Failed to count events for ActivityPub outbox: {e}");
            return HttpResponse::InternalServerError().body("Failed to fetch events");
        }
    };

    let total_items_usize = total_items.max(0) as usize;
    let last_page = if total_items_usize == 0 {
        1
    } else {
        ((total_items_usize - 1) / OUTBOX_PAGE_SIZE as usize) + 1
    };

    if let Some(page_number) = page {
        let offset = (page_number - 1) * OUTBOX_PAGE_SIZE;
        match state
            .events_repo
            .list_full_unfiltered_paged(OUTBOX_PAGE_SIZE, offset)
            .await
        {
            Ok(events) => {
                let ordered_items: Vec<Activity<ActivityPubEvent>> = events
                    .iter()
                    .map(|event| Activity {
                        id: activity_url(&base_url, event.id),
                        kind: "Create",
                        actor: actor_id.clone(),
                        published: event.created_at.to_rfc3339(),
                        to: vec![ACTIVITYPUB_PUBLIC],
                        object: activitypub_event(event, &base_url),
                    })
                    .collect();

                let next = if (page_number as usize) < last_page {
                    Some(outbox_page_url(&base_url, page_number + 1))
                } else {
                    None
                };
                let prev = if page_number > 1 {
                    Some(outbox_page_url(&base_url, page_number - 1))
                } else {
                    None
                };

                let page_response = OrderedCollectionPage {
                    context: activitypub_context(),
                    id: outbox_page_url(&base_url, page_number),
                    kind: "OrderedCollectionPage",
                    part_of: outbox_url(&base_url),
                    next,
                    prev,
                    ordered_items,
                };

                activitypub_response(&page_response)
            }
            Err(e) => {
                log::error!("Failed to fetch events for ActivityPub outbox: {e}");
                HttpResponse::InternalServerError().body("Failed to fetch events")
            }
        }
    } else {
        let collection = OrderedCollection::<Activity<ActivityPubEvent>> {
            context: activitypub_context(),
            id: outbox_url(&base_url),
            kind: "OrderedCollection",
            total_items: total_items_usize,
            ordered_items: Vec::new(),
            first: Some(outbox_page_url(&base_url, 1)),
            last: Some(outbox_page_url(&base_url, last_page as i64)),
        };

        activitypub_response(&collection)
    }
}

pub async fn event(state: web::Data<AppState>, path: web::Path<i64>) -> impl Responder {
    let id = path.into_inner();
    let base_url = base_url();
    match state.events_repo.get(id).await {
        Ok(Some(event)) => activitypub_response(&activitypub_event(&event, &base_url)),
        Ok(None) => HttpResponse::NotFound().body("Event not found"),
        Err(e) => {
            log::error!("Failed to fetch event for ActivityPub: {e}");
            HttpResponse::InternalServerError().body("Failed to fetch event")
        }
    }
}

pub async fn inbox(
    state: web::Data<AppState>,
    client: web::Data<Client>,
    body: web::Bytes,
) -> impl Responder {
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(e) => {
            log::warn!("Invalid ActivityPub inbox payload: {e}");
            return HttpResponse::BadRequest().body("Invalid payload");
        }
    };

    let activity_id = match payload.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return HttpResponse::BadRequest().body("Missing activity id"),
    };
    let activity_type = match payload.get("type").and_then(|v| v.as_str()) {
        Some(value) => value.to_string(),
        None => return HttpResponse::BadRequest().body("Missing activity type"),
    };
    let actor_id = match payload.get("actor").and_then(value_as_string) {
        Some(actor) => actor,
        None => return HttpResponse::BadRequest().body("Missing actor"),
    };

    let object = payload.get("object");
    let object_id = object.and_then(value_as_string);
    let object_type = object
        .and_then(|v| v.get("type"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let object_url = object
        .and_then(|v| v.get("url"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let object_content = object
        .and_then(|v| v.get("content"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let object_published = object.and_then(|v| v.get("published")).and_then(parse_datetime);
    let in_reply_to = object
        .and_then(|v| v.get("inReplyTo"))
        .and_then(value_as_string);

    let event_id = in_reply_to
        .as_deref()
        .and_then(event_id_from_url)
        .or_else(|| object_id.as_deref().and_then(event_id_from_url))
        .or_else(|| object_url.as_deref().and_then(event_id_from_url));

    let inbox_activity = crate::models::ActivityPubInboxActivityInsert {
        activity_id: activity_id.clone(),
        activity_type: activity_type.clone(),
        actor_id: actor_id.clone(),
        object_id: object_id.clone(),
        object_type: object_type.clone(),
        object_url: object_url.clone(),
        object_content: object_content.clone(),
        object_published,
        in_reply_to: in_reply_to.clone(),
        event_id,
        payload: payload.clone(),
    };

    if let Err(e) = state
        .events_repo
        .insert_activitypub_inbox_activity(&inbox_activity)
        .await
    {
        log::error!("Failed to store ActivityPub inbox activity: {e}");
        return HttpResponse::InternalServerError().body("Failed to store activity");
    }

    let base_url = base_url();
    let local_actor = actor_url(&base_url);

    match activity_type.as_str() {
        "Follow" => {
            if let Some(object_id) = object_id.as_deref() {
                if object_id != local_actor {
                    return HttpResponse::Accepted().finish();
                }
            }

            let remote_actor = match fetch_remote_actor(&client, &actor_id).await {
                Ok(actor) => actor,
                Err(response) => return response,
            };

            if let Err(e) = state
                .events_repo
                .upsert_activitypub_follower(
                    &remote_actor.actor_id,
                    &remote_actor.actor_url,
                    &remote_actor.inbox_url,
                    remote_actor.shared_inbox_url.as_deref(),
                    remote_actor.public_key_pem.as_deref(),
                )
                .await
            {
                log::error!("Failed to store ActivityPub follower: {e}");
                return HttpResponse::InternalServerError().body("Failed to store follower");
            }

            let accept_activity = serde_json::json!({
                "@context": activitypub_context(),
                "id": format!("{}/activitypub/accept/{}", base_url, uuid::Uuid::new_v4()),
                "type": "Accept",
                "actor": local_actor,
                "object": payload
            });

            let inbox_target = remote_actor.shared_inbox_url.as_deref().unwrap_or(&remote_actor.inbox_url);
            if let Err(response) = deliver_signed_activity(&client, inbox_target, &accept_activity).await {
                return response;
            }

            HttpResponse::Accepted().finish()
        }
        "Undo" => {
            let object_type = object
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if object_type == "Follow" {
                if let Err(e) = state.events_repo.remove_activitypub_follower(&actor_id).await {
                    log::error!("Failed to remove ActivityPub follower: {e}");
                    return HttpResponse::InternalServerError().body("Failed to remove follower");
                }
            }

            HttpResponse::Accepted().finish()
        }
        "Accept" | "TentativeAccept" | "Reject" => {
            if let Some(event_id) = event_id {
                if let Err(e) = state
                    .events_repo
                    .upsert_activitypub_rsvp(
                        event_id,
                        &actor_id,
                        &activity_type,
                        &activity_id,
                        object_id.as_deref(),
                        payload,
                    )
                    .await
                {
                    log::error!("Failed to store ActivityPub RSVP: {e}");
                    return HttpResponse::InternalServerError().body("Failed to store RSVP");
                }
            }

            HttpResponse::Accepted().finish()
        }
        _ => HttpResponse::Accepted().finish(),
    }
}

pub async fn webfinger(query: actix_web_lab::extract::Query<WebfingerQuery>) -> impl Responder {
    let base_url = base_url();
    let host = public_host(&base_url);
    let subject = format!("acct:{}@{}", ACTIVITYPUB_USERNAME, host);
    let actor = actor_url(&base_url);

    let matches_subject = query.resource == subject;
    let matches_actor = query.resource == actor;

    if !matches_subject && !matches_actor {
        return HttpResponse::NotFound().body("Unknown resource");
    }

    let response = WebfingerResponse {
        subject,
        aliases: vec![actor.clone()],
        links: vec![WebfingerLink {
            rel: "self".to_string(),
            kind: "application/activity+json".to_string(),
            href: actor,
        }],
    };

    match serde_json::to_string(&response) {
        Ok(body) => HttpResponse::Ok()
            .content_type("application/jrd+json; charset=utf-8")
            .body(body),
        Err(e) => {
            log::error!("Failed to serialize WebFinger response: {e}");
            HttpResponse::InternalServerError().body("Failed to render WebFinger response")
        }
    }
}

pub async fn deliver_event_to_followers(
    state: &AppState,
    client: &Client,
    event_id: i64,
) -> Result<(), HttpResponse> {
    let event = match state.events_repo.get(event_id).await {
        Ok(Some(event)) => event,
        Ok(None) => return Ok(()),
        Err(e) => {
            log::error!("Failed to fetch event for ActivityPub delivery: {e}");
            return Err(HttpResponse::InternalServerError().body("Failed to fetch event"));
        }
    };

    let followers = match state.events_repo.list_activitypub_followers().await {
        Ok(followers) => followers,
        Err(e) => {
            log::error!("Failed to list ActivityPub followers: {e}");
            return Err(HttpResponse::InternalServerError().body("Failed to list followers"));
        }
    };

    if followers.is_empty() {
        return Ok(());
    }

    let base_url = base_url();
    let actor_id = actor_url(&base_url);
    let activity = Activity {
        id: activity_url(&base_url, event.id),
        kind: "Create",
        actor: actor_id,
        published: event.created_at.to_rfc3339(),
        to: vec![ACTIVITYPUB_PUBLIC],
        object: activitypub_event(&event, &base_url),
    };

    let activity_value = serde_json::to_value(activity).map_err(|e| {
        log::error!("Failed to serialize ActivityPub activity: {e}");
        HttpResponse::InternalServerError().body("Failed to serialize activity")
    })?;

    let mut delivered = HashSet::new();
    for follower in followers {
        let inbox_url = follower
            .shared_inbox_url
            .as_deref()
            .unwrap_or(&follower.inbox_url)
            .to_string();
        if !delivered.insert(inbox_url.clone()) {
            continue;
        }

        if let Err(response) = deliver_signed_activity(client, &inbox_url, &activity_value).await {
            log::warn!(
                "Failed to deliver ActivityPub activity to {}: {}",
                inbox_url,
                response.status()
            );
        }
    }

    Ok(())
}

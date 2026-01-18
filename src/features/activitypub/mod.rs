use crate::config::Config;
use crate::features::view::IndexQuery;
use crate::models::Event;
use crate::AppState;
use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Duration, TimeZone, Utc};
use chrono_tz::America::New_York;
use serde::{Deserialize, Serialize};
use url::Url;

const ACTIVITYPUB_USERNAME: &str = "events";
const ACTIVITYPUB_PUBLIC: &str = "https://www.w3.org/ns/activitystreams#Public";

#[derive(Deserialize)]
pub struct WebfingerQuery {
    resource: String,
}

#[derive(Serialize)]
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
    #[serde(rename = "preferredUsername")]
    preferred_username: String,
    url: String,
}

#[derive(Serialize)]
struct OrderedCollection<T> {
    #[serde(rename = "@context")]
    context: Vec<&'static str>,
    id: String,
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(rename = "totalItems")]
    total_items: usize,
    #[serde(rename = "orderedItems")]
    ordered_items: Vec<T>,
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
struct ActivityPubEvent {
    id: String,
    #[serde(rename = "type")]
    kind: &'static str,
    name: String,
    summary: String,
    content: String,
    #[serde(rename = "mediaType")]
    media_type: &'static str,
    #[serde(rename = "startTime")]
    start_time: String,
    #[serde(rename = "endTime", skip_serializing_if = "Option::is_none")]
    end_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    location: Option<ActivityPubPlace>,
    url: String,
    published: String,
    updated: String,
    #[serde(rename = "attributedTo")]
    attributed_to: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tag: Vec<ActivityPubTag>,
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
    vec!["https://www.w3.org/ns/activitystreams"]
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

fn outbox_url(base_url: &str) -> String {
    format!("{}/activitypub/outbox", base_url)
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

fn default_time_range(now_utc: DateTime<Utc>) -> (Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
    let index_query = IndexQuery::default();
    let has_date_filter =
        index_query.since.is_some() || index_query.until.is_some() || index_query.on.is_some();

    let (since, until) = if let Some(on_date) = index_query.on {
        let start = New_York
            .from_local_datetime(&on_date.and_hms_opt(0, 0, 0).unwrap())
            .single()
            .unwrap()
            .with_timezone(&Utc);
        let end = New_York
            .from_local_datetime(&on_date.and_hms_opt(23, 59, 59).unwrap())
            .single()
            .unwrap()
            .with_timezone(&Utc);
        (Some(start), Some(end))
    } else if has_date_filter {
        let start = index_query.since.map(|d| {
            New_York
                .from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap())
                .single()
                .unwrap()
                .with_timezone(&Utc)
        });
        let end = index_query.until.map(|d| {
            New_York
                .from_local_datetime(&d.and_hms_opt(23, 59, 59).unwrap())
                .single()
                .unwrap()
                .with_timezone(&Utc)
        });
        (start, end)
    } else if index_query.past.unwrap_or(false) {
        (None, Some(now_utc))
    } else {
        (Some(now_utc - Duration::days(2)), None)
    };

    (since, until)
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

pub async fn actor() -> impl Responder {
    let base_url = base_url();
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
    };

    activitypub_response(&actor)
}

pub async fn outbox(state: web::Data<AppState>) -> impl Responder {
    let now_utc = Utc::now();
    let (since, until) = default_time_range(now_utc);
    let base_url = base_url();
    let actor_id = actor_url(&base_url);

    match state
        .events_repo
        .list_full(IndexQuery::default(), since, until)
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

            let collection = OrderedCollection {
                context: activitypub_context(),
                id: outbox_url(&base_url),
                kind: "OrderedCollection",
                total_items: ordered_items.len(),
                ordered_items,
            };

            activitypub_response(&collection)
        }
        Err(e) => {
            log::error!("Failed to fetch events for ActivityPub outbox: {e}");
            HttpResponse::InternalServerError().body("Failed to fetch events")
        }
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

pub async fn inbox() -> impl Responder {
    HttpResponse::NotImplemented().body("Inbox not supported")
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

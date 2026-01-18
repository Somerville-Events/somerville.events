use crate::config::Config;
use crate::features::common::{
    get_color_for_type, get_icon_for_type, DateFormat, EventLocation, EventViewModel,
    SimpleEventViewModel,
};
use crate::models::{Event, EventSource, EventType, SimpleEvent};
use crate::AppState;
use actix_web::http::header::ContentType;
use actix_web::{web, HttpResponse, Responder};
use askama::Template;
use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use chrono_tz::America::New_York;
use icalendar::{Calendar, CalendarDateTime, Component, Event as IcalEvent, EventLike};
use serde::Deserialize;
use std::collections::BTreeMap;
use strum::IntoEnumIterator;

#[derive(Template)]
#[template(path = "view/index.html")]
pub struct IndexTemplate {
    pub days: Vec<DaySection>,
    pub is_past_view: bool,
    pub all_event_types: Vec<EventTypeViewModel>,
    pub all_sources: Vec<LabeledValue>,
    pub all_locations: Vec<LabeledValue>,
    pub query: IndexQuery,
    pub prev_day_link: Option<String>,
    pub next_day_link: Option<String>,
    pub webcal_url: String,
    pub https_url: String,
    pub google_cal_link: String,
}

pub struct EventTypeViewModel {
    pub value: String,
    pub label: String,
    pub icon: String,
    pub color: String,
}

pub struct LabeledValue {
    pub value: String,
    pub label: String,
}

#[derive(Template)]
#[template(path = "view/show.html")]
pub struct ShowTemplate {
    pub event: EventViewModel,
}

#[derive(Template)]
#[template(path = "view/atom_entry.html", escape = "none")]
struct AtomEntryTemplate {
    event: EventViewModel,
}

struct AtomEntry {
    id: String,
    title: String,
    link: String,
    updated: String,
    content: String,
}

#[derive(Template)]
#[template(path = "view/events.atom.xml")]
struct AtomFeedTemplate {
    title: String,
    subtitle: String,
    feed_id: String,
    feed_url: String,
    site_url: String,
    updated: String,
    entries: Vec<AtomEntry>,
}

pub struct DaySection {
    pub day_id: String,
    pub date_header: String,
    pub events: Vec<SimpleEventViewModel>,
}

#[derive(Deserialize, Default, Clone)]
pub struct IndexQuery {
    #[serde(default, rename = "type")]
    pub event_types: Vec<EventType>,
    #[serde(default)]
    pub source: Vec<EventSource>,
    #[serde(default)]
    pub location: Vec<String>,
    pub free: Option<bool>,
    pub q: Option<String>,
    pub past: Option<bool>,
    pub since: Option<NaiveDate>,
    pub until: Option<NaiveDate>,
    pub on: Option<NaiveDate>,
}

impl IndexQuery {
    pub fn has_filters(&self) -> bool {
        !self.event_types.is_empty()
            || !self.source.is_empty()
            || !self.location.is_empty()
            || self.free.unwrap_or(false)
            || self.q.as_deref().map(|s| !s.is_empty()).unwrap_or(false)
            || self.since.is_some()
            || self.until.is_some()
            || self.on.is_some()
    }

    pub fn has_event_type(&self, type_val: &str) -> bool {
        self.event_types.iter().any(|t| t.value() == type_val)
    }

    pub fn has_source(&self, source_val: &str) -> bool {
        self.source.iter().any(|s| s.value() == source_val)
    }

    pub fn has_location(&self, location_val: &str) -> bool {
        self.location.iter().any(|l| l == location_val)
    }

    pub fn to_query_string(&self) -> String {
        let mut params = url::form_urlencoded::Serializer::new(String::new());

        for t in &self.event_types {
            params.append_pair("type", &t.value());
        }
        for s in &self.source {
            params.append_pair("source", &s.value());
        }
        for l in &self.location {
            params.append_pair("location", l);
        }
        if let Some(true) = self.free {
            params.append_pair("free", "true");
        }
        if let Some(ref q) = self.q {
            if !q.is_empty() {
                params.append_pair("q", q);
            }
        }
        if let Some(true) = self.past {
            params.append_pair("past", "true");
        }
        if let Some(d) = self.since {
            params.append_pair("since", &d.to_string());
        }
        if let Some(d) = self.until {
            params.append_pair("until", &d.to_string());
        }
        if let Some(d) = self.on {
            params.append_pair("on", &d.to_string());
        }

        params.finish()
    }
}

fn compute_time_range(
    now_utc: DateTime<Utc>,
    index_query: &IndexQuery,
) -> (bool, bool, Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
    let is_past = index_query.past.unwrap_or(false);
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
    } else if is_past {
        // Past events
        (None, Some(now_utc))
    } else {
        // Upcoming events (default)
        (Some(now_utc - Duration::days(2)), None)
    };

    (is_past, has_date_filter, since, until)
}

pub async fn index(
    state: web::Data<AppState>,
    query: actix_web_lab::extract::Query<IndexQuery>,
) -> impl Responder {
    index_with_now(state, Utc::now(), query.into_inner()).await
}

pub async fn index_with_now(
    state: web::Data<AppState>,
    now_utc: DateTime<Utc>,
    query: IndexQuery,
) -> impl Responder {
    let (is_past, has_date_filter, since, until) = compute_time_range(now_utc, &query);

    // Fetch events and distinct locations
    let events_result = state.events_repo.list(query.clone(), since, until).await;
    let locations_result = state.events_repo.get_distinct_locations().await;

    match (events_result, locations_result) {
        (Ok(events), Ok(locations)) => {
            let earliest_day_to_render: NaiveDate = if is_past || has_date_filter {
                NaiveDate::MIN
            } else {
                (now_utc - Duration::days(1))
                    .with_timezone(&New_York)
                    .date_naive()
            };

            let mut events_by_day: BTreeMap<NaiveDate, Vec<SimpleEvent>> = BTreeMap::new();

            for event in events {
                let start = event.start_date;
                let start_day = start.with_timezone(&New_York).date_naive();
                let (end_day, visibility_end) = match event.end_date {
                    None => (start_day, start + Duration::days(1)),
                    Some(end) => (end.with_timezone(&New_York).date_naive(), end),
                };

                // Filter based on visibility relative to now
                if !has_date_filter {
                    if is_past {
                        // In past view, show only events that have ended
                        if visibility_end >= now_utc {
                            continue;
                        }
                    } else {
                        // In upcoming view, show only events that haven't ended yet
                        if visibility_end < now_utc {
                            continue;
                        }
                    }
                }

                let (mut day, last_day) = if start_day <= end_day {
                    (start_day, end_day)
                } else {
                    (end_day, start_day)
                };

                while day <= last_day {
                    if day >= earliest_day_to_render {
                        events_by_day.entry(day).or_default().push(event.clone());
                    }
                    if day == last_day {
                        break;
                    }
                    day = day.succ_opt().expect("date overflow");
                }
            }

            let mut days = Vec::new();
            // Process days. If past view, we want descending order.
            // BTreeMap iterates in ascending order.
            let day_iter: Box<dyn Iterator<Item = (NaiveDate, Vec<SimpleEvent>)>> = if is_past {
                Box::new(events_by_day.into_iter().rev())
            } else {
                Box::new(events_by_day.into_iter())
            };

            for (day, mut day_events) in day_iter {
                day_events.sort_by(|a, b| {
                    a.start_date
                        .cmp(&b.start_date)
                        .then_with(|| a.name.cmp(&b.name))
                });

                let vms: Vec<SimpleEventViewModel> = day_events
                    .iter()
                    .map(|e| SimpleEventViewModel::from_event(e, DateFormat::TimeOnly, "/event"))
                    .collect();

                days.push(DaySection {
                    day_id: format!("day-{}", day.format("%Y-%m-%d")),
                    date_header: day.format("%A, %B %d, %Y").to_string(),
                    events: vms,
                });
            }

            let (prev_day_link, next_day_link) = if let Some(on_date) = query.on {
                let prev_date = on_date.pred_opt().unwrap();
                let next_date = on_date.succ_opt().unwrap();

                let mut prev_query = query.clone();
                prev_query.on = Some(prev_date);

                let mut next_query = query.clone();
                next_query.on = Some(next_date);

                (
                    Some(format!("/?{}", prev_query.to_query_string())),
                    Some(format!("/?{}", next_query.to_query_string())),
                )
            } else {
                (None, None)
            };

            let config = Config::from_env();
            // Construct subscription URLs
            let query_str = query.to_query_string();
            let https_url = if query_str.is_empty() {
                format!("{}/events.ics", config.public_url.trim_end_matches('/'))
            } else {
                format!(
                    "{}/events.ics?{}",
                    config.public_url.trim_end_matches('/'),
                    query_str
                )
            };

            // For webcal, we replace http/https with webcal.
            // If the public_url is just "somerville-events.com", we assume https (webcal).
            // But config.public_url usually includes scheme.
            let webcal_url = if https_url.starts_with("https://") {
                https_url.replace("https://", "webcal://")
            } else if https_url.starts_with("http://") {
                https_url.replace("http://", "webcal://")
            } else {
                format!("webcal://{}", https_url)
            };

            let google_cal_link = format!(
                "https://calendar.google.com/calendar/render?cid={}",
                url::form_urlencoded::byte_serialize(webcal_url.as_bytes()).collect::<String>()
            );

            let template = IndexTemplate {
                days,
                is_past_view: is_past,
                all_event_types: EventType::iter()
                    .map(|t| EventTypeViewModel {
                        value: t.value(),
                        label: t.to_string(),
                        icon: get_icon_for_type(&t).to_string(),
                        color: get_color_for_type(&t),
                    })
                    .collect(),
                all_sources: EventSource::iter()
                    .map(|s| LabeledValue {
                        value: s.value(),
                        label: s.to_string(),
                    })
                    .collect(),
                all_locations: locations
                    .iter()
                    .map(|l| LabeledValue {
                        value: l.id.clone(),
                        label: l.name.clone(),
                    })
                    .collect(),
                query,
                prev_day_link,
                next_day_link,
                webcal_url,
                https_url,
                google_cal_link,
            };

            HttpResponse::Ok()
                .content_type(ContentType::html())
                .body(template.render().unwrap())
        }
        (Err(e), _) | (_, Err(e)) => {
            log::error!("Failed to fetch events or locations: {e}");
            HttpResponse::InternalServerError().body("Failed to fetch events")
        }
    }
}

pub async fn show(state: web::Data<AppState>, path: web::Path<i64>) -> impl Responder {
    let id = path.into_inner();
    match state.events_repo.get(id).await {
        Ok(Some(event)) => {
            let template = ShowTemplate {
                event: EventViewModel::from_event(&event, DateFormat::FullDate, false),
            };
            HttpResponse::Ok()
                .content_type(ContentType::html())
                .body(template.render().unwrap())
        }
        Ok(None) => HttpResponse::NotFound().body("Event not found"),
        Err(e) => {
            log::error!("Failed to fetch event: {e}");
            HttpResponse::InternalServerError().body("Failed to fetch event")
        }
    }
}

fn generate_calendar_metadata(
    index_query: &IndexQuery,
    location_map: &BTreeMap<String, String>,
    public_url: &str,
) -> (String, String) {
    // Name Construction
    let mut name_parts = Vec::new();
    if let Some(q) = &index_query.q {
        if !q.is_empty() {
            name_parts.push(format!("\"{}\"", q));
        }
    }

    if !index_query.event_types.is_empty() {
        let types: Vec<String> = index_query
            .event_types
            .iter()
            .map(|t| t.to_string())
            .collect();
        name_parts.push(types.join(", "));
    }

    name_parts.push("Somerville Events".to_string());

    if !index_query.location.is_empty() {
        let loc_names: Vec<String> = index_query
            .location
            .iter()
            .map(|id| {
                location_map
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| "Unknown Location".to_string())
            })
            .collect();
        name_parts.push(format!("at {}", loc_names.join(", ")));
    }

    if !index_query.source.is_empty() {
        let sources: Vec<String> = index_query.source.iter().map(|s| s.to_string()).collect();
        name_parts.push(format!("from {}", sources.join(", ")));
    }

    let name = name_parts.join(" ");

    // Description Construction
    let query_str = index_query.to_query_string();
    let url = if query_str.is_empty() {
        public_url.to_string()
    } else {
        format!("{}/?{}", public_url.trim_end_matches('/'), query_str)
    };

    let description = format!("Events from Somerville Events.\nView on Web: {}", url);

    (name, description)
}

async fn load_location_map(
    state: &web::Data<AppState>,
    index_query: &IndexQuery,
) -> BTreeMap<String, String> {
    if index_query.location.is_empty() {
        return BTreeMap::new();
    }

    state
        .events_repo
        .get_distinct_locations()
        .await
        .map(|locs| {
            locs.into_iter()
                .map(|l| (l.id, l.name))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default()
}

pub async fn ical_feed(
    state: web::Data<AppState>,
    query: actix_web_lab::extract::Query<IndexQuery>,
) -> impl Responder {
    let index_query = query.into_inner();
    // Use similar logic to index_with_now for date filtering if needed,
    // but typically a subscription feed should include "future" events.
    // However, if the user filters by a specific date, they might expect only that date.
    // The previous implementation of index_with_now handles "since/until/on" or defaults to "now - 2 days".

    // We'll reuse the logic from index_with_now to determine the time range,
    // but we need to compute it here.
    let now_utc = Utc::now();
    let (_is_past, _has_date_filter, since, until) = compute_time_range(now_utc, &index_query);

    // Fetch location names if we have location filters
    let location_map = load_location_map(&state, &index_query).await;

    match state
        .events_repo
        .list_full(index_query.clone(), since, until)
        .await
    {
        Ok(events) => {
            let mut calendar = Calendar::new();

            let config = Config::from_env();
            let (name, description) =
                generate_calendar_metadata(&index_query, &location_map, &config.public_url);

            calendar.name(&name);
            calendar.description(&description);

            for event in events {
                calendar.push(IcalEvent::from(&event));
            }

            HttpResponse::Ok()
                .content_type("text/calendar")
                .insert_header(("Content-Disposition", "inline; filename=\"events.ics\""))
                .body(calendar.to_string())
        }
        Err(e) => {
            log::error!("Failed to fetch events for ical feed: {e}");
            HttpResponse::InternalServerError().body("Failed to fetch events")
        }
    }
}

pub async fn atom_feed(
    state: web::Data<AppState>,
    query: actix_web_lab::extract::Query<IndexQuery>,
) -> impl Responder {
    let index_query = query.into_inner();
    let now_utc = Utc::now();
    let (is_past, _has_date_filter, since, until) = compute_time_range(now_utc, &index_query);

    let location_map = load_location_map(&state, &index_query).await;

    match state
        .events_repo
        .list_full(index_query.clone(), since, until)
        .await
    {
        Ok(events) => {
            let config = Config::from_env();
            let base_url = config.public_url.trim_end_matches('/');
            let query_str = index_query.to_query_string();
            let feed_url = if query_str.is_empty() {
                format!("{}/events.atom", base_url)
            } else {
                format!("{}/events.atom?{}", base_url, query_str)
            };
            let site_url = if query_str.is_empty() {
                base_url.to_string()
            } else {
                format!("{}/?{}", base_url, query_str)
            };

            let (title, subtitle) =
                generate_calendar_metadata(&index_query, &location_map, &config.public_url);

            let entries_result: Result<Vec<AtomEntry>, askama::Error> = events
                .iter()
                .map(|event| {
                    let id = event.id;
                    let link = format!("{}/event/{}", base_url, id);
                    let updated = event.updated_at.to_rfc3339();
                    let content = AtomEntryTemplate {
                        event: EventViewModel::from_event(event, DateFormat::FullDate, is_past),
                    }
                    .render()?;

                    Ok(AtomEntry {
                        id: link.clone(),
                        title: event.name.clone(),
                        link,
                        updated,
                        content,
                    })
                })
                .collect();

            match entries_result {
                Ok(entries) => {
                    let updated = events
                        .iter()
                        .map(|event| event.updated_at)
                        .max()
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_else(|| now_utc.to_rfc3339());

                    let template = AtomFeedTemplate {
                        title,
                        subtitle,
                        feed_id: feed_url.clone(),
                        feed_url,
                        site_url,
                        updated,
                        entries,
                    };

                    HttpResponse::Ok()
                        .content_type("application/atom+xml; charset=utf-8")
                        .body(template.render().unwrap())
                }
                Err(e) => {
                    log::error!("Failed to render Atom feed entries: {e}");
                    HttpResponse::InternalServerError().body("Failed to render Atom feed")
                }
            }
        }
        Err(e) => {
            log::error!("Failed to fetch events for Atom feed: {e}");
            HttpResponse::InternalServerError().body("Failed to fetch events")
        }
    }
}

pub async fn ical(state: web::Data<AppState>, path: web::Path<i64>) -> impl Responder {
    let id = path.into_inner();
    match state.events_repo.get(id).await {
        Ok(Some(event)) => {
            let ical_event = IcalEvent::from(&event);
            let calendar = Calendar::new().push(ical_event).done();

            HttpResponse::Ok()
                .content_type("text/calendar")
                .insert_header((
                    "Content-Disposition",
                    format!("inline; filename=\"event-{}.ics\"", id),
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

impl From<&Event> for IcalEvent {
    fn from(event: &Event) -> Self {
        let mut ical_event = IcalEvent::new();
        ical_event
            .summary(&event.name)
            .description(&event.full_text);

        if let Some(url) = &event.url {
            ical_event.url(url);
        }

        let location = if let (Some(name), Some(addr)) = (&event.location_name, &event.address) {
            format!("{}, {}", name, addr)
        } else {
            event
                .address
                .clone()
                .or(event.original_location.clone())
                .unwrap_or_default()
        };

        if !location.is_empty() {
            ical_event.location(&location);
        }

        let start = event.start_date;
        let start_et = start.with_timezone(&New_York);
        ical_event.starts(CalendarDateTime::from_date_time(start_et));
        if let Some(end) = event.end_date {
            ical_event.ends(CalendarDateTime::from_date_time(
                end.with_timezone(&New_York),
            ));
        } else {
            ical_event.ends(CalendarDateTime::from_date_time(
                start_et + chrono::Duration::hours(1),
            ));
        }

        // Use event ID for UID to ensure updates are tracked correctly
        ical_event.uid(&format!("somerville-events-{}", event.id));

        ical_event
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_calendar_metadata_default() {
        let query = IndexQuery::default();
        let location_map = BTreeMap::new();
        let public_url = "https://example.com";

        let (name, description) = generate_calendar_metadata(&query, &location_map, public_url);

        assert_eq!(name, "Somerville Events");
        assert!(description.contains("Events from Somerville Events"));
        assert!(description.contains(public_url));
    }

    #[test]
    fn test_generate_calendar_metadata_with_search() {
        let query = IndexQuery {
            q: Some("music".to_string()),
            ..Default::default()
        };
        let location_map = BTreeMap::new();
        let public_url = "https://example.com";

        let (name, description) = generate_calendar_metadata(&query, &location_map, public_url);

        assert_eq!(name, "\"music\" Somerville Events");
        assert!(description.contains(public_url));
    }

    #[test]
    fn test_generate_calendar_metadata_with_type() {
        let query = IndexQuery {
            event_types: vec![EventType::Art],
            ..Default::default()
        };
        let location_map = BTreeMap::new();
        let public_url = "https://example.com";

        let (name, description) = generate_calendar_metadata(&query, &location_map, public_url);

        assert_eq!(name, "Art Somerville Events");
        assert!(description.contains(public_url));
    }

    #[test]
    fn test_generate_calendar_metadata_with_location() {
        let query = IndexQuery {
            location: vec!["loc1".to_string()],
            ..Default::default()
        };
        let mut location_map = BTreeMap::new();
        location_map.insert("loc1".to_string(), "The Library".to_string());
        let public_url = "https://example.com";

        let (name, description) = generate_calendar_metadata(&query, &location_map, public_url);

        assert_eq!(name, "Somerville Events at The Library");
        assert!(description.contains(public_url));
    }

    #[test]
    fn test_generate_calendar_metadata_with_source() {
        let query = IndexQuery {
            source: vec![EventSource::CityOfCambridge],
            ..Default::default()
        };
        let location_map = BTreeMap::new();
        let public_url = "https://example.com";

        let (name, description) = generate_calendar_metadata(&query, &location_map, public_url);

        assert_eq!(name, "Somerville Events from City of Cambridge");
        assert!(description.contains(public_url));
    }

    #[test]
    fn test_generate_calendar_metadata_complex() {
        let query = IndexQuery {
            q: Some("concert".to_string()),
            event_types: vec![EventType::Music, EventType::Art],
            location: vec!["loc1".to_string()],
            source: vec![EventSource::ArtsAtTheArmory],
            ..Default::default()
        };

        let mut location_map = BTreeMap::new();
        location_map.insert("loc1".to_string(), "The Armory".to_string());
        let public_url = "https://example.com";

        let (name, description) = generate_calendar_metadata(&query, &location_map, public_url);

        // "concert" Music, Art Somerville Events at The Armory from Arts at the Armory
        assert!(name.contains("\"concert\""));
        assert!(name.contains("Music, Art"));
        assert!(name.contains("Somerville Events"));
        assert!(name.contains("at The Armory"));
        assert!(name.contains("from Arts at the Armory"));

        // Exact match check
        assert_eq!(
            name,
            "\"concert\" Music, Art Somerville Events at The Armory from Arts at the Armory"
        );

        assert!(description.contains(public_url));
    }
}

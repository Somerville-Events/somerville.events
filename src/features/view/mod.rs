use crate::models::Event;
use crate::AppState;
use actix_web::{web, HttpResponse, Responder};
use askama::Template;
use chrono::{DateTime, Duration, NaiveDate, Utc};
use chrono_tz::America::New_York;
use icalendar::{Calendar, CalendarDateTime, Component, Event as IcalEvent, EventLike};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Template)]
#[template(path = "view/index.html")]
struct IndexTemplate {
    page_title: String,
    filter_badge: String,
    days: Vec<DaySection>,
}

#[derive(Template)]
#[template(path = "view/show.html")]
struct ShowTemplate {
    event: EventViewModel,
}

struct DaySection {
    day_id: String,
    date_header: String,
    events: Vec<EventViewModel>,
}

#[derive(Clone)]
struct EventViewModel {
    id: i64,
    name: String,
    start_iso: String,
    start_formatted: String,
    end_iso: String,
    end_formatted: Option<String>,
    location: String,
    category_link: Option<(String, String)>,
    website_link: Option<(String, String)>,
    description: String,
}

impl EventViewModel {
    fn from_event(event: &Event) -> Self {
        let start_ny = event.start_date.with_timezone(&New_York);
        let start_iso = start_ny.to_rfc3339();
        let start_formatted = start_ny.format("%A, %B %d, %Y at %I:%M %p").to_string();

        let (end_iso, end_formatted) = if let Some(end) = event.end_date {
            let end_ny = end.with_timezone(&New_York);
            (
                end_ny.to_rfc3339(),
                Some(end_ny.format("%A, %B %d, %Y at %I:%M %p").to_string()),
            )
        } else {
            (String::new(), None)
        };

        let category_link = event.event_type.as_ref().map(|c| {
             let encoded = url::form_urlencoded::byte_serialize(c.as_bytes()).collect::<String>();
             (format!("/?category={}", encoded), c.clone())
        });

        let website_link = event.url.as_ref().map(|u| (u.clone(), u.clone()));

        Self {
            id: event.id.unwrap_or_default(),
            name: event.name.clone(),
            start_iso,
            start_formatted,
            end_iso,
            end_formatted,
            location: event.location.clone().unwrap_or_default(),
            category_link,
            website_link,
            description: event.full_description.clone(),
        }
    }
}

#[derive(Deserialize)]
pub struct IndexQuery {
    pub category: Option<String>,
}

pub async fn index(state: web::Data<AppState>, query: web::Query<IndexQuery>) -> impl Responder {
    index_with_now(state, Utc::now(), query.into_inner().category).await
}

pub async fn index_with_now(
    state: web::Data<AppState>,
    now_utc: DateTime<Utc>,
    category: Option<String>,
) -> impl Responder {
    let events_result = state.events_repo.list().await;

    match events_result {
        Ok(events) => {
            let filtered_events: Vec<Event> = if let Some(ref category_filter) = category {
                events
                    .into_iter()
                    .filter(|event| {
                        event
                            .event_type
                            .as_ref()
                            .map(|c| c.eq_ignore_ascii_case(category_filter))
                            .unwrap_or(false)
                    })
                    .collect()
            } else {
                events
            };

            let earliest_day_to_render: NaiveDate = (now_utc - Duration::days(1))
                .with_timezone(&New_York)
                .date_naive();

            let mut events_by_day: BTreeMap<NaiveDate, Vec<Event>> = BTreeMap::new();

            for event in filtered_events {
                let start = event.start_date;
                let start_day = start.with_timezone(&New_York).date_naive();
                let (end_day, visibility_end) = match event.end_date {
                    None => (start_day, start + Duration::days(1)),
                    Some(end) => (end.with_timezone(&New_York).date_naive(), end),
                };

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
                        events_by_day.entry(day).or_default().push(event.clone());
                    }
                    if day == last_day {
                        break;
                    }
                    day = day.succ_opt().expect("date overflow");
                }
            }

            let mut days = Vec::new();
            for (day, mut day_events) in events_by_day {
                day_events.sort_by(|a, b| {
                    a.start_date
                        .cmp(&b.start_date)
                        .then_with(|| a.name.cmp(&b.name))
                });

                let vms: Vec<EventViewModel> = day_events.iter().map(EventViewModel::from_event).collect();

                days.push(DaySection {
                    day_id: format!("day-{}", day.format("%Y-%m-%d")),
                    date_header: day.format("%A, %B %d, %Y").to_string(),
                    events: vms,
                });
            }

            let (page_title, filter_badge) = if let Some(ref category_filter) = category {
                (
                    format!("Somerville {} Events", category_filter),
                    category_filter.clone(),
                )
            } else {
                ("Somerville Events".to_string(), String::new())
            };

            let template = IndexTemplate {
                page_title,
                filter_badge,
                days,
            };

            HttpResponse::Ok().body(template.render().unwrap())
        }
        Err(e) => {
            log::error!("Failed to fetch events: {e}");
            HttpResponse::InternalServerError().body("Failed to fetch events")
        }
    }
}

pub async fn show(state: web::Data<AppState>, path: web::Path<i64>) -> impl Responder {
    let id = path.into_inner();
    match state.events_repo.get(id).await {
        Ok(Some(event)) => {
            let template = ShowTemplate {
                event: EventViewModel::from_event(&event),
            };
            HttpResponse::Ok().body(template.render().unwrap())
        }
        Ok(None) => HttpResponse::NotFound().body("Event not found"),
        Err(e) => {
            log::error!("Failed to fetch event: {e}");
            HttpResponse::InternalServerError().body("Failed to fetch event")
        }
    }
}

pub async fn ical(state: web::Data<AppState>, path: web::Path<i64>) -> impl Responder {
    let id = path.into_inner();
    match state.events_repo.get(id).await {
        Ok(Some(event)) => {
            let mut ical_event = IcalEvent::new();
            ical_event
                .summary(&event.name)
                .description(&event.full_description);

            if let Some(location) = event.location {
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


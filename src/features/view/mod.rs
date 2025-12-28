use crate::features::common::{DateFormat, EventLocation, EventViewModel};
use crate::models::Event;
use crate::AppState;
use actix_web::http::header::ContentType;
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
    is_past_view: bool,
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

#[derive(Deserialize)]
pub struct IndexQuery {
    pub category: Option<String>,
    pub past: Option<bool>,
}

pub async fn index(state: web::Data<AppState>, query: web::Query<IndexQuery>) -> impl Responder {
    index_with_now(state, Utc::now(), query.into_inner()).await
}

pub async fn index_with_now(
    state: web::Data<AppState>,
    now_utc: DateTime<Utc>,
    query: IndexQuery,
) -> impl Responder {
    let is_past = query.past.unwrap_or(false);

    let (since, until) = if is_past {
        // For past events, we want events that started before now.
        (None, Some(now_utc))
    } else {
        // For upcoming events, we want events that started recently or are in the future.
        // We have a buffer of 2 days ago because sometimes there are multi-day events
        // with no specified end date.
        (Some(now_utc - Duration::days(2)), None)
    };

    let events_result = state
        .events_repo
        .list(query.category.clone(), since, until)
        .await;

    match events_result {
        Ok(events) => {
            let earliest_day_to_render: NaiveDate = if is_past {
                NaiveDate::MIN
            } else {
                (now_utc - Duration::days(1))
                    .with_timezone(&New_York)
                    .date_naive()
            };

            let mut events_by_day: BTreeMap<NaiveDate, Vec<Event>> = BTreeMap::new();

            for event in events {
                let start = event.start_date;
                let start_day = start.with_timezone(&New_York).date_naive();
                let (end_day, visibility_end) = match event.end_date {
                    None => (start_day, start + Duration::days(1)),
                    Some(end) => (end.with_timezone(&New_York).date_naive(), end),
                };

                // Filter based on visibility relative to now
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
            // Process days. If past view, we want descending order.
            // BTreeMap iterates in ascending order.
            let day_iter: Box<dyn Iterator<Item = (NaiveDate, Vec<Event>)>> = if is_past {
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

                let vms: Vec<EventViewModel> = day_events
                    .iter()
                    .map(|e| EventViewModel::from_event(e, DateFormat::TimeOnly, is_past))
                    .collect();

                days.push(DaySection {
                    day_id: format!("day-{}", day.format("%Y-%m-%d")),
                    date_header: day.format("%A, %B %d, %Y").to_string(),
                    events: vms,
                });
            }

            let (page_title, filter_badge) = if let Some(ref category_filter) = query.category {
                (
                    if is_past {
                        format!("Past Somerville {category_filter} Events")
                    } else {
                        format!("Somerville {category_filter} Events")
                    },
                    category_filter.clone(),
                )
            } else {
                (
                    if is_past {
                        "Past Somerville Events".to_string()
                    } else {
                        "Somerville Events".to_string()
                    },
                    String::new(),
                )
            };

            let template = IndexTemplate {
                page_title,
                filter_badge,
                days,
                is_past_view: is_past,
            };

            HttpResponse::Ok()
                .content_type(ContentType::html())
                .body(template.render().unwrap())
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

pub async fn ical(state: web::Data<AppState>, path: web::Path<i64>) -> impl Responder {
    let id = path.into_inner();
    match state.events_repo.get(id).await {
        Ok(Some(event)) => {
            let mut ical_event = IcalEvent::new();
            ical_event
                .summary(&event.name)
                .description(&event.full_description);

            let location = if let (Some(name), Some(addr)) = (&event.location_name, &event.address)
            {
                format!("{}, {}", name, addr)
            } else {
                event
                    .address
                    .clone()
                    .or(event.original_location)
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

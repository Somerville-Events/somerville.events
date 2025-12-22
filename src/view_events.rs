use crate::common_ui::{render_event_html, COMMON_STYLES};
use crate::models::Event;
use crate::AppState;
use actix_web::{http::header::ContentType, web, web::Data, HttpResponse};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use chrono_tz::America::New_York;
use icalendar::{Calendar, CalendarDateTime, Component, Event as IcalEvent, EventLike};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
pub struct IndexQuery {
    pub category: Option<String>,
}

pub async fn index(state: Data<AppState>, query: web::Query<IndexQuery>) -> HttpResponse {
    index_with_now(state, Utc::now(), query.into_inner().category).await
}

pub async fn index_with_now(
    state: Data<AppState>,
    now_utc: DateTime<Utc>,
    category: Option<String>,
) -> HttpResponse {
    let events = state.events_repo.list().await;

    match events {
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
            // We normally hide anything too far in the past, but we allow a small look-back
            // window so "no end date" events from yesterday still show up.
            let earliest_day_to_render: NaiveDate = (now_utc - Duration::days(1))
                .with_timezone(&New_York)
                .date_naive();

            let mut events_by_day: BTreeMap<NaiveDate, Vec<Event>> = BTreeMap::new();

            for event in filtered_events {
                let start = event.start_date;

                let start_day = start.with_timezone(&New_York).date_naive();
                let (end_day, visibility_end) = match event.end_date {
                    // Events without an end date render only once (on their start day), but they
                    // should remain visible for up to 24h after start (so "yesterday" can show).
                    None => (start_day, start + Duration::days(1)),
                    Some(end) => (end.with_timezone(&New_York).date_naive(), end),
                };

                // "End time is in the past" is our most reliable signal. For missing end dates,
                // we approximate an end for visibility only (see above).
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
                        // Show spanning events multiple times: once per day they touch.
                        events_by_day.entry(day).or_default().push(event.clone());
                    }

                    if day == last_day {
                        break;
                    }
                    day = day.succ_opt().expect("date overflow");
                }
            }

            for day_events in events_by_day.values_mut() {
                day_events.sort_by(|a, b| {
                    a.start_date
                        .cmp(&b.start_date)
                        .then_with(|| a.name.cmp(&b.name))
                });
            }

            let mut events_html = String::new();

            for (day, day_events) in events_by_day {
                let day_id = format!("day-{}", day.format("%Y-%m-%d"));
                events_html.push_str(&format!(
                    r#"<section aria-labelledby="{day_id}">
                        <h2 id="{day_id}">{}</h2>"#,
                    day.format("%A, %B %d, %Y")
                ));

                for event in day_events {
                    events_html.push_str(&render_event_html(&event, false, None));
                }

                events_html.push_str("</section>");
            }

            let (page_title, filter_badge) = if let Some(ref category_filter) = category {
                let category_label = html_escape::encode_text(category_filter);
                (
                    format!("Somerville {} Events", category_label),
                    r#"<p><a class="button" href="/">Show all events</a></p>"#.to_string(),
                )
            } else {
                ("Somerville Events".to_string(), String::new())
            };

            HttpResponse::Ok().content_type(ContentType::html()).body(format!(
                r#"<!doctype html>
                <html lang="en">
                <head>
                    <meta name="color-scheme" content="light dark">
                    <meta name="viewport" content="width=device-width, minimum-scale=1, initial-scale=1">
                    <title>{page_title}</title>
                    <style>
                        {common_styles}
                    </style>
                </head>
                <body>
                    <header>
                        <h1>{page_title}</h1>
                        <nav aria-label="Site">
                            <a href="/upload" class="button primary">Upload new event</a>
                        </nav>
                    </header>
                    <main>
                        {filter_badge}
                        {events_html}
                    </main>
                </body>
                </html>"#,
                page_title = page_title,
                common_styles = COMMON_STYLES,
                filter_badge = filter_badge,
                events_html = events_html
            ))
        }
        Err(e) => {
            log::error!("Failed to fetch events: {e}");
            HttpResponse::InternalServerError().body("Failed to fetch events")
        }
    }
}

pub async fn show(state: Data<AppState>, path: web::Path<i64>) -> HttpResponse {
    let id = path.into_inner();
    let event = state.events_repo.get(id).await;

    match event {
        Ok(Some(event)) => {
            HttpResponse::Ok().content_type(ContentType::html()).body(format!(
                r#"<!doctype html>
                <html lang="en">
                <head>
                    <meta name="color-scheme" content="light dark">
                    <meta name="viewport" content="width=device-width, minimum-scale=1, initial-scale=1">
                    <title>{name} - Somerville Events</title>
                    <style>
                        {common_styles}
                    </style>
                </head>
                <body>
                    <p><a href="/">&larr; Back to Events</a></p>
                    {event_html}
                </body>
                </html>"#,
                name = html_escape::encode_text(&event.name),
                common_styles = COMMON_STYLES,
                event_html = render_event_html(&event, true, None)
            ))
        }
        Ok(None) => HttpResponse::NotFound().body("Event not found"),
        Err(e) => {
            log::error!("Failed to fetch event: {e}");
            HttpResponse::InternalServerError().body("Failed to fetch event")
        }
    }
}

pub async fn ical(state: Data<AppState>, path: web::Path<i64>) -> HttpResponse {
    let id = path.into_inner();
    let event_res = state.events_repo.get(id).await;

    match event_res {
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
                // Default to 1 hour duration if no end date
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

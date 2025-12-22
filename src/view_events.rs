use crate::models::Event;
use crate::{AppState, COMMON_STYLES};
use actix_web::{http::header::ContentType, web, web::Data, HttpResponse};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use chrono_tz::America::New_York;
use icalendar::{Calendar, CalendarDateTime, Component, Event as IcalEvent, EventLike};
use serde::Deserialize;
use std::collections::BTreeMap;

pub fn format_datetime(dt: DateTime<Utc>) -> String {
    // Somerville, MA observes DST, so we use a real TZ database instead of a fixed offset.
    dt.with_timezone(&New_York)
        .format("%A, %B %d, %Y at %I:%M %p")
        .to_string()
}

fn percent_encode_query_value(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{:02X}", b),
        })
        .collect()
}

fn render_event_html(event: &Event, is_details_view: bool) -> String {
    let when = match (event.start_date, event.end_date) {
        (Some(start), Some(end)) => {
            format!("{} – {}", format_datetime(start), format_datetime(end))
        }
        (Some(start), None) => format_datetime(start),
        (None, Some(end)) => format_datetime(end),
        (None, None) => "TBD".to_string(),
    };

    let when_html = match (event.start_date, event.end_date) {
        (Some(start), Some(end)) => format!(
            r#"<time datetime="{start_dt}">{start_label}</time> – <time datetime="{end_dt}">{end_label}</time>"#,
            start_dt = html_escape::encode_double_quoted_attribute(
                &start.with_timezone(&New_York).to_rfc3339()
            ),
            start_label = html_escape::encode_text(&format_datetime(start)),
            end_dt = html_escape::encode_double_quoted_attribute(
                &end.with_timezone(&New_York).to_rfc3339()
            ),
            end_label = html_escape::encode_text(&format_datetime(end)),
        ),
        (Some(start), None) => format!(
            r#"<time datetime="{start_dt}">{start_label}</time>"#,
            start_dt = html_escape::encode_double_quoted_attribute(
                &start.with_timezone(&New_York).to_rfc3339()
            ),
            start_label = html_escape::encode_text(&format_datetime(start)),
        ),
        _ => html_escape::encode_text(&when).to_string(),
    };

    let id = event.id.unwrap_or_default();
    let name = html_escape::encode_text(&event.name);
    let loc_str = event.location.as_deref().unwrap_or("");
    let location = html_escape::encode_text(loc_str);
    let description = html_escape::encode_text(&event.full_description);

    let title_html = if is_details_view {
        format!("<h1>{}</h1>", name)
    } else {
        format!(r#"<h3><a href="/event/{id}.html">{name}</a></h3>"#)
    };

    let category_html = match event.event_type.as_deref() {
        Some(category) if !category.is_empty() => {
            let category_encoded = percent_encode_query_value(category);
            format!(
                r#"<a href="/?category={category_query}">{category_label}</a>"#,
                category_query = html_escape::encode_double_quoted_attribute(&category_encoded),
                category_label = html_escape::encode_text(category)
            )
        }
        _ => "Not specified".to_string(),
    };

    format!(
        r#"
        <article>
            {title_html}
            <dl>
                <dt>When</dt>
                <dd>{when_html}</dd>
                <dt>Location</dt>
                <dd>{location}</dd>
                <dt>Category</dt>
                <dd>{category_html}</dd>
            </dl>
            <p>{description}</p>
            <p><a href="/event/{id}.ical" class="button">Add to calendar</a></p>
        </article>
        "#
    )
}

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
                // If we don't have a start date, we can't show it on a calendar-like "by day" view.
                let Some(start) = event.start_date else {
                    continue;
                };

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
                    events_html.push_str(&render_event_html(&event, false));
                }

                events_html.push_str("</section>");
            }

            let filter_badge = if let Some(category_filter) = category {
                let category_label = html_escape::encode_text(&category_filter);
                r#"<p><a class="button" href="/">"#.to_string()
                    + &format!("Category: {category_label} \u{2715}")
                    + "</a></p>"
            } else {
                String::new()
            };

            HttpResponse::Ok().content_type(ContentType::html()).body(format!(
                r#"<!doctype html>
                <html lang="en">
                <head>
                    <meta name="color-scheme" content="light dark">
                    <meta name="viewport" content="width=device-width, minimum-scale=1, initial-scale=1">
                    <title>Somerville Events</title>
                    <style>
                        {common_styles}
                    </style>
                </head>
                <body>
                    <header>
                        <h1>Somerville Events</h1>
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

pub async fn event_details(state: Data<AppState>, path: web::Path<i64>) -> HttpResponse {
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
                event_html = render_event_html(&event, true)
            ))
        }
        Ok(None) => HttpResponse::NotFound().body("Event not found"),
        Err(e) => {
            log::error!("Failed to fetch event: {e}");
            HttpResponse::InternalServerError().body("Failed to fetch event")
        }
    }
}

pub async fn event_ical(state: Data<AppState>, path: web::Path<i64>) -> HttpResponse {
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

            if let Some(start) = event.start_date {
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

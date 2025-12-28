use crate::models::Event;
use chrono_tz::America::New_York;

#[derive(Clone)]
pub struct EventViewModel {
    pub id: i64,
    pub name: String,
    pub start_iso: String,
    pub start_formatted: String,
    pub end_iso: String,
    pub end_formatted: Option<String>,
    pub location: String,
    pub description: String,
    pub category_link: Option<(String, String)>,
    pub website_link: Option<String>,
}

pub enum DateFormat {
    TimeOnly,
    FullDate,
}

impl EventViewModel {
    pub fn from_event(event: &Event, format: DateFormat, is_past_view: bool) -> Self {
        let start_ny = event.start_date.with_timezone(&New_York);
        let start_iso = start_ny.to_rfc3339();

        let start_formatted = match format {
            DateFormat::TimeOnly => start_ny.format("%I:%M %p").to_string(),
            DateFormat::FullDate => start_ny.format("%A, %B %d, %Y at %I:%M %p").to_string(),
        };

        let (end_iso, end_formatted) = if let Some(end) = event.end_date {
            let end_ny = end.with_timezone(&New_York);
            let end_str = match format {
                DateFormat::TimeOnly => end_ny.format("%I:%M %p").to_string(),
                DateFormat::FullDate => end_ny.format("%A, %B %d, %Y at %I:%M %p").to_string(),
            };
            (end_ny.to_rfc3339(), Some(end_str))
        } else {
            (String::new(), None)
        };

        let category_link = event
            .event_type
            .as_ref()
            .map(|c| (c.get_url_with_past(is_past_view), c.to_string()));

        Self {
            id: event.id.unwrap_or_default(),
            name: event.name.clone(),
            start_iso,
            start_formatted,
            end_iso,
            end_formatted,
            location: event
                .location
                .clone()
                .or(event.original_location.clone())
                .unwrap_or_default(),
            description: event.full_description.clone(),
            category_link,
            website_link: event.url.clone(),
        }
    }
}

use crate::models::Event;
use chrono_tz::America::New_York;

#[derive(Clone)]
pub enum EventLocation {
    Structured {
        name: String,
        address: String,
        google_maps_link: String,
    },
    Unstructured(String),
    Unknown,
}

#[derive(Clone)]
pub struct EventViewModel {
    pub id: i64,
    pub name: String,
    pub start_iso: String,
    pub start_formatted: String,
    pub end_iso: String,
    pub end_formatted: Option<String>,
    pub location: EventLocation,
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

        let location = if let (Some(name), Some(addr), Some(google_place_id)) =
            (&event.location_name, &event.address, &event.google_place_id)
        {
            let encoded_addr: String =
                url::form_urlencoded::byte_serialize(addr.as_bytes()).collect();
            EventLocation::Structured {
                name: name.clone(),
                address: addr.clone(),
                google_maps_link: format!("https://www.google.com/maps/search/?api=1&query={encoded_addr}&query_place_id={google_place_id}")
            }
        } else if let Some(orig) = &event.original_location {
            EventLocation::Unstructured(orig.clone())
        } else {
            EventLocation::Unknown
        };

        Self {
            id: event.id.unwrap_or_default(),
            name: event.name.clone(),
            start_iso,
            start_formatted,
            end_iso,
            end_formatted,
            location,
            description: event.full_description.clone(),
            category_link,
            website_link: event.url.clone(),
        }
    }
}

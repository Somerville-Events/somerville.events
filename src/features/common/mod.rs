use crate::models::{Event, EventType};
use chrono_tz::America::New_York;

pub fn get_color_for_type(t: &EventType) -> String {
    let (light_mode, dark_mode) = match t {
        EventType::Art
        | EventType::Exhibition
        | EventType::Film
        | EventType::Theater
        | EventType::Literature => ("#c2185b", "#f48fb1"), // Pink 700 / 200
        EventType::Music | EventType::Dance | EventType::Performance | EventType::Comedy => {
            ("#7b1fa2", "#ce93d8") // Purple 700 / 200
        }
        EventType::YardSale | EventType::Food | EventType::Market => ("#e65100", "#ffcc80"), // Orange 900 / 200
        EventType::Government
        | EventType::Meeting
        | EventType::Volunteer
        | EventType::PersonalService
        | EventType::Workshop
        | EventType::Fundraiser => ("#455a64", "#b0bec5"), // Blue Grey 700 / 200
        EventType::Family | EventType::ChildFriendly => ("#2e7d32", "#a5d6a7"), // Green 800 / 200
        EventType::Social | EventType::Holiday => ("#d32f2f", "#ef9a9a"),       // Red 700 / 200
        EventType::Sports | EventType::Fitness => ("#1976d2", "#90caf9"),       // Blue 700 / 200
        EventType::Religious => ("#5d4037", "#bcaaa4"),                         // Brown 700 / 200
        EventType::Other => ("#616161", "#eeeeee"),                             // Grey 700 / 200
    };
    format!("light-dark({}, {})", light_mode, dark_mode)
}

pub fn get_icon_for_type(t: &EventType) -> &'static str {
    match t {
        EventType::YardSale => "icon-tag",
        EventType::Art | EventType::Exhibition => "icon-palette",
        EventType::Music | EventType::Dance => "icon-music",
        EventType::Food => "icon-utensils",
        EventType::PersonalService | EventType::Volunteer => "icon-heart-handshake",
        EventType::Meeting => "icon-users",
        EventType::Government => "icon-landmark",
        EventType::Fundraiser => "icon-hand-coins",
        EventType::Film => "icon-film",
        EventType::Theater | EventType::Comedy | EventType::Performance => "icon-drama",
        EventType::Literature => "icon-book-open",
        EventType::Workshop => "icon-education",
        EventType::Fitness | EventType::Sports => "icon-trophy",
        EventType::Market => "icon-store",
        EventType::Family | EventType::ChildFriendly => "icon-baby",
        EventType::Social | EventType::Holiday => "icon-party-popper",
        EventType::Religious => "icon-church",
        EventType::Other => "icon-circle-help",
    }
}

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
pub struct EventTypeLink {
    pub url: String,
    pub label: String,
    pub icon: String,
    pub color: String,
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
    pub full_text_paragraphs: Vec<String>,
    pub event_types: Vec<EventTypeLink>,
    pub website_link: Option<String>,
    pub google_calendar_url: String,
    pub age_restrictions: Option<String>,
    pub price: Option<f64>,
    pub accent_color: String,
    pub accent_icon: String,
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
            DateFormat::TimeOnly => start_ny.format("%-I:%M %p").to_string(),
            DateFormat::FullDate => start_ny.format("%a, %b %-d, %Y • %-I:%M %p").to_string(),
        };

        let (end_iso, end_formatted) = if let Some(end) = event.end_date {
            let end_ny = end.with_timezone(&New_York);
            let end_str = match format {
                DateFormat::TimeOnly => end_ny.format("%-I:%M %p").to_string(),
                DateFormat::FullDate => end_ny.format("%a, %b %-d, %Y • %-I:%M %p").to_string(),
            };
            (end_ny.to_rfc3339(), Some(end_str))
        } else {
            (String::new(), None)
        };

        let event_types = event
            .event_types
            .iter()
            .map(|c| EventTypeLink {
                url: c.get_url_with_past(is_past_view),
                label: c.to_string(),
                icon: get_icon_for_type(c).to_string(),
                color: get_color_for_type(c).to_string(),
            })
            .collect();

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

        let start_utc = event.start_date.format("%Y%m%dT%H%M%SZ").to_string();
        let end_utc = if let Some(end) = event.end_date {
            end.format("%Y%m%dT%H%M%SZ").to_string()
        } else {
            (event.start_date + chrono::Duration::hours(1))
                .format("%Y%m%dT%H%M%SZ")
                .to_string()
        };
        let dates = format!("{}/{}", start_utc, end_utc);

        let location_str = if let (Some(name), Some(addr)) = (&event.location_name, &event.address)
        {
            format!("{}, {}", name, addr)
        } else {
            event
                .address
                .clone()
                .or(event.original_location.clone())
                .unwrap_or_default()
        };

        let mut google_cal_params = url::form_urlencoded::Serializer::new(String::new());
        google_cal_params.append_pair("action", "TEMPLATE");
        google_cal_params.append_pair("text", &event.name);
        google_cal_params.append_pair("dates", &dates);
        google_cal_params.append_pair("details", &event.full_text);
        if !location_str.is_empty() {
            google_cal_params.append_pair("location", &location_str);
        }
        let google_calendar_url = format!(
            "https://calendar.google.com/calendar/render?{}",
            google_cal_params.finish()
        );

        // Use the icon of the first event type, or default to "Other" icon if none
        let first_type = event.event_types.first().unwrap_or(&EventType::Other);
        let accent_icon = get_icon_for_type(first_type).to_string();
        let accent_color = get_color_for_type(first_type);

        Self {
            id: event.id.unwrap_or_default(),
            name: event.name.clone(),
            start_iso,
            start_formatted,
            end_iso,
            end_formatted,
            location,
            description: event.description.clone(),
            full_text_paragraphs: event
                .full_text
                .split('\n')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            event_types,
            website_link: event.url.clone(),
            google_calendar_url,
            age_restrictions: event.age_restrictions.clone(),
            price: event.price,
            accent_color,
            accent_icon,
        }
    }
}

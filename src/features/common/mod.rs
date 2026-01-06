use crate::models::{Event, EventType};
use chrono_tz::America::New_York;

fn get_color_for_type(t: &EventType) -> &'static str {
    match t {
        EventType::Art
        | EventType::Exhibition
        | EventType::Film
        | EventType::Theater
        | EventType::Literature => "#e91e63", // Pink
        EventType::Music | EventType::Dance | EventType::Performance | EventType::Comedy => {
            "#9c27b0"
        } // Purple
        EventType::YardSale | EventType::Food | EventType::Market => "#ff9800", // Orange
        EventType::Government
        | EventType::Meeting
        | EventType::Volunteer
        | EventType::PersonalService
        | EventType::Workshop => "#607d8b", // Blue Grey
        EventType::Family | EventType::ChildFriendly => "#4caf50",              // Green
        EventType::Social | EventType::Holiday | EventType::Fundraiser => "#f44336", // Red
        EventType::Sports | EventType::Fitness => "#2196f3",                    // Blue
        EventType::Religious => "#795548",                                      // Brown
        EventType::Other => "#9e9e9e",                                          // Grey
    }
}

fn get_icon_for_type(t: &EventType) -> &'static str {
    match t {
        EventType::YardSale => r##"<svg><use href="#icon-tag"/></svg>"##, // tag
        EventType::Art | EventType::Exhibition => r##"<svg><use href="#icon-palette"/></svg>"##, // palette
        EventType::Music | EventType::Dance | EventType::Performance => {
            r##"<svg><use href="#icon-music"/></svg>"##
        } // music
        EventType::Food => r##"<svg><use href="#icon-utensils"/></svg>"##, // utensils
        EventType::PersonalService | EventType::Volunteer => {
            r##"<svg><use href="#icon-heart-handshake"/></svg>"##
        } // heart-handshake
        EventType::Meeting | EventType::Government => r##"<svg><use href="#icon-users"/></svg>"##, // users
        EventType::Fundraiser => r##"<svg><use href="#icon-dollar-sign"/></svg>"##, // dollar-sign
        EventType::Film => r##"<svg><use href="#icon-film"/></svg>"##,              // film
        EventType::Theater | EventType::Comedy => r##"<svg><use href="#icon-drama"/></svg>"##, // smile / drama
        EventType::Literature => r##"<svg><use href="#icon-book-open"/></svg>"##, // book-open
        EventType::Workshop => r##"<svg><use href="#icon-wrench"/></svg>"##,      // wrench
        EventType::Fitness | EventType::Sports => r##"<svg><use href="#icon-trophy"/></svg>"##, // trophy
        EventType::Market => r##"<svg><use href="#icon-store"/></svg>"##, // store
        EventType::Family | EventType::ChildFriendly => r##"<svg><use href="#icon-baby"/></svg>"##, // baby
        EventType::Social | EventType::Holiday => r##"<svg><use href="#icon-beer"/></svg>"##, // beer
        EventType::Religious => r##"<svg><use href="#icon-church"/></svg>"##, // church
        EventType::Other => r##"<svg><use href="#icon-circle-help"/></svg>"##, // circle-help
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
    pub category_links: Vec<(String, String)>,
    pub website_link: Option<String>,
    pub google_calendar_url: String,
    pub age_restrictions: Option<String>,
    pub price: Option<f64>,
    pub source: String,
    pub accent_gradient: String,
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

        let category_links = event
            .event_types
            .iter()
            .map(|c| (c.get_url_with_past(is_past_view), c.to_string()))
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

        let colors: Vec<&str> = event.event_types.iter().map(get_color_for_type).collect();
        let accent_gradient = if colors.is_empty() {
            // Default grey if no types
            format!("linear-gradient(to bottom, {}, {})", "#9e9e9e", "#9e9e9e")
        } else if colors.len() == 1 {
            // Single color gradient
            format!("linear-gradient(to bottom, {}, {})", colors[0], colors[0])
        } else {
            // Blend colors
            format!("linear-gradient(to bottom, {})", colors.join(", "))
        };

        // Use the icon of the first event type, or default to "Other" icon if none
        let first_type = event.event_types.first().unwrap_or(&EventType::Other);
        let accent_icon = get_icon_for_type(first_type).to_string();

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
            category_links,
            website_link: event.url.clone(),
            google_calendar_url,
            age_restrictions: event.age_restrictions.clone(),
            price: event.price,
            source: event.source.to_string(),
            accent_gradient,
            accent_icon,
        }
    }
}

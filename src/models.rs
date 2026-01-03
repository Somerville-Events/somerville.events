use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use strum::{Display, EnumString};

#[derive(
    Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Clone, sqlx::Type, Display, EnumString,
)]
#[sqlx(type_name = "app.event_type")]
pub enum EventType {
    #[strum(serialize = "Yard Sale", serialize = "YardSale")]
    YardSale,
    Art,
    Music,
    Dance,
    Performance,
    Food,
    #[strum(serialize = "Personal Service", serialize = "PersonalService")]
    PersonalService,
    Meeting,
    Government,
    Volunteer,
    Fundraiser,
    Film,
    Theater,
    Comedy,
    Literature,
    Exhibition,
    Workshop,
    Fitness,
    Market,
    Sports,
    Family,
    Social,
    Holiday,
    Religious,
    #[strum(serialize = "Child Friendly", serialize = "ChildFriendly")]
    ChildFriendly,
    // Catch-all.
    #[serde(other)]
    Other,
}

impl EventType {
    pub fn get_url(&self) -> String {
        format!("/?category={self}")
    }

    pub fn get_url_with_past(&self, past: bool) -> String {
        if past {
            format!("/?category={self}&past=true")
        } else {
            self.get_url()
        }
    }
}

impl EventType {
    pub fn db_id(&self) -> &'static str {
        match self {
            EventType::YardSale => "YardSale",
            EventType::Art => "Art",
            EventType::Music => "Music",
            EventType::Dance => "Dance",
            EventType::Performance => "Performance",
            EventType::Food => "Food",
            EventType::PersonalService => "PersonalService",
            EventType::Meeting => "Meeting",
            EventType::Government => "Government",
            EventType::Volunteer => "Volunteer",
            EventType::Fundraiser => "Fundraiser",
            EventType::Film => "Film",
            EventType::Theater => "Theater",
            EventType::Comedy => "Comedy",
            EventType::Literature => "Literature",
            EventType::Exhibition => "Exhibition",
            EventType::Workshop => "Workshop",
            EventType::Fitness => "Fitness",
            EventType::Market => "Market",
            EventType::Sports => "Sports",
            EventType::Family => "Family",
            EventType::Social => "Social",
            EventType::Holiday => "Holiday",
            EventType::Religious => "Religious",
            EventType::ChildFriendly => "ChildFriendly",
            EventType::Other => "Other",
        }
    }
}

// Support conversion for sqlx query_as! compatibility
impl From<String> for EventType {
    fn from(s: String) -> Self {
        EventType::from_str(&s).unwrap_or(EventType::Other)
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, PartialEq, Clone, sqlx::FromRow)]
pub struct Event {
    pub name: String,
    pub description: String,
    pub full_text: String,
    pub start_date: DateTime<Utc>,
    pub end_date: Option<DateTime<Utc>>,
    pub address: Option<String>,
    #[serde(skip_deserializing)]
    pub original_location: Option<String>,
    pub google_place_id: Option<String>,
    pub location_name: Option<String>,
    pub event_types: Vec<EventType>,
    pub url: Option<String>,
    /// Confidence level of the extraction (0.0 to 1.0)
    pub confidence: f64,
    /// Database ID (optional)
    #[serde(skip, default)]
    #[schemars(skip)]
    pub id: Option<i64>,
    pub age_restrictions: Option<String>,
    pub price: Option<f64>,
    pub source_name: Option<String>,
}

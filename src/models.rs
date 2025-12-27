use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use strum::{Display, EnumString};

#[derive(
    Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Clone, sqlx::Type, Display, EnumString,
)]
#[sqlx(type_name = "event_type")]
pub enum EventType {
    #[strum(serialize = "Yard Sale")]
    YardSale,
    Art,
    Music,
    Dance,
    Performance,
    Food,
    #[strum(serialize = "Personal Service")]
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
    // Catch-all.
    #[serde(other)]
    Other,
}

impl EventType {
    pub fn get_url(&self) -> String {
        format!("/?category={self}")
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
    pub full_description: String,
    pub start_date: DateTime<Utc>,
    pub end_date: Option<DateTime<Utc>>,
    pub location: Option<String>,
    pub event_type: Option<EventType>,
    pub url: Option<String>,
    /// Confidence level of the extraction (0.0 to 1.0)
    pub confidence: f64,
    /// Database ID (optional)
    #[serde(skip, default)]
    #[schemars(skip)]
    pub id: Option<i64>,
}

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use strum::{Display, EnumString};

#[derive(
    Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Clone, sqlx::Type, Display, EnumString,
)]
#[sqlx(type_name = "app.event_type")]
/// IMPORTANT: This enum is coupled to the `app.event_types` table in the database.
/// If you add a new variant here, you MUST create a migration to insert the corresponding
/// string value into the `app.event_types` table. Otherwise, inserting events with
/// the new type will fail due to foreign key constraints.
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

#[derive(
    Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Clone, sqlx::Type, Display, EnumString,
)]
#[sqlx(type_name = "text")] // Maps to standard TEXT in Postgres, not a custom enum type
/// IMPORTANT: This enum is coupled to the `app.source_names` table in the database.
/// If you add a new variant here, you MUST create a migration to insert the corresponding
/// string value into the `app.source_names` table. Otherwise, inserting events with
/// the new source will fail due to foreign key constraints.
pub enum SourceName {
    #[strum(serialize = "Aeronaut Brewing")]
    AeronautBrewing,
    #[strum(serialize = "American Repertory Theater")]
    AmericanRepertoryTheater,
    #[strum(serialize = "Arts at the Armory")]
    ArtsAtTheArmory,
    #[strum(serialize = "Boston Swing Central")]
    BostonSwingCentral,
    #[strum(serialize = "BostonShows.org")]
    BostonShowsOrg,
    #[strum(serialize = "Brattle Theatre")]
    BrattleTheatre,
    #[strum(serialize = "Central Square Theater")]
    CentralSquareTheater,
    #[strum(serialize = "City of Cambridge")]
    CityOfCambridge,
    #[strum(serialize = "Harvard Art Museums")]
    HarvardArtMuseums,
    #[strum(serialize = "Harvard Book Store")]
    HarvardBookStore,
    #[strum(serialize = "Lamplighter Brewing")]
    LamplighterBrewing,
    #[strum(serialize = "Porter Square Books")]
    PorterSquareBooks,
    #[strum(serialize = "Portico Brewing")]
    PorticoBrewing,
    #[strum(serialize = "Sanders Theatre")]
    SandersTheatre,
    #[strum(serialize = "Somerville Theatre")]
    SomervilleTheatre,
    #[strum(serialize = "The Comedy Studio")]
    TheComedyStudio,
    #[strum(serialize = "The Dance Complex")]
    TheDanceComplex,
    #[strum(serialize = "The Lily Pad")]
    TheLilyPad,
    #[strum(serialize = "The Middle East")]
    TheMiddleEast,
    // Catch-all for when DB has values code doesn't know yet (forward compatibility)
    #[serde(other)]
    Other,
}

// Support conversion for sqlx query_as! compatibility
impl From<String> for SourceName {
    fn from(s: String) -> Self {
        SourceName::from_str(&s).unwrap_or(SourceName::Other)
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
    /// Must match a value in the `app.source_names` table.
    /// If you introduce a new source, you must add it to that table first.
    pub source_name: Option<SourceName>,
}

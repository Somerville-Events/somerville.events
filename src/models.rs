use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use strum::{AsRefStr, EnumIter, EnumString};
use url::Url;

pub fn sanitize_url(url: Option<String>) -> Option<String> {
    let url_str = url?.trim().to_string();
    if url_str.is_empty() {
        return None;
    }

    // Check if it parses as is
    if let Ok(u) = Url::parse(&url_str) {
        if u.scheme() == "http" || u.scheme() == "https" {
            return Some(u.to_string());
        }
    }

    // Try adding https://
    if let Ok(u) = Url::parse(&format!("https://{}", url_str)) {
        // Ensure that the host is valid (e.g., contains a dot)
        if let Some(host) = u.host_str() {
            if host.contains('.') {
                return Some(u.to_string());
            }
        }
    }

    None
}

#[derive(
    Debug,
    Serialize,
    Deserialize,
    JsonSchema,
    PartialEq,
    Eq,
    Clone,
    sqlx::Type,
    EnumString,
    AsRefStr,
    EnumIter,
)]
#[serde(rename_all = "kebab-case")] // Maps to kebab-case for url query params
#[sqlx(type_name = "text")] // Maps to standard TEXT in Postgres, not a custom enum type
/// IMPORTANT: This enum is coupled to the `app.event_types` table in the database.
/// If you add a new variant here, you MUST create a migration to insert the corresponding
/// string value into the `app.event_types` table. Otherwise, inserting events with
/// the new type will fail due to foreign key constraints.
///
/// ALSO IMPORTANT: This enum is coupled to the LLM prompt in `src/image_processing.rs`.
/// If you modify this enum, please update the list of event types in the `SingleEventExtraction` struct documentation in that file.
pub enum EventType {
    YardSale,
    Art,
    Music,
    Dance,
    Performance,
    Food,
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
    Social,
    Holiday,
    Religious,
    ChildFriendly,
    // Catch-all.
    #[serde(other)]
    Other,
}

impl fmt::Display for EventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EventType::YardSale => write!(f, "Yard Sale"),
            EventType::Art => write!(f, "Art"),
            EventType::Music => write!(f, "Music"),
            EventType::Dance => write!(f, "Dance"),
            EventType::Performance => write!(f, "Performance"),
            EventType::Food => write!(f, "Food"),
            EventType::PersonalService => write!(f, "Personal Service"),
            EventType::Meeting => write!(f, "Meeting"),
            EventType::Government => write!(f, "Government"),
            EventType::Volunteer => write!(f, "Volunteer"),
            EventType::Fundraiser => write!(f, "Fundraiser"),
            EventType::Film => write!(f, "Film"),
            EventType::Theater => write!(f, "Theater"),
            EventType::Comedy => write!(f, "Comedy"),
            EventType::Literature => write!(f, "Literature"),
            EventType::Exhibition => write!(f, "Exhibition"),
            EventType::Workshop => write!(f, "Workshop"),
            EventType::Fitness => write!(f, "Fitness"),
            EventType::Market => write!(f, "Market"),
            EventType::Sports => write!(f, "Sports"),
            EventType::Social => write!(f, "Social"),
            EventType::Holiday => write!(f, "Holiday"),
            EventType::Religious => write!(f, "Religious"),
            EventType::ChildFriendly => write!(f, "Child Friendly"),
            EventType::Other => write!(f, "Other"),
        }
    }
}

impl EventType {
    pub fn value(&self) -> String {
        serde_json::to_string(&self)
            .unwrap_or_else(|_| format!("\"{}\"", self.as_ref().to_lowercase()))
            .trim_matches('"')
            .to_string()
    }

    pub fn get_url(&self) -> String {
        format!("/?type={}", self.value())
    }

    pub fn get_url_with_past(&self, past: bool) -> String {
        if past {
            format!("/?type={}&past=true", self.value())
        } else {
            self.get_url()
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
    Debug,
    Serialize,
    Deserialize,
    JsonSchema,
    PartialEq,
    Eq,
    Clone,
    sqlx::Type,
    EnumString,
    AsRefStr,
    EnumIter,
)]
#[serde(rename_all = "kebab-case")] // Maps to kebab-case for url query params
#[sqlx(type_name = "text")] // Maps to standard TEXT in Postgres, not a custom enum type
/// IMPORTANT: This enum is coupled to the `app.source_names` table in the database.
/// If you add a new variant here, you MUST create a migration to insert the corresponding
/// string value into the `app.source_names` table. Otherwise, inserting events with
/// the new source will fail due to foreign key constraints.
pub enum EventSource {
    AeronautBrewing,
    AmericanRepertoryTheater,
    ArtsAtTheArmory,
    BostonSwingCentral,
    BostonShowsOrg,
    BrattleTheatre,
    CentralSquareTheater,
    CityOfCambridge,
    FirstParishInCambridge,
    GrolierPoetryBookShop,
    HarvardArtMuseums,
    HarvardBookStore,
    ImageUpload,
    LamplighterBrewing,
    PorterSquareBooks,
    PorticoBrewing,
    SandersTheatre,
    SomervilleTheatre,
    TheComedyStudio,
    TheDanceComplex,
    TheLilyPad,
    TheMiddleEast,
    UserSubmitted,
}

impl fmt::Display for EventSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EventSource::AeronautBrewing => write!(f, "Aeronaut Brewing"),
            EventSource::AmericanRepertoryTheater => write!(f, "American Repertory Theater"),
            EventSource::ArtsAtTheArmory => write!(f, "Arts at the Armory"),
            EventSource::BostonSwingCentral => write!(f, "Boston Swing Central"),
            EventSource::BostonShowsOrg => write!(f, "BostonShows.org"),
            EventSource::BrattleTheatre => write!(f, "Brattle Theatre"),
            EventSource::CentralSquareTheater => write!(f, "Central Square Theater"),
            EventSource::CityOfCambridge => write!(f, "City of Cambridge"),
            EventSource::FirstParishInCambridge => write!(f, "First Parish in Cambridge"),
            EventSource::GrolierPoetryBookShop => write!(f, "Grolier Poetry Book Shop"),
            EventSource::HarvardArtMuseums => write!(f, "Harvard Art Museums"),
            EventSource::HarvardBookStore => write!(f, "Harvard Book Store"),
            EventSource::ImageUpload => write!(f, "Image Upload"),
            EventSource::LamplighterBrewing => write!(f, "Lamplighter Brewing"),
            EventSource::PorterSquareBooks => write!(f, "Porter Square Books"),
            EventSource::PorticoBrewing => write!(f, "Portico Brewing"),
            EventSource::SandersTheatre => write!(f, "Sanders Theatre"),
            EventSource::SomervilleTheatre => write!(f, "Somerville Theatre"),
            EventSource::TheComedyStudio => write!(f, "The Comedy Studio"),
            EventSource::TheDanceComplex => write!(f, "The Dance Complex"),
            EventSource::TheLilyPad => write!(f, "The Lily Pad"),
            EventSource::TheMiddleEast => write!(f, "The Middle East"),
            EventSource::UserSubmitted => write!(f, "User Submitted"),
        }
    }
}

impl EventSource {
    pub fn value(&self) -> String {
        serde_json::to_string(&self)
            .unwrap_or_else(|_| format!("\"{}\"", self.as_ref().to_lowercase()))
            .trim_matches('"')
            .to_string()
    }
}

// Support conversion for sqlx query_as! compatibility
impl From<String> for EventSource {
    fn from(s: String) -> Self {
        // Since source is deterministic and we don't have an "Other" variant,
        // we fallback to ImageUpload or could panic if we strictly trust the DB.
        // For robustness, we'll use ImageUpload as the default for unknown strings.
        EventSource::from_str(&s).unwrap_or(EventSource::ImageUpload)
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
    pub source: EventSource,
    /// External ID for idempotency/updates
    #[serde(skip, default)]
    #[schemars(skip)]
    pub external_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct SimpleEvent {
    pub id: i64,
    pub name: String,
    pub start_date: DateTime<Utc>,
    pub end_date: Option<DateTime<Utc>>,
    pub original_location: Option<String>,
    pub location_name: Option<String>,
    pub event_types: Vec<EventType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationOption {
    pub id: String,
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_url() {
        assert_eq!(sanitize_url(None), None);
        assert_eq!(sanitize_url(Some("".to_string())), None);
        assert_eq!(sanitize_url(Some("   ".to_string())), None);

        // Valid URLs
        let url = "https://example.com";
        assert!(sanitize_url(Some(url.to_string()))
            .unwrap()
            .starts_with("https://example.com"));

        // Missing scheme
        let url = "example.com";
        assert!(sanitize_url(Some(url.to_string()))
            .unwrap()
            .starts_with("https://example.com"));

        let url = "bla.com";
        assert!(sanitize_url(Some(url.to_string()))
            .unwrap()
            .starts_with("https://bla.com"));

        // Invalid URLs
        assert_eq!(sanitize_url(Some("not a url".to_string())), None);
    }
}

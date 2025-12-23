use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, JsonSchema, PartialEq, Clone, sqlx::FromRow)]
pub struct Event {
    /// The name of the event
    pub name: String,
    /// The full description of the event or content
    pub full_description: String,
    /// The date and time of the event
    pub start_date: DateTime<Utc>,
    /// The end date of the event
    pub end_date: Option<DateTime<Utc>>,
    /// The location of the event
    pub location: Option<String>,
    /// Type of event (e.g., "YardSale", "Art", "Dance", "Performance", "Food", "PersonalService", "CivicEvent", "Other")
    pub event_type: Option<String>,
    /// URL for the event, if available
    pub url: Option<String>,
    /// Confidence level of the extraction (0.0 to 1.0)
    pub confidence: f64,
    /// Database ID (optional)
    #[serde(skip, default)]
    #[schemars(skip)]
    pub id: Option<i64>,
}

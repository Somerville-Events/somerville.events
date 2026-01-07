pub mod config;
pub mod database;
pub mod features;
pub mod geocoding;
pub mod image_processing;
pub mod models;

use database::EventsRepo;

pub struct AppState {
    pub openai_api_key: String,
    pub google_maps_api_key: String,
    pub username: String,
    pub password: String,
    pub events_repo: Box<dyn EventsRepo>,
}

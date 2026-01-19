pub mod config;
pub mod database;
pub mod features;
pub mod geocoding;
pub mod image_processing;
pub mod models;

use database::EventsRepo;

#[derive(Debug, Clone)]
pub struct ImageProcessingJob {
    pub path: std::path::PathBuf,
}

pub struct AppState {
    pub openai_api_key: String,
    pub google_maps_api_key: String,
    pub username: String,
    pub password: String,
    pub events_repo: Box<dyn EventsRepo>,
    pub activitypub_sender: tokio::sync::mpsc::Sender<i64>,
    pub image_processing_sender: tokio::sync::mpsc::Sender<ImageProcessingJob>,
}

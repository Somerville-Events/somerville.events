use crate::image_processing::parse_image;
use crate::AppState;
use actix_multipart::form::{tempfile::TempFile, MultipartForm};
use actix_web::{http::header::ContentType, web, HttpResponse, Responder};
use askama::Template;
use awc::Client;
use futures_util::future;
use std::collections::{HashMap, HashSet};
use std::fs;
use uuid::Uuid;

#[derive(Template)]
#[template(path = "upload/upload.html")]
struct UploadTemplate {
    idempotency_key: String,
}

#[derive(Template)]
#[template(path = "upload/success.html")]
struct SuccessTemplate;

#[derive(Debug, MultipartForm)]
pub struct UploadForm {
    pub image: TempFile,
    pub idempotency_key: actix_multipart::form::text::Text<Uuid>,
}

pub async fn index() -> impl Responder {
    let idempotency_key = Uuid::new_v4().to_string();
    let template = UploadTemplate { idempotency_key };
    HttpResponse::Ok()
        .content_type(ContentType::html())
        .body(template.render().unwrap())
}

pub async fn save(
    state: web::Data<AppState>,
    client: web::Data<Client>,
    MultipartForm(req): MultipartForm<UploadForm>,
) -> impl Responder {
    let idempotency_key = req.idempotency_key.0;

    // Check for idempotency
    match state
        .events_repo
        .claim_idempotency_key(idempotency_key)
        .await
    {
        Ok(true) => {
            // New request, proceed
        }
        Ok(false) => {
            // Duplicate request
            log::warn!(
                "Duplicate upload attempt blocked for key: {}",
                idempotency_key
            );
            return HttpResponse::Conflict().body("Upload already in progress or completed.");
        }
        Err(e) => {
            log::error!("Database error checking idempotency: {e}");
            return HttpResponse::InternalServerError().body("Database error");
        }
    }

    let temp_dir = std::env::temp_dir();
    let extension = req
        .image
        .file_name
        .as_ref()
        .and_then(|name| std::path::Path::new(name).extension())
        .and_then(|ext| ext.to_str())
        .unwrap_or("jpg");
    let file_name = format!("{}.{}", idempotency_key, extension);
    let dest_path = temp_dir.join(&file_name);
    let dest_path_clone = dest_path.clone();

    // Offload blocking file persist to thread pool
    let persist_result = web::block(move || req.image.file.persist(&dest_path_clone)).await;

    match persist_result {
        Ok(Ok(_)) => {} // Success
        Ok(Err(e)) => {
            log::error!("Failed to persist uploaded file: {e}");
            return HttpResponse::InternalServerError().body("Failed to save uploaded file");
        }
        Err(e) => {
            log::error!("Blocking task failed: {e}");
            return HttpResponse::InternalServerError().body("Internal Server Error");
        }
    }

    let state = state.into_inner();
    let client = client.into_inner();

    actix_web::rt::spawn(async move {
        match parse_image(&dest_path, &client, &state.openai_api_key).await {
            Ok(mut events) => {
                if events.is_empty() {
                    log::info!("Image processed but no events found");
                } else {
                    hydrate_event_locations(&mut events, &client, &state.google_maps_api_key).await;

                    for event in &mut events {
                        match state.events_repo.insert(event).await {
                            Ok(id) => {
                                log::info!(
                                    "Saved event '{}' to database with id: {}",
                                    event.name,
                                    id
                                );
                            }
                            Err(e) => {
                                log::error!(
                                    "Failed to save event '{}' to database: {e:#}",
                                    event.name
                                );
                            }
                        }
                    }
                }
            }
            Err(e) => {
                log::error!("parse_image failed: {e:#}");
            }
        }

        let path_to_remove = dest_path.clone();
        if let Err(e) = web::block(move || fs::remove_file(path_to_remove)).await {
            log::warn!("Failed to remove temp file {:?}: {}", dest_path, e);
        }
    });

    HttpResponse::SeeOther()
        .insert_header((actix_web::http::header::LOCATION, "/upload-success"))
        .finish()
}

pub async fn success() -> impl Responder {
    let template = SuccessTemplate;
    HttpResponse::Ok()
        .content_type(ContentType::html())
        .body(template.render().unwrap())
}

pub async fn hydrate_event_locations(
    events: &mut [crate::models::Event],
    client: &awc::Client,
    api_key: &str,
) {
    let unique_locations: HashSet<String> = events
        .iter()
        .filter_map(|e| e.original_location.clone())
        .collect();

    let geocoding_futures = unique_locations.iter().map(|loc| async move {
        match crate::geocoding::canonicalize_address(client, loc, api_key).await {
            Ok(Some(canon)) => Some((loc.clone(), canon)),
            Ok(None) => None,
            Err(e) => {
                log::warn!("Geocoding failed for '{}': {}", loc, e);
                None
            }
        }
    });

    let geocoded_results = future::join_all(geocoding_futures).await;
    let location_map: HashMap<String, _> = geocoded_results.into_iter().flatten().collect();

    for event in events {
        if let Some(loc) = &event.original_location {
            if let Some(canon) = location_map.get(loc) {
                event.address = Some(canon.formatted_address.clone());
                event.google_place_id = Some(canon.place_id.clone());
                event.location_name = Some(canon.name.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Event;
    use chrono::Utc;

    #[actix_rt::test]
    async fn test_hydrate_event_locations() {
        let _ = env_logger::builder().is_test(true).try_init();

        let api_key = crate::config::Config::from_env()
            .google_maps_api_key
            .clone();
        let client: awc::Client = awc::Client::default();

        let mut events = vec![
            Event {
                name: "Davis Square Event".to_string(),
                original_location: Some("Davis Square".to_string()),
                description: "".to_string(),
                full_text: "".to_string(),
                start_date: Utc::now(),
                end_date: None,
                address: None,
                google_place_id: None,
                location_name: None,
                event_type: None,
                url: None,
                confidence: 1.0,
                id: None,
            },
            Event {
                name: "Somerville Theatre Event".to_string(),
                original_location: Some("Somerville Theatre".to_string()),
                description: "".to_string(),
                full_text: "".to_string(),
                start_date: Utc::now(),
                end_date: None,
                address: None,
                google_place_id: None,
                location_name: None,
                event_type: None,
                url: None,
                confidence: 1.0,
                id: None,
            },
            Event {
                name: "Unknown Place Event".to_string(),
                original_location: Some("ThisPlaceDefinitelyDoesNotExist12345".to_string()),
                description: "".to_string(),
                full_text: "".to_string(),
                start_date: Utc::now(),
                end_date: None,
                address: None,
                google_place_id: None,
                location_name: None,
                event_type: None,
                url: None,
                confidence: 1.0,
                id: None,
            },
            Event {
                name: "Another Davis Square Event".to_string(),
                original_location: Some("Davis Square".to_string()),
                description: "".to_string(),
                full_text: "".to_string(),
                start_date: Utc::now(),
                end_date: None,
                address: None,
                google_place_id: None,
                location_name: None,
                event_type: None,
                url: None,
                confidence: 1.0,
                id: None,
            },
        ];

        hydrate_event_locations(&mut events, &client, &api_key).await;

        // Verify results
        assert_eq!(events[0].location_name.as_deref(), Some("Davis Square"));
        assert_eq!(
            events[1].location_name.as_deref(),
            Some("Somerville Theatre")
        );
        assert!(events[2].address.is_none()); // Unknown place should not result in address
        assert_eq!(events[3].location_name.as_deref(), Some("Davis Square"));
    }
}

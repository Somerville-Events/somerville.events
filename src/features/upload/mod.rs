use crate::image_processing::parse_image;
use crate::AppState;
use actix_multipart::form::{tempfile::TempFile, MultipartForm};
use actix_web::{web, HttpResponse, Responder};
use askama::Template;
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
    HttpResponse::Ok().body(template.render().unwrap())
}

pub async fn save(
    state: web::Data<AppState>,
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
    let file_name = format!("{}.jpg", idempotency_key);
    let dest_path = temp_dir.join(&file_name);

    if let Err(e) = req.image.file.persist(&dest_path) {
        log::error!("Failed to persist uploaded file: {e}");
        return HttpResponse::InternalServerError().body("Failed to save uploaded file");
    }

    let state = state.into_inner();
    let dest_path_clone = dest_path.clone();

    actix_web::rt::spawn(async move {
        match parse_image(&dest_path_clone, state.client.clone(), &state.api_key).await {
            Ok(Some(event)) => match state.events_repo.insert(&event).await {
                Ok(id) => {
                    log::info!("Saved event to database with id: {}", id);
                }
                Err(e) => {
                    log::error!("Failed to save event to database: {e:#}");
                }
            },
            Ok(None) => {
                log::info!("Image processed but no event found (or missing date)");
            }
            Err(e) => {
                log::error!("parse_image failed: {e:#}");
            }
        }

        if let Err(e) = fs::remove_file(&dest_path_clone) {
            log::warn!("Failed to remove temp file {:?}: {}", dest_path_clone, e);
        }
    });

    HttpResponse::SeeOther()
        .insert_header((actix_web::http::header::LOCATION, "/upload-success"))
        .finish()
}

pub async fn success() -> impl Responder {
    let template = SuccessTemplate;
    HttpResponse::Ok().body(template.render().unwrap())
}

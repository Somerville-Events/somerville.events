use crate::features::common::{DateFormat, EventLocation, EventViewModel, SimpleEventViewModel};
use crate::AppState;
use actix_web::http::header::ContentType;
use actix_web::{web, HttpResponse, Responder};
use askama::Template;

use crate::features::view::IndexQuery;

#[derive(Template)]
#[template(path = "edit/index.html")]
struct EditListTemplate {
    events: Vec<SimpleEventViewModel>,
}

#[derive(Template)]
#[template(path = "edit/show.html")]
pub struct EditShowTemplate {
    pub event: EventViewModel,
}

pub async fn index(state: web::Data<AppState>) -> impl Responder {
    match state
        .events_repo
        .list(IndexQuery::default(), None, None)
        .await
    {
        Ok(events) => {
            let vms: Vec<SimpleEventViewModel> = events
                .iter()
                .map(|e| SimpleEventViewModel::from_event(e, DateFormat::FullDate, "/edit/event"))
                .collect();
            let template = EditListTemplate { events: vms };
            HttpResponse::Ok()
                .content_type(ContentType::html())
                .body(template.render().unwrap())
        }
        Err(e) => {
            log::error!("Failed to fetch events: {e}");
            HttpResponse::InternalServerError().body("Failed to fetch events")
        }
    }
}

pub async fn show(state: web::Data<AppState>, path: web::Path<i64>) -> impl Responder {
    let id = path.into_inner();
    match state.events_repo.get(id).await {
        Ok(Some(event)) => {
            let template = EditShowTemplate {
                event: EventViewModel::from_event(&event, DateFormat::FullDate, false),
            };
            HttpResponse::Ok()
                .content_type(ContentType::html())
                .body(template.render().unwrap())
        }
        Ok(None) => HttpResponse::NotFound().body("Event not found"),
        Err(e) => {
            log::error!("Failed to fetch event: {e}");
            HttpResponse::InternalServerError().body("Failed to fetch event")
        }
    }
}

pub async fn delete(state: web::Data<AppState>, path: web::Path<i64>) -> impl Responder {
    match state.events_repo.delete(path.into_inner()).await {
        Ok(_) => HttpResponse::SeeOther()
            .insert_header(("Location", "/edit"))
            .finish(),
        Err(e) => {
            HttpResponse::InternalServerError().body(format!("Failed to delete event: {}", e))
        }
    }
}

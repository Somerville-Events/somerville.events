use crate::features::common::{DateFormat, EventViewModel};
use crate::AppState;
use actix_web::{web, HttpResponse, Responder};
use askama::Template;

#[derive(Template)]
#[template(path = "edit/index.html")]
struct EditListTemplate {
    events: Vec<EventViewModel>,
}

pub async fn index(state: web::Data<AppState>) -> impl Responder {
    match state.events_repo.list(None, None, None).await {
        Ok(events) => {
            let vms: Vec<EventViewModel> = events
                .iter()
                .map(|e| EventViewModel::from_event(e, DateFormat::FullDate, false))
                .collect();
            let template = EditListTemplate { events: vms };
            HttpResponse::Ok().body(template.render().unwrap())
        }
        Err(e) => {
            log::error!("Failed to fetch events: {e}");
            HttpResponse::InternalServerError().body("Failed to fetch events")
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

use crate::models::Event;
use crate::AppState;
use actix_web::{web, HttpResponse, Responder};
use askama::Template;
use chrono_tz::America::New_York;

#[derive(Template)]
#[template(path = "edit/list.html")]
struct EditListTemplate {
    events: Vec<EditEventViewModel>,
}

struct EditEventViewModel {
    id: i64,
    name: String,
    start_iso: String,
    start_formatted: String,
    end_iso: String,
    end_formatted: Option<String>,
    location: String,
    description: String,
}

impl EditEventViewModel {
    fn from_event(event: &Event) -> Self {
        let start_ny = event.start_date.with_timezone(&New_York);
        let start_iso = start_ny.to_rfc3339();
        let start_formatted = start_ny.format("%A, %B %d, %Y at %I:%M %p").to_string();

        let (end_iso, end_formatted) = if let Some(end) = event.end_date {
            let end_ny = end.with_timezone(&New_York);
            (
                end_ny.to_rfc3339(),
                Some(end_ny.format("%A, %B %d, %Y at %I:%M %p").to_string()),
            )
        } else {
            (String::new(), None)
        };

        Self {
            id: event.id.unwrap_or_default(),
            name: event.name.clone(),
            start_iso,
            start_formatted,
            end_iso,
            end_formatted,
            location: event.location.clone().unwrap_or_default(),
            description: event.full_description.clone(),
        }
    }
}

pub async fn index(state: web::Data<AppState>) -> impl Responder {
    match state.events_repo.list().await {
        Ok(events) => {
            let vms: Vec<EditEventViewModel> = events.iter().map(EditEventViewModel::from_event).collect();
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


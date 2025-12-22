use crate::common_ui::{render_event_html, COMMON_STYLES};
use crate::AppState;
use actix_web::{http::header::ContentType, web, HttpResponse};

pub async fn index(state: web::Data<AppState>) -> HttpResponse {
    let events = match state.events_repo.list().await {
        Ok(events) => events,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .body(format!("Failed to load events: {}", e))
        }
    };

    let mut event_list_html = String::new();
    for event in events {
        let id = event.id.unwrap_or(-1);
        let delete_form = format!(
            r#"<form action="/event/{id}?_method=DELETE" method="post" style="margin-left: 1rem;">
                <button type="submit" class="primary" style="background-color: #d13a26; color: white; white-space: nowrap;">Delete</button>
            </form>"#
        );

        event_list_html.push_str(&render_event_html(&event, false, Some(&delete_form)));
    }

    HttpResponse::Ok()
        .content_type(ContentType::html())
        .body(format!(
            r#"<!doctype html>
            <html lang="en">
            <head>
                <meta charset="utf-8">
                <meta name="color-scheme" content="light dark">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <title>Edit Events</title>
                <style>
                    {styles}
                </style>
            </head>
            <body>
                <header>
                    <h1>Edit Events</h1>
                    <nav>
                        <a href="/">Back to Home</a>
                    </nav>
                </header>
                <section>
                    {events}
                </section>
            </body>
            </html>"#,
            styles = COMMON_STYLES,
            events = event_list_html
        ))
}

pub async fn delete(state: web::Data<AppState>, path: web::Path<i64>) -> HttpResponse {
    match state.events_repo.delete(path.into_inner()).await {
        Ok(_) => HttpResponse::SeeOther()
            .insert_header(("Location", "/edit"))
            .finish(),
        Err(e) => {
            HttpResponse::InternalServerError().body(format!("Failed to delete event: {}", e))
        }
    }
}

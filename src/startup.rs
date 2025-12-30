use crate::config::Config;
use crate::database::EventsRepo;
use crate::features;
use actix_web::dev::Server;
use actix_web::{
    error::ErrorUnauthorized,
    middleware,
    web::{self, Data},
    App, Error, HttpServer,
};
use actix_web_httpauth::{extractors::basic::BasicAuth, middleware::HttpAuthentication};
use actix_web_query_method_middleware::QueryMethod;
use sqlx::postgres::PgPoolOptions;
use std::net::TcpListener;

pub struct AppState {
    pub openai_api_key: String,
    pub google_maps_api_key: String,
    pub openai_base_url: String,
    pub google_maps_base_url: String,
    pub username: String,
    pub password: String,
    pub events_repo: Box<dyn EventsRepo>,
}

async fn basic_auth_validator(
    req: actix_web::dev::ServiceRequest,
    credentials: BasicAuth,
) -> Result<actix_web::dev::ServiceRequest, (Error, actix_web::dev::ServiceRequest)> {
    let state = req
        .app_data::<Data<AppState>>()
        .expect("AppState missing; did you register .app_data(Data::new(AppState{...}))?");

    let username = credentials.user_id();
    let password = credentials.password().unwrap_or_default();

    if username == state.username && password == state.password {
        Ok(req)
    } else {
        Err((ErrorUnauthorized("Invalid credentials"), req))
    }
}

pub async fn run(listener: TcpListener, config: Config) -> Result<Server, anyhow::Error> {
    let db_url = config.get_db_url();
    let db_connection_pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await?;

    let static_file_dir = config.static_file_dir.clone();

    let state = AppState {
        openai_api_key: config.openai_api_key.clone(),
        google_maps_api_key: config.google_maps_api_key.clone(),
        openai_base_url: config.openai_base_url.clone(),
        google_maps_base_url: config.google_maps_base_url.clone(),
        username: config.username.clone(),
        password: config.password.clone(),
        events_repo: Box::new(db_connection_pool),
    };
    let app_state = Data::new(state);

    let server = HttpServer::new(move || {
        let auth_middleware = HttpAuthentication::basic(basic_auth_validator);

        let client = awc::ClientBuilder::new()
            .timeout(std::time::Duration::from_secs(120))
            .finish();

        App::new()
            .app_data(app_state.clone())
            .app_data(Data::new(client))
            .wrap(QueryMethod::default())
            .wrap(middleware::Logger::default())
            .service(actix_files::Files::new("/static", &static_file_dir).show_files_listing())
            .route("/", web::get().to(features::view::index))
            .route("/event/{id}.ical", web::get().to(features::view::ical))
            .route("/event/{id}", web::get().to(features::view::show))
            .service(
                web::resource("/upload")
                    .wrap(auth_middleware.clone())
                    .route(web::get().to(features::upload::index))
                    .route(web::post().to(features::upload::save)),
            )
            .service(
                web::resource("/event/{id}")
                    .wrap(auth_middleware.clone())
                    .route(web::delete().to(features::edit::delete)),
            )
            .service(
                web::scope("/edit")
                    .wrap(auth_middleware)
                    .route("", web::get().to(features::edit::index)),
            )
            .route("/upload-success", web::get().to(features::upload::success))
    })
    .listen(listener)?
    .run();

    Ok(server)
}

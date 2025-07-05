use actix_web::dev::ServiceRequest;
use actix_web::web::scope;
use actix_web::{delete, error, get, post, web, App, HttpResponse, HttpServer, Responder};
use actix_web_httpauth::extractors;
use actix_web_httpauth::extractors::basic::BasicAuth;
use actix_web_httpauth::middleware::HttpAuthentication;
use chrono::Utc;
use r2d2_sqlite::SqliteConnectionManager;
use serde::Serialize;

use std::sync::Mutex;

mod auth;
mod db;

async fn validator(
    req: ServiceRequest,
    credentials: BasicAuth,
) -> Result<ServiceRequest, (actix_web::Error, ServiceRequest)> {
    let token = credentials.user_id();

    match auth::is_key_allowed_access(token) {
        Ok(true) => Ok(req),
        Ok(false) => Err((
            actix_web::error::ErrorUnauthorized("Supplied token is not authorized."),
            req,
        )),
        Err(_) => Err((actix_web::error::ErrorInternalServerError(""), req)),
    }
}

#[derive(Serialize)]
struct Temperature {
    fahrenheit: f32,
    celsius: f32,
}

struct UsageStats {
    to_fahrenheit: Mutex<u32>,
    to_celsius: Mutex<u32>,
}

#[derive(Serialize)]
struct UsageStatsResponse {
    to_fahrenheit: u32,
    to_celsius: u32,
}

#[get("/to-celsius/{fahrenheit}")]
async fn to_celsius(
    f: web::Path<f32>,
    stats: web::Data<UsageStats>,
    database: web::Data<db::Pool>,
    auth: extractors::basic::BasicAuth,
) -> impl Responder {
    let now = Utc::now();

    actix_web::rt::spawn(async move {
        let mut count = stats.to_celsius.lock().unwrap();
        *count += 1;
    });

    actix_web::rt::spawn(async move {
        let query = db::Query::RecordApiUsage {
            api_key: auth.user_id().to_string(),
            endpoint: db::ApiEndpoint::ToFahrenheit,
            called_at: now,
        };
        query.execute(database).await
    });

    let f = f.into_inner();
    let c = (f - 32.0) / 1.8;
    web::Json(Temperature {
        celsius: c,
        fahrenheit: f,
    })
}

#[get("/to-fahrenheit/{celsius}")]
async fn to_fahrenheit(
    c: web::Path<f32>,
    stats: web::Data<UsageStats>,
    database: web::Data<db::Pool>,
    auth: extractors::basic::BasicAuth,
) -> impl Responder {
    let now = Utc::now();

    actix_web::rt::spawn(async move {
        let mut count = stats.to_fahrenheit.lock().unwrap();
        *count += 1;
    });

    async {
        let query = db::Query::RecordApiUsage {
            api_key: auth.user_id().to_string(),
            endpoint: db::ApiEndpoint::ToFahrenheit,
            called_at: now,
        };
        query.execute(database).await
    }
    .await
    .map_err(error::ErrorInternalServerError)
    .unwrap();

    let c = c.into_inner();
    let f = 32.0 + (c * 1.8);
    web::Json(Temperature {
        celsius: c,
        fahrenheit: f,
    })
}

#[get("/usage-statistics")]
async fn usage_statistics(stats: web::Data<UsageStats>) -> impl Responder {
    let mut fahrenheit_count = stats.to_fahrenheit.lock().unwrap();
    let mut celsius_count = stats.to_fahrenheit.lock().unwrap();

    let response = UsageStatsResponse {
        to_fahrenheit: *fahrenheit_count,
        to_celsius: *celsius_count,
    };

    *fahrenheit_count = 0;
    *celsius_count = 0;

    web::Json(response)
}

#[post("/reset-usage-statistics")]
async fn reset_usage_statistics(stats: web::Data<UsageStats>) -> impl Responder {
    let mut fahrenheit_count = stats.to_fahrenheit.lock().unwrap();
    let mut celsius_count = stats.to_fahrenheit.lock().unwrap();

    *fahrenheit_count = 0;
    *celsius_count = 0;

    HttpResponse::NoContent()
}

#[get("/api-key")]
async fn request_api_key(database: web::Data<db::Pool>) -> actix_web::Result<impl Responder> {
    let mut api_key = auth::create_api_key();

    let api_key_ = api_key.clone();
    web::block(move || auth::store_api_key(database.clone(), api_key_))
        .await?
        .await?;

    api_key.push_str("\r\n");

    Ok(api_key)
}

#[delete("/api-key")]
async fn delete_api_key(
    auth: BasicAuth,
    database: web::Data<db::Pool>,
) -> actix_web::Result<impl Responder> {
    let token = auth.user_id().to_owned();

    web::block(|| auth::revoke_api_key(database, token))
        .await?
        .await?;

    Ok(HttpResponse::NoContent().finish())
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let manager = SqliteConnectionManager::file(db::DB_FILE);
    let db_pool = db::Pool::new(manager).unwrap();
    db::setup(db_pool.clone());

    let counts = web::Data::new(UsageStats {
        to_fahrenheit: Mutex::new(0),
        to_celsius: Mutex::new(0),
    });

    HttpServer::new(move || {
        App::new()
            .app_data(counts.clone())
            .app_data(web::Data::new(db_pool.clone()))
            .service(
                scope("/api")
                    .wrap(HttpAuthentication::basic(validator))
                    .service(to_fahrenheit)
                    .service(to_celsius),
            )
            .service(request_api_key)
            .service(delete_api_key)
            .service(usage_statistics)
            .service(reset_usage_statistics)
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}

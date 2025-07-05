use actix_web::dev::ServiceRequest;
use actix_web::{delete, error, get, post, web, HttpResponse, Responder};
use actix_web_httpauth::extractors;
use actix_web_httpauth::extractors::basic::BasicAuth;
use chrono::Utc;
use serde::Serialize;
use tracing::instrument;

use std::sync::Mutex;

pub mod auth;
pub mod db;

pub async fn validator(
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
pub struct Temperature {
    fahrenheit: f32,
    celsius: f32,
}

#[derive(Default, Debug)]
pub struct UsageStats {
    pub counters: Mutex<Counters>,
}

#[derive(Default, Debug)]
pub struct Counters {
    to_celsius: u32,
    to_fahrenheit: u32,
}

impl UsageStats {
    pub fn new() -> Self {
        UsageStats::default()
    }
}

#[derive(Serialize)]
struct UsageStatsResponse {
    to_fahrenheit: u32,
    to_celsius: u32,
}

#[get("/to-celsius/{fahrenheit}")]
#[instrument(skip(stats, database, auth))]
pub async fn to_celsius(
    f: web::Path<f32>,
    stats: web::Data<UsageStats>,
    database: web::Data<db::Pool>,
    auth: extractors::basic::BasicAuth,
) -> impl Responder {
    let now = Utc::now();

    actix_web::rt::spawn(async move {
        let mut counters = stats.counters.lock().unwrap();
        counters.to_celsius += 1;
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
#[instrument(skip(stats, database, auth))]
pub async fn to_fahrenheit(
    c: web::Path<f32>,
    stats: web::Data<UsageStats>,
    database: web::Data<db::Pool>,
    auth: extractors::basic::BasicAuth,
) -> impl Responder {
    let now = Utc::now();

    actix_web::rt::spawn(async move {
        let mut counters = stats.counters.lock().unwrap();
        counters.to_fahrenheit += 1;
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
pub async fn usage_statistics(stats: web::Data<UsageStats>) -> impl Responder {
    let mut counters = stats.counters.lock().unwrap();

    let response = UsageStatsResponse {
        to_fahrenheit: counters.to_fahrenheit,
        to_celsius: counters.to_celsius,
    };

    counters.to_fahrenheit = 0;
    counters.to_celsius = 0;

    web::Json(response)
}

#[post("/reset-usage-statistics")]
pub async fn reset_usage_statistics(stats: web::Data<UsageStats>) -> impl Responder {
    let mut counters = stats.counters.lock().unwrap();

    counters.to_fahrenheit = 0;
    counters.to_celsius = 0;

    HttpResponse::NoContent()
}

#[get("/api-key")]
#[instrument(skip(database))]
pub async fn request_api_key(database: web::Data<db::Pool>) -> actix_web::Result<impl Responder> {
    let mut api_key = auth::create_api_key();

    let api_key_ = api_key.clone();
    web::block(move || auth::store_api_key(database.clone(), api_key_))
        .await?
        .await?;

    api_key.push_str("\r\n");

    Ok(api_key)
}

#[delete("/api-key")]
pub async fn delete_api_key(
    auth: BasicAuth,
    database: web::Data<db::Pool>,
) -> actix_web::Result<impl Responder> {
    let token = auth.user_id().to_owned();

    web::block(|| auth::revoke_api_key(database, token))
        .await?
        .await?;

    Ok(HttpResponse::NoContent().finish())
}

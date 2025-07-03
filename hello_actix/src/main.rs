use actix_web::{App, HttpResponse, HttpServer, Responder, get};

#[get("/healthz")]
async fn liveness() -> impl Responder {
    HttpResponse::Ok().body("Hello, Actix!\r\n")
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let app = || App::new().service(liveness);

    HttpServer::new(app).bind("127.0.0.1:8080")?.run().await
}

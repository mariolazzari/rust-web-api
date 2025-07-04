use actix_web::{get, guard, web, App, HttpServer};
use actix_web::{post, web::Form, web::Json, HttpResponse};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Subscriber {
    name: String,
    email: String,
}

#[post("/subscribe")]
async fn subscribe(info: Form<Subscriber>) -> HttpResponse {
    println!("🎉 new subscription: {:?}", info.into_inner());
    HttpResponse::NoContent().finish()
}

async fn subscribe_with_json(info: Json<Subscriber>) -> HttpResponse {
    println!("🎉 new subscription: {:?}", info.into_inner());
    HttpResponse::NoContent().finish()
}

#[get("/")]
async fn index() -> HttpResponse {
    let webpage = r#"<!DOCTYPE html>
<head>
    <style>
        * { font-family: sans-serif;}

        form { display: table; }
        form > div { display: table-row; }
        input,label { display: table-cell; margin-bottom: 8px; }
        label { padding-right: 1rem; }
    </style>
</head>
<body>
<p>A small webapp. Subscribe for more info.</p>

<form action="/subscribe" method="POST">
    <div>
        <label for="n">Name:</label>
        <input id="n" name="name" type="text" required>
    </div>

    <div>
        <label for="e">Email:</label>
        <input id="e" name="email" type="email" required>
    </div>

    <div>
        <label>&nbsp;</label>
        <input id=submit type=submit value="Subscribe"/>
    </div>
</form>
</body>
    "#;

    HttpResponse::Ok().content_type("text/html").body(webpage)
}

#[get("/healthz")]
async fn liveness() -> &'static str {
    "ok\r\n"
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let app = || {
        App::new()
            .service(index)
            .service(
                web::resource("/subscribe")
                    .guard(guard::Header("Content-Type", "application/json"))
                    .route(web::post().to(subscribe_with_json)),
            )
            .service(subscribe)
            .service(liveness)
    };

    HttpServer::new(app).bind("127.0.0.1:8080")?.run().await
}

use std::{collections::HashMap, sync::Arc};

use actix_web::{App, HttpRequest, HttpResponse, HttpServer, Responder, route};
use awc::{Client, Connector, http::header};
use clap::Parser;
use env_logger::Env;
use log::info;
use serde_urlencoded::from_str;

#[derive(Default, Debug, Parser)]
#[clap(version, about = "A very basic http proxy server")]
struct Arguments {
    #[clap(short, long, default_value_t = 80, help = "Port to listen on")]
    port: u16,
    #[clap(short, long, default_value_t = 1, help = "Number of worker threads")]
    workers: usize,
}

trait Forward {
    fn destination(&self) -> Option<String>;
}

impl Forward for HttpRequest {
    fn destination(&self) -> Option<String> {
        let dest = self.headers()
            .get("x-transitive-dest")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        match dest {
            Some(dest) => Some(dest),
            None => {
                let query_str = self.query_string();
                info!("Query string: {}", query_str);
                let query_map = from_str::<HashMap<String, String>>(query_str).unwrap_or_default();
                query_map.get("x-transitive-dest").cloned()
            }
        }
    }
}

fn rustls_config() -> rustls::ClientConfig {
    use rustls_platform_verifier::ConfigVerifierExt as _;

    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .unwrap();

    rustls::ClientConfig::with_platform_verifier().unwrap()
}

#[route(
    "/{_:.*}",
    method = "GET",
    method = "POST",
    method = "PUT",
    method = "DELETE",
    method = "HEAD",
    method = "CONNECT",
    method = "OPTIONS",
    method = "TRACE",
    method = "PATCH"
)]
async fn forward(req: HttpRequest, _body: String) -> impl Responder {
    if let Some(dest) = req.destination() {
        info!("{} {}", req.method(), dest);

        let client_tls_config = Arc::new(rustls_config());
        let connector = Connector::new().rustls_0_23(Arc::clone(&client_tls_config));
        let client = Client::builder()
            .add_default_header((header::USER_AGENT, "transitive-rs"))
            .connector(connector)
            .finish();

        let mut res = client.get(dest).send().await.unwrap();
        // TODO: forward request headers
        // TODO: copy proxy response headers to response
        let body = res.body().await.unwrap();
        HttpResponse::Ok().body(body)
    } else {
        HttpResponse::BadRequest().body("Missing x-transitive-dest header or query parameter")
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let args = Arguments::parse();
    let port = args.port;
    let workers = args.workers;
    info!("Starting server on port {port}");
    HttpServer::new(|| App::new().service(forward))
        .bind(("0.0.0.0", port))?
        .workers(workers)
        .run()
        .await
}

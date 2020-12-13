use crate::config::Config;
use crate::controller::DeviceController;
use crate::utils::ResultExtensions;
use hyper::server::conn::AddrIncoming;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server};
use slog::{debug, info};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::oneshot::Sender;

pub struct HttpServer {
    config: Config,
    controller: Arc<dyn DeviceController>,
    shutdown_signal: Sender<()>,
}

impl HttpServer {
    pub fn new(config: &Config, controller: Arc<dyn DeviceController>) -> Arc<HttpServer> {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();

        let this = Arc::new(HttpServer {
            config: config.clone(),
            controller,
            shutdown_signal: tx,
        });

        let that = this.clone();
        let handler = make_service_fn(move |_conn| {
            let this = that.clone();
            async move {
                let this = this.clone();
                Ok::<_, hyper::Error>(service_fn(move |req| this.clone().handler(req)))
            }
        });

        info!(slog_scope::logger(), "starting_http_server"; "port" => config.http_port.unwrap());

        let server = Server::bind(&SocketAddr::from((
            [127, 0, 0, 1],
            config.http_port.unwrap(),
        )))
        .tcp_nodelay(true)
        .http1_only(true)
        .serve(handler)
        .with_graceful_shutdown(async move {
            rx.await.ok();
        });

        tokio::task::spawn(async move {
            server.await.log_failing_result("http_server_failed");
        });

        this
    }

    async fn handler(
        self: Arc<Self>,
        request: Request<Body>,
    ) -> Result<Response<Body>, hyper::Error> {
        debug!(slog_scope::logger(), "http_request"; "method" => %request.method(), "uri" => %request.uri());

        match (request.method(), request.uri().path()) {
            (&Method::GET, "/") => Ok(Response::new(Body::from("Hello world!"))),
            _ => Ok(Response::builder()
                .status(404)
                .body(Body::from("Not found"))
                .unwrap()),
        }
    }
}

use crate::config::Config;
use crate::controller::{DeviceController, DeviceId, AttributeId};
use crate::utils::{ResultExtensions, Numberish};
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server};
use slog::{debug, info, error};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::oneshot::Sender;
use std::path::Path;
use std::ffi::OsStr;
use rust_embed::RustEmbed;
use std::error::Error;
use regex::Regex;
use simple_error::{simple_error};

pub struct HttpServer {
    config: Config,
    controller: Arc<dyn DeviceController>,
    shutdown_signal: Sender<()>,
}

#[derive(RustEmbed)]
#[folder = "src/web/"]
struct Assets;

lazy_static! {
    static ref SET_DEVICE_ATTRIBUTE_REGEX: Regex = Regex::new("/api/devices/(?P<device_id>[0-9]+)/(?P<attribute_id>[0-9]+)").unwrap();
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
            [0, 0, 0, 0],
            config.http_port.unwrap(),
        )))
        .tcp_nodelay(true)
        .http1_only(true)
        .http1_keepalive(false)
        .serve(handler)
        .with_graceful_shutdown(async move {
            rx.await.ok();
        });

        tokio::task::spawn(async move {
            server.await.log_failing_result("http_server_failed");
        });

        this
    }

    fn static_response(file: &str) -> Response<Body> {
        let result = match Assets::get(file) {
            Some(v) => v,
            None => {
                return Response::builder()
                    .status(404)
                    .body(Body::from("Not Found"))
                    .unwrap();
            }
        };

        Response::builder()
            .header("Content-Type", match Path::new(file).extension().and_then(OsStr::to_str) {
                Some("html") => "text/html; charset=utf-8",
                Some("js") => "text/javascript; charset=utf-8",
                _ => "application/octet-stream",
            })
            .header("Cache-Control", "no-cache, no-store")
            .header("Connection", "close")
            .body(Body::from(result))
            .unwrap()
    }

    fn json_response(status: u16, body: serde_json::Value) -> Response<Body> {
        Response::builder()
            .status(status)
            .header("Content-Type", "application/json")
            .header("Cache-Control", "no-cache, no-store")
            .header("Connection", "close")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    fn json_error_response(err: &Box<dyn Error>) -> Response<Body> {
        Self::json_response(500, serde_json::json!({"error": format!("{:?}", err)}))
    }

    async fn handler(
        self: Arc<Self>,
        request: Request<Body>,
    ) -> Result<Response<Body>, hyper::Error> {
        debug!(slog_scope::logger(), "http_request"; "method" => %request.method(), "uri" => %request.uri());

        match (request.method(), request.uri().path()) {
            (&Method::GET, "/") => Ok(Self::static_response("index.html")),
            (&Method::GET, "/static/index.js") => Ok(Self::static_response("index.js")),
            (&Method::GET, "/api/devices") => self.devices_list().await.or_else(|e| {
                error!(slog_scope::logger(), "device_list_failed"; "error" => ?e);
                Ok(Self::json_error_response(&e))
            }),
            (&Method::POST, path) if SET_DEVICE_ATTRIBUTE_REGEX.is_match(path) => {
                return self.set_attribute(request).await.or_else(|e| {
                    error!(slog_scope::logger(), "set_attribute_failed"; "error" => ?e);
                    Ok(Self::json_error_response(&e))
                })
            },
            _ => Ok(Response::builder()
                .status(404)
                .body(Body::from("Not found"))
                .unwrap()),
        }
    }

    async fn set_attribute(self: Arc<Self>, request: Request<Body>) -> Result<Response<Body>, Box<dyn Error>> {
        let components = SET_DEVICE_ATTRIBUTE_REGEX.captures(request.uri().path()).ok_or_else(|| simple_error!("Bad URL"))?;
        let device_id = components.name("device_id").unwrap().as_str().parse_numberish::<u64>()? as DeviceId;
        let attribute_id = components.name("attribute_id").unwrap().as_str().parse_numberish::<u64>()? as AttributeId;

        let device_data_future = self.controller.as_ref().describe(device_id);

        let body : serde_json::Value = serde_json::from_slice(&hyper::body::to_bytes(request.into_body()).await?)?;

        let attribute = device_data_future.await?.attributes.into_iter().find(|a| a.id == attribute_id).ok_or_else(|| simple_error!("Unknown attribute id {}", attribute_id))?;
        let attribute_value = match body["value"] {
            serde_json::Value::Null => attribute.attribute_type.parse(body["value_text"].as_str().ok_or_else(|| simple_error!("Unknown input format - no value or value_text!"))?)?,
            _ => attribute.attribute_type.parse_json(&body["value"])?,
        };

        self.controller.set(device_id, attribute_id, &attribute_value).await?;

        Ok(Self::json_response(200, serde_json::json!({})))
    }

    async fn devices_list(self: Arc<Self>) -> Result<Response<Body>, Box<dyn Error>> {
        let device_futures : Vec<_> = self.controller.list().await?
            .into_iter()
            .map(|d| self.controller.describe(d.id))
            .collect();
        let mut devices = Vec::with_capacity(device_futures.len());
        for f in device_futures {
            devices.push(f.await?)
        };

        Ok(Self::json_response(200, serde_json::json!({"devices": devices})))
    }
}

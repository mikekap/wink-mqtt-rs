#![feature(async_closure)]

#[macro_use]
extern crate lazy_static;

use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::io::{BufReader, Read};

use crate::config::Config;
use crate::http::HttpServer;
use clap::{crate_version, App, Arg, ArgMatches, ErrorKind};
use rumqttc::MqttOptions;
use simple_error::bail;
use slog::{info, o, trace, Drain};
use slog_scope::GlobalLoggerGuard;
use slog_term;
use std::sync::Arc;
use tokio::{self, time::Duration};
use url::Url;

mod config;
mod controller;
mod converter;
mod http;
mod syncer;
mod utils;

fn init_logger(args: &ArgMatches) -> GlobalLoggerGuard {
    let min_log_level = match args.occurrences_of("verbose") {
        0 => slog::Level::Info,
        1 => slog::Level::Debug,
        2 | _ => slog::Level::Trace,
    };
    let decorator = slog_term::PlainSyncDecorator::new(std::io::stderr());
    let drain = slog_term::FullFormat::new(decorator)
        .build()
        .filter_level(min_log_level)
        .fuse();
    let logger = slog::Logger::root(drain, o!());
    info!(logger, "init_logger"; "min_log_level" => format!("{:?}", min_log_level));

    let scope_guard = slog_scope::set_global_logger(logger);

    slog_stdlog::init().unwrap();

    scope_guard
}

fn init_mqtt_client(a: &ArgMatches) -> Result<Option<MqttOptions>, Box<dyn Error>> {
    let mqtt_uri = match a.value_of("mqtt-uri") {
        Some(v) => v,
        None => return Ok(None),
    };
    trace!(slog_scope::logger(), "parse_uri"; "uri" => mqtt_uri);
    let mqtt_uri = if !mqtt_uri.starts_with("mqtt://") && !mqtt_uri.starts_with("mqtts://") {
        format!("mqtt://{}", mqtt_uri)
    } else {
        mqtt_uri.to_string()
    };

    let parsed = Url::parse(&mqtt_uri)?;

    if !["mqtt", "mqtts", ""].contains(&parsed.scheme()) {
        bail!("Invalid mqtt url: {}", mqtt_uri)
    }

    let host = match parsed.host() {
        Some(host) => host.to_string(),
        None => bail!("No host in mqtt uri: {}", mqtt_uri),
    };

    let port = parsed.port().unwrap_or(1883);

    let hash_query: HashMap<_, _> = parsed.query_pairs().into_owned().collect();

    let client_id = hash_query
        .get("client_id")
        .map(|x| x.as_str())
        .unwrap_or("wink-mqtt-rs");
    if client_id.starts_with(" ") {
        bail!("Invalid client id: {}", client_id)
    }

    let mut options = MqttOptions::new(client_id, host, port);

    if parsed.username() != "" {
        let password = parsed.password().unwrap_or("");
        options.set_credentials(parsed.username(), password);
    }

    if "mqtts" == parsed.scheme() {
        if let Some(cert) = hash_query.get("tls_root_cert") {
            let mut pem = BufReader::new(fs::File::open(cert)?);
            let mut data = Vec::new();
            pem.read_to_end(&mut data)?;
            options.set_ca(data);
            ()
        } else {
            bail!("Missing root cert for mqtts")
        }
    }

    Ok(Some(options))
}

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn Error>> {
    let matches = App::new("wink-mqtt-rs")
        .version(crate_version!())
        .author("Mike Kaplinskiy <mike.kaplinskiy@gmail.com>")
        .about("wink hub v1 mqtt bridge")
        .arg(Arg::new("verbose")
            .short('v')
            .multiple(true)
            .takes_value(false)
            .about("verbosity level"))
        .arg(Arg::new("resync-interval")
            .short('i')
            .required(false)
            .takes_value(true)
            .about("how frequently to check if the light changed state (e.g. via Wink or other external means)")
            .default_value("10000"))
        .arg(Arg::new("mqtt-uri")
            .short('s')
            .required(false)
            .takes_value(true)
            .about("mqtt server to connect to. Should be of the form mqtt[s]://[username:password@]host:port/[?connection_options]"))
        .arg(Arg::new("topic-prefix")
            .short('t')
            .about("Prefix for the mqtt topic used for device status/control")
            .default_value("home/wink/"))
        .arg(Arg::new("discovery-prefix")
            .short('d')
            .takes_value(true)
            .about("Prefix (applied independently of --topic-prefix) to broadcast mqtt discovery information (see https://www.home-assistant.io/docs/mqtt/discovery/)")
            .required(false))
        .arg(Arg::new("discovery-listen-topic")
            .required(false)
            .takes_value(true)
            .long("--discovery-listen-topic")
            .about("Topic to listen to in order to (re)broadcast discovery information. Only applies if --discovery-prefix is set.")
            .default_value("homeassistant/status"))
        .arg(Arg::new("http-port")
            .required(false)
            .takes_value(true)
            .long("--http-port")
            .about("If you'd like an http server, this is the port on which to start it")
            .default_value("3000"))
        .get_matches();

    let resync_interval: u64 = matches
        .value_of_t("resync-interval")
        .unwrap_or_else(|e| e.exit());

    let http_port = matches
        .value_of_t::<u16>("http-port")
        .map(|t| Some(t))
        .unwrap_or_else(|e| {
            if e.kind == ErrorKind::ArgumentNotFound {
                None
            } else {
                e.exit()
            }
        });

    let _guard = init_logger(&matches);

    info!(slog_scope::logger(), "starting"; "version" => crate_version!());

    let options = init_mqtt_client(&matches)?;
    let config = Config::new(
        options,
        matches.value_of("topic-prefix"),
        matches.value_of("discovery-prefix"),
        matches.value_of("discovery-listen-topic"),
        resync_interval,
        http_port,
    );
    #[cfg(target_arch = "arm")]
    let controller = controller::AprontestController::new();
    #[cfg(not(target_arch = "arm"))]
    let controller = controller::FakeController::new();
    let controller = Arc::new(controller);

    let syncer = if config.has_mqtt() {
        Some(syncer::DeviceSyncer::new(&config, controller.clone()))
    } else {
        None
    };
    let _http = if http_port.is_some() {
        Some(HttpServer::new(&config, controller.clone(), syncer))
    } else {
        None
    };

    loop {
        tokio::time::delay_for(Duration::from_secs(0xfffff)).await;
    }
}

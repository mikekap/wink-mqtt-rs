#![feature(async_closure)]

#[macro_use]
extern crate lazy_static;

use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::io::BufReader;

use clap::{App, Arg, ArgMatches, crate_version};
use mqtt_async_client::client::{
    Client, Publish, QoS, Subscribe, SubscribeTopic, Unsubscribe, UnsubscribeTopic,
};
use rustls;
use rustls_native_certs;
use simple_error::bail;
use slog::{Drain, info, LevelFilter, o, trace, warn};
use slog_scope::{GlobalLoggerGuard};
use slog_term;
use tokio::{
    self,
    time::{Duration, timeout},
};
use url::Url;

mod controller;
mod syncer;

fn init_logger(args: &ArgMatches) -> GlobalLoggerGuard {
    let min_log_level = match args.occurrences_of("verbose") {
        0 => slog::Level::Info,
        1 => slog::Level::Debug,
        2 | _ => slog::Level::Trace,
    };
    let decorator = slog_term::PlainSyncDecorator::new(std::io::stderr());
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = LevelFilter::new(drain, min_log_level).fuse();
    let logger = slog::Logger::root(drain, o!());
    info!(logger, "init_logger"; "min_log_level" => format!("{:?}", min_log_level));

    slog_stdlog::init().unwrap();

    slog_scope::set_global_logger(logger)
}

fn init_mqtt_client(a: &ArgMatches) -> Result<Client, Box<dyn Error>> {
    let mqtt_uri = a.value_of("mqtt-uri").unwrap();
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

    let mut builder = Client::builder();

    let builder = match parsed.host() {
        Some(host) => builder.set_host(host.to_string()),
        None => bail!("No host in mqtt uri: {}", mqtt_uri),
    };

    if let Some(port) = parsed.port() {
        builder.set_port(port);
    }

    if parsed.username() != "" {
        builder.set_username(Some(parsed.username().to_string()));
    }
    if let Some(v) = parsed.password() {
        builder.set_password(Some(Vec::from(v)));
    }

    let hash_query: HashMap<_, _> = parsed.query_pairs().into_owned().collect();

    if "mqtts" == parsed.scheme() {
        let mut ssl_config = rustls::ClientConfig::new();
        match hash_query.get("tls_root_cert") {
            Some(v) => {
                let mut pem = BufReader::new(fs::File::open(v)?);
                match ssl_config.root_store.add_pem_file(&mut pem) {
                    Ok(_) => {}
                    Err(_) => bail!("Failed to load root cert"),
                };
            }
            None => {
                match rustls_native_certs::load_native_certs() {
                    Ok(cert_store) => ssl_config.root_store = cert_store,
                    Err((Some(partial_certs), err)) => {
                        warn!(slog_scope::logger(), "native_cert_failure"; "err" => format!("{}", err));
                        ssl_config.root_store = partial_certs
                    },
                    Err((_, err)) => {
                        bail!("Failed to load any SSL certificates. Check that ca-certificates exists. Error: {}", err)
                    }
                };
            }
        }
        builder.set_tls_client_config(ssl_config);
    }

    if let Some(v) = hash_query.get("client_id") {
        builder.set_client_id(Some(v.to_string()));
    }

    info!(slog_scope::logger(), "opening_client"; "host" => parsed.host().unwrap().to_string(), "port" => parsed.port());
    match builder.build() {
        Ok(b) => Ok(b),
        Err(e) => Err(e.into()),
    }
}

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn Error>> {
    let matches = App::new("wink-mqtt-rs")
        .version(crate_version!())
        .author("Mike Kaplinskiy <mike.kaplinskiy@gmail.com>")
        .about("wink hub v1 mqtt bridge")
        .arg(Arg::with_name("verbose")
            .short('v')
            .multiple(true)
            .about("verbosity level"))
        .arg(Arg::with_name("mqtt-uri")
            .short('s')
            .about("mqtt server to connect to. Should be of the form mqtt[s]://[username:password@]host:port/[?connection_options]"))
        .arg(Arg::with_name("topic-prefix")
            .short('t')
            .about("Prefix for the mqtt topic used for device status/control")
            .default_value("home/wink/"))
        .arg(Arg::with_name("discovery-prefix")
            .short('d')
            .about("Prefix (applied independently of --topic-prefix) to broadcast mqtt discovery information (see https://www.home-assistant.io/docs/mqtt/discovery/)")
            .default_value(""))
        .arg(Arg::with_name("discovery-listen-topic")
            .required(false)
            .long("--discovery-listen-topic")
            .about("Topic to listen to in order to (re)broadcast discovery information. Only applies if --discovery-prefix is set.")
            .default_value("homeassistant/status"))
        .get_matches();

    let _guard = init_logger(&matches);

    let mut client = init_mqtt_client(&matches)?;
    let mut controller = controller::AprontestController::new();
    let mut syncer = syncer::DeviceSyncer::new(&mut controller, &mut client, matches.value_of("topic-prefix").unwrap());
    syncer.start();

    Ok(())
}

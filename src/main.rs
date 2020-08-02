#![feature(async_closure)]

#[macro_use]
extern crate lazy_static;

use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::io::{BufReader, Read};

use clap::{crate_version, App, Arg, ArgMatches};
use rumqttc::MqttOptions;
use simple_error::bail;
use slog::{info, o, trace, Drain, LevelFilter};
use slog_scope::GlobalLoggerGuard;
use slog_term;
use tokio::{self, time::Duration};
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

fn init_mqtt_client(a: &ArgMatches) -> Result<MqttOptions, Box<dyn Error>> {
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

    info!(slog_scope::logger(), "opening_client"; "host" => options.broker_address().0, "port" => options.broker_address().1);
    Ok(options)
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

    let options = init_mqtt_client(&matches)?;
    let controller = controller::AprontestController::new();
    let _ = syncer::DeviceSyncer::new(
        options,
        matches.value_of("topic-prefix").unwrap(),
        matches.value_of("discovery-prefix").unwrap(),
        controller,
    )
    .await;
    loop {
        tokio::time::delay_for(Duration::from_secs(0xffffffff)).await;
    }
}

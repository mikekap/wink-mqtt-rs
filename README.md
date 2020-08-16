# wink-mqtt-rs

This is a rust implementation of an MQTT daemon to run on the wink hub (1, not 2). This turns your wink hub into a mqtt "radio" that can control attached devices.

This version also includes support for [home-assistant autodiscovery](https://www.home-assistant.io/docs/mqtt/discovery/) so you don't have to configure your devices by hand. 

## Installation

First you need to have root on your wink hub. [This tutorial](todo) has instructions on how to root your hub. If you, like me, don't want to buy a UART->USB dongle, you can use the UART port on a Raspberry PI (since both UARTs are 3.3V). This worked for me, but I make no guarantees otherwise.

Once you have root on your hub, run the following command from your root shell:

```bash
curl -L --cacert /etc/ssl/certs/ca-certificates.crt https://github.com/mikekap/wink-mqtt-rs/releases/latest/download/wink-mqtt-rs.tar.gz | tar -C / -zxvf - && /opt/wink-mqtt-rs/setup.sh
```

and follow the prompts! 

You can configure more options by editing the `/opt/wink-mqtt-rs/config` file after installation (it's just the CLI args to the process).

## Usage
```bash
wink-mqtt-rs 0.1.2
Mike Kaplinskiy <mike.kaplinskiy@gmail.com>
wink hub v1 mqtt bridge

USAGE:
    wink-mqtt-rs [FLAGS] [OPTIONS] -s <mqtt-uri>

FLAGS:
    -h, --help       Prints help information
    -v               verbosity level
    -V, --version    Prints version information

OPTIONS:
        --discovery-listen-topic <discovery-listen-topic>
            Topic to listen to in order to (re)broadcast discovery information. Only applies if --discovery-prefix is
            set. [default: homeassistant/status]
    -d <discovery-prefix>
            Prefix (applied independently of --topic-prefix) to broadcast mqtt discovery information (see
            https://www.home-assistant.io/docs/mqtt/discovery/)
    -s <mqtt-uri>
            mqtt server to connect to. Should be of the form
            mqtt[s]://[username:password@]host:port/[?connection_options]
    -i <resync-interval>
            how frequently to check if the light changed state (e.g. via Wink or other external means) [default: 10000]

    -t <topic-prefix>
            Prefix for the mqtt topic used for device status/control [default: home/wink/]
```

The default setup above will read these options from `/opt/wink-mqtt-rs/config` . You can also see this by running `cargo +nightly run`.

### MQTT Messages

If you have a topic prefix of `home/wink/`, and a device id with `1` named `Fan`:
 - You can *receive* messages on `home/wink/1/status` with the contents, in json:
 ```json
 {"On_Off": 0}
 ```
   The keys/values match the attributes that `aprontest` reports.
 - You can *send* messages on `home/wink/1/set` with the same style json blob as above to set values on the attribute.
 - You can *send* messages on `home/wink/1/7/set` with a value to set for a particular attribute. Note that the attribute id here is a integer as reported by `aprontest`. Prefer the above version in code that does not listen to MQTT Discovery.

Messages on the discovery topic follow a format that works with home assistant MQTT discovery. For details, see [converter.rs](https://github.com/mikekap/wink-mqtt-rs/blob/master/src/converter.rs).

## Known Issues
 - Groups are not exposed.
 - This has only been tested with Z-Wave devices. It may not work in other scenarios.
   This is very easy to fix, so please file issues with the output of `aprontest -l` and `aprontest -l -m <device_id>` from your Wink!
 - Does not send device details to Home Assistant, even though the data exists. PRs welcome!
 - `mqtts` support is untested.
 - Could be smarter about tailing log files (like wink-mqtt), but a resync every 10 seconds seems fine.
 - Don't publish status if nothing changed. Easy fix, if necessary.

## Uninstalling
To uninstall, run:
```bash
/opt/wink-mqtt-rs/uninstall.sh
```

## Developing
This is a vanilla Rust project - just use cargo nightly.

### Running Locally
You can run wink-mqtt-rs locally, though obviously it won't control any lights. There's a fake implementation of aprontest for local use
that mostly just pretends whatever you do to it succeeded.

### Running on the Wink
Use `./release/build_release.sh` to build a ARM binary (requires docker). Then you can:
```bash
scp target/armv5te-unknown-linux-musleabi/release/wink-mqtt-rs root@wink:/opt/wink-mqtt-rs/
ssh root@wink-mqtt-rs /etc/rc.d/init.d/wink-mqtt-rs restart
```

## License

See LICENSE.md

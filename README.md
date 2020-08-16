# wink-mqtt-rs

This is a rust implementation of an MQTT daemon to run on the wink hub (1, not 2). This turns your wink hub into a mqtt "radio" that can control attached devices.

## Installation

First you need to have root on your wink hub. [This tutorial](todo) has instructions on how to root your hub.

Once you have root on your hub, run the following command from your root shell:

```bash
curl -L --cacert /etc/ssl/certs/ca-certificates.crt https://github.com/mikekap/wink-mqtt-rs/releases/latest/download/wink-mqtt-rs.tar.gz | tar -C / -zxvf - && /opt/wink-mqtt-rs/setup.sh
```

and follow the prompts! 

You can configure more options by editing the `/opt/wink-mqtt-rs/config` file after installation (it's just the CLI args to the process).

## Usage
```bash
wink-mqtt-rs 0.1.1
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

The default setup in release/ will read these options from /opt/wink-mqtt-rs/config .

## Known Issues
 - Groups are not exposed.
 - This has only been test with Z-Wave devices. Somewhat unlikely to work with others.
   This is very easy to fix, so please file issues with the output of `aprontest -l` and `aprontest -l -m <device_id>`.
 - Does not send device info to Home Assistant, even though the data exists.
 - `mqtts` support is untested.
 - Could be smarter about tailing log files (like wink-mqtt), but a resync every 10 seconds seems fine.

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

# wink-mqtt-rs

This is a rust implementation of an MQTT daemon to run on the wink hub (1, not 2). This turns your wink hub into a mqtt "radio" that can control attached devices.

## Installation

TODO. See release folder for a cross-compilation + files you'll want to put in place.

## Usage
```bash
wink-mqtt-rs 0.1.0
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
    -t <topic-prefix>
            Prefix for the mqtt topic used for device status/control [default: home/wink/]
```

The default setup in release/ will read these options from /opt/wink-mqtt-rs/config .

## License

See LICENSE.md
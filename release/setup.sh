#!/bin/bash
set -eo pipefail

if [[ -f /opt/wink-mqtt-rs/config ]]; then
    echo "Updating wink-mqtt-rs - skipping config...";
    /etc/rc.d/init.d/wink-mqtt-rs restart
    exit 0;
fi

echo "Welcome to the wink-mqtt-rs installation! Please fill in the prompts: "
echo -n "Address of your MQTT server: "
read MQTT_SERVER

echo -n "Topic prefix for state & action messages (enter for default): "
read TOPIC_PREFIX

echo -n "Discovery topic prefix for broadcast (enter to disable): "
read DISCOVERY_PREFIX

echo "-s mqtt://$MQTT_SERVER" > /opt/wink-mqtt-rs/config
if [[ ! -z $TOPIC_PREFIX ]]; then
    echo "-t '$TOPIC_PREFIX'" >> /opt/wink-mqtt-rs/config
fi
if [[ ! -z $DISCOVERY_PREFIX ]]; then
    echo "-d '$DISCOVERY_PREFIX'" >> /opt/wink-mqtt-rs/config
fi

cp /opt/wink-mqtt-rs/mqtt.sh /etc/rc.d/init.d/wink-mqtt-rs
chmod +x /etc/rc.d/init.d/wink-mqtt-rs

cp /etc/monitrc /etc/monitrc.bak
cat /opt/wink-mqtt-rs/monit >> /etc/monitrc
monit reload

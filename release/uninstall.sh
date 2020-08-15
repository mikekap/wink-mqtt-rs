#!/bin/bash
set -exo pipefail

echo "Removing wink-mqtt-rs..."

# Monit?
cp /etc/monitrc.bak /etc/monitrc
monit reload

/etc/rc.d/init.d/wink-mqtt-rs stop || true
rm /etc/rc.d/init.d/wink-mqtt-rs || true
rm /var/log/wink-mqtt-rs.* || true
rm /var/run/wink-mqtt-rs.pid || true
rm -rf /opt/wink-mqtt-rs || true

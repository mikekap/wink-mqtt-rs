#!/usr/bin/env bash
set -eo pipefail

export PATH="$PATH:/sbin:/bin:/usr/sbin:/usr/bin"

case "${1}" in
  start)
    CMD=$(cat /opt/wink-mqtt-rs/config | xargs echo /opt/wink-mqtt-rs/wink-mqtt-rs)

    echo "Starting wink-mqtt-rs..."
    start-stop-daemon --background --pidfile=/var/run/wink-mqtt-rs.pid --make-pidfile --startas /bin/bash -S -- -c "exec $CMD >/var/log/wink-mqtt-rs.log 2>&1";
    ;;

  stop)
    echo "Stopping wink-mqtt-rs..."
    start-stop-daemon --pidfile=/var/run/wink-mqtt-rs.pid -K
    rm /var/run/wink-mqtt-rs.pid
    ;;

  restart)
    ${0} stop
    sleep 1
    ${0} start
    ;;

  *)
    echo "Usage: $0 [start|stop|restart]"
    ;;
esac

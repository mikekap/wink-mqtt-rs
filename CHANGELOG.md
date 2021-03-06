## 0.2.2
 - Include a device stanza in the autodiscovery payload to create devices (not just entities) in HA.

## 0.2.1
 - Added support for a UINT64 attribute type. (Closes #34)
 - Broadcast discovery for all devices, even if one in the middle fails.
 - Omit attributes from devices with unknown types.

## 0.2.0
 - Add HTTP server running on port 3000. You can toggle stuff on it!
 - Crash when the message queue is full, in the hopes that the supervisor restarts us. rumqttc seems to have issues reconnecting sometimes.
 - Broadcast discovery info every time we connect to the mqtt server.

## 0.1.5
 - Fix STRING types in the JSON mqtt set endpoint as well.

## 0.1.4
 - Add support for older version of firmware that doesn't report Gang data.
 - Add support for STRING-based properties & discovery of STRING-based On_Off switches.
 - Add support for UInt16/32-based properties in parsing.
 - Upgrade the mqtt client to attempt fixing reconnection issues (disconnects from the MQTT server would sometimes fail to retry connecting).

## 0.1.3
 - Report errors more readably better when things fail

## 0.1.2
 - Remove silly Dockerfile in bundle
 - Add version to startup log
 - Avoid burning the CPU if the MQTT server is unreachable

## 0.1.1
 - Add resync interval
 - Add some better log messages

## 0.1.0
 - First release, not in github

#!/usr/bin/env bash
set -exo pipefail

docker build -t wink_builder release
docker run -it --rm -v `pwd`:/work wink_builder /bin/bash -c "cd /work; cargo build --release --target armv5te-unknown-linux-musleabi"

(rm -rf target/pkg || true) && mkdir -p target/pkg/opt/wink-mqtt-rs/
cp release/* target/pkg/opt/wink-mqtt-rs/
rm target/pkg/opt/wink-mqtt-rs/build_release.sh
cp target/armv5te-unknown-linux-musleabi/release/wink-mqtt-rs target/pkg/opt/wink-mqtt-rs/
tar -zcvf target/wink-mqtt-rs.tar.gz -C target/pkg .

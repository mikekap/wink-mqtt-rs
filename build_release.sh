#!/usr/bin/env bash
set -exo pipefail

docker build -t wink_builder .
docker run -it --rm -v `pwd`:/work wink_builder /bin/bash -c "cd /work; cargo build --release --target armv5te-unknown-linux-musleabi"

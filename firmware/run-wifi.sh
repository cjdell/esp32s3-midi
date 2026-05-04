#!/usr/bin/env bash
set -euo pipefail

cat keyboard.html | gzip > keyboard.html.gz

cargo run -r --bin midi_wifi

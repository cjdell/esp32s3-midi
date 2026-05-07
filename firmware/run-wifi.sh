#!/usr/bin/env bash
set -euo pipefail

source ~/export-esp.sh

cat ../web/keyboard.html | gzip > keyboard.html.gz

cargo run -r --bin midi_wifi

#!/usr/bin/env bash
set -euo pipefail

source ~/export-esp.sh

cargo run -r --bin midi_async

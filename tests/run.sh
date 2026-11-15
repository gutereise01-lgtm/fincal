#!/usr/bin/env bash
# Rust 엔진 결과를 원본 엔진(JS 예비 엔진, Python 엔진)과 무작위 입력으로 대조한다. 먼저 ../build.sh 를 실행할 것.
set -euo pipefail
cd "$(dirname "$0")"
node compare.js "${1:-1500}" | grep -v '^ok' || true
node py_cases.js "${2:-150}"
python3 py_compare.py

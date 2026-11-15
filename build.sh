#!/usr/bin/env bash
# Rust 엔진을 WebAssembly로 빌드해 docs/pkg 에 넣고, 서비스 워커 버전을 갱신한다.
# 준비물: rustup target add wasm32-unknown-unknown && cargo install wasm-bindgen-cli --version 0.2.100
set -euo pipefail
cd "$(dirname "$0")"
( cd engine && cargo build --release --target wasm32-unknown-unknown )
WASM=engine/target/wasm32-unknown-unknown/release/fincalc_engine.wasm
wasm-bindgen --target no-modules --no-typescript --out-dir docs/pkg "$WASM"
wasm-bindgen --target nodejs --no-typescript --out-dir tests/pkg "$WASM"   # 검증용
# 서비스 워커 버전 = 앱 파일 내용 해시 (파일이 바뀌면 아이폰에서 '새 버전' 안내가 뜬다)
HASH=$(cd docs && find . -type f ! -name sw.js ! -name '.nojekyll' -print0 | sort -z | xargs -0 sha256sum | sha256sum | cut -c1-12)
sed -i "s/^const VERSION = '.*';/const VERSION = '${HASH}';/" docs/sw.js
echo "built: $(wc -c < docs/pkg/fincalc_engine_bg.wasm) bytes wasm, version ${HASH}"

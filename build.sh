#!/bin/sh
# Build the plugin package: tests, wasm32 core module, component, zip.
set -e
cd "$(dirname "$0")/plugin"
~/.cargo/bin/cargo test --quiet
RUSTFLAGS="--remap-path-prefix=$HOME=~" ~/.cargo/bin/cargo build --release --target wasm32-unknown-unknown
~/.cargo/bin/wasm-tools component new target/wasm32-unknown-unknown/release/swatch.wasm -o ../package/filament-swatch/plugin.wasm
cd ../package
rm -f filament-swatch-*.zip
VERSION=$(python3 -c "import json;print(json.load(open('filament-swatch/plugin.json'))['version'])")
python3 -c "import zipfile; z=zipfile.ZipFile('filament-swatch-$VERSION.zip','w',zipfile.ZIP_DEFLATED); [z.write('filament-swatch/'+f) for f in ('plugin.json','plugin.wasm')]"
echo "package/filament-swatch-$VERSION.zip"

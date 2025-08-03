#!/bin/bash

set -euo pipefail

build=$(cargo build --release --bin touchportal-youtube-live -q --message-format=json)

exe=$(jq -r 'select(.reason == "compiler-artifact" and .target.name == "touchportal-youtube-live").executable' <<<"$build")
out_dir="$(dirname "$(jq -r 'select(.reason == "build-script-executed") | select(.package_id | contains("plugin#")).out_dir' <<<"$build")")"/out/
entry_tp="$out_dir"/entry.tp

tmp=$(mktemp -d)
mkdir "$tmp/YouTubeLive"
cp "$exe" "$entry_tp" "$tmp/YouTubeLive"
here=$(pwd)
pushd "$tmp"
zip -r "YouTubeLivePlugin.tpp" "YouTubeLive"
rsync -a YouTubeLive/ ~/.config/TouchPortal/plugins/YouTubeLive/
cp "YouTubeLivePlugin.tpp" "$here"
popd
rm -r "$tmp"

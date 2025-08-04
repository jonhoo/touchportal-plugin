#!/bin/bash

set -euo pipefail

plugin_name=YouTubeLive
crate_binary=touchportal-youtube-live

build=$(cargo build --release --bin "$crate_binary" -q --message-format=json)
exe=$(jq -r "select(.reason == \"compiler-artifact\" and .target.name == \"$crate_binary\").executable" <<<"$build")
out_dir="$(dirname "$(jq -r "select(.reason == \"build-script-executed\") | select(.package_id | contains(\"#$crate_binary@\")).out_dir" <<<"$build")")"/out/
entry_tp="$out_dir"/entry.tp

tmp=$(mktemp -d)
mkdir "$tmp/$plugin_name"
cp "$exe" "$entry_tp" "$tmp/$plugin_name"
here=$(pwd)
pushd "$tmp"
zip -r "$plugin_name.tpp" "$plugin_name"
rsync -a "$plugin_name/" ~/.config/TouchPortal/plugins/"$plugin_name"/
cp "$plugin_name.tpp" "$here"
popd
rm -r "$tmp"

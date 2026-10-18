#!/bin/bash
# 56-kitty — install kitty from its upstream release bundle.
# Installs: /opt/kitty.app, /usr/local/bin/kitty, /usr/local/bin/kitten
#
# Upstream rather than Ubuntu's package, which lags behind: the render-check
# skill should see the same kitty as the maintainer's host. The checksum is
# the one GitHub lists for the release asset.
set -euo pipefail

version=0.47.1
sha256=9348ea9493ade88955dfdf4d69008cc4a4fb1b752ac8f38d471bc47188ddba04
bundle=/tmp/kitty-${version}.txz

echo "==> Installing kitty ${version}"
curl -fsSL -o "$bundle" \
    "https://github.com/kovidgoyal/kitty/releases/download/v${version}/kitty-${version}-x86_64.txz"
echo "${sha256}  ${bundle}" | sha256sum -c -
mkdir -p /opt/kitty.app
tar -xJf "$bundle" -C /opt/kitty.app
rm "$bundle"
ln -sf /opt/kitty.app/bin/kitty /usr/local/bin/kitty
ln -sf /opt/kitty.app/bin/kitten /usr/local/bin/kitten

/usr/local/bin/kitty --version
/usr/local/bin/kitten --version

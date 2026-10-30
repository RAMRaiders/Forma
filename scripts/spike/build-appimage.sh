#!/bin/sh
# Builds the spike AppImage for one architecture into dist-spike/. Needs Docker with BuildKit;
# arm64 on an x86 machine also needs QEMU binfmt and takes a long time (an arm64 host is faster).
set -eu
arch="${1:-amd64}"
case "$arch" in amd64 | arm64) ;; *) echo "usage: $0 amd64|arm64" >&2; exit 2 ;; esac
cd "$(dirname "$0")/../.."
docker buildx build --platform "linux/$arch" -f scripts/spike/Dockerfile \
  --output "type=local,dest=dist-spike/$arch" .
ls -lh "dist-spike/$arch"

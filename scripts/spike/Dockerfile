# Builds a RomDex AppImage for the Batocera spike (#39). WebKitGTK is bundled inside it, since
# Batocera does not ship it. Not a release build: nothing here is signed, versioned or published.
#
#   scripts/spike/build-appimage.sh amd64    # or arm64
#
# Debian bookworm's glibc (2.36) is older than Batocera's, so the AppImage runs there.
FROM debian:bookworm AS build

RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential ca-certificates curl file git pkg-config xz-utils \
      libwebkit2gtk-4.1-dev libssl-dev libxdo-dev librsvg2-dev \
      libayatana-appindicator3-dev libudev-dev \
    && rm -rf /var/lib/apt/lists/*

# Node 24, as .nvmrc pins.
RUN arch="$(dpkg --print-architecture | sed 's/amd64/x64/')" \
    && curl -fsSL "https://nodejs.org/dist/latest-v24.x/SHASUMS256.txt" \
       | grep "linux-${arch}.tar.xz" | awk '{print $2}' | head -n1 > /tmp/node \
    && curl -fsSL "https://nodejs.org/dist/latest-v24.x/$(cat /tmp/node)" | tar -xJ -C /usr/local --strip-components=1

# Rust, at the version rust-toolchain.toml pins (rustup reads it in /src).
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal
ENV PATH="/root/.cargo/bin:${PATH}"
# linuxdeploy is itself an AppImage, and containers have no FUSE.
ENV APPIMAGE_EXTRACT_AND_RUN=1

WORKDIR /src
COPY . .
RUN npm ci
# The bundled default theme ships as a resource at themes/default, where RomDex looks for it.
RUN npx tauri build --bundles appimage --config '{"bundle":{"active":true,"targets":["appimage"],"resources":{"../../packages/theme-default/dist":"themes/default"}}}'

FROM scratch
COPY --from=build /src/target/release/bundle/appimage/*.AppImage /

#!/bin/sh
# Builds FrameWeaver inside a Podman container, so nothing needs installing on this machine.
#
#   ./build.sh          Build Linux and Windows release versions into dist/
#   ./build.sh test     Run the automated tests and code checks
#   ./build.sh linux    Build only the Linux version
#   ./build.sh windows  Build only the Windows version and its installer
#
# The downloads include FFmpeg, unpacked from the official BtbN builds kept in
# ~/.cache/frameweaver/ffmpeg-download (or $FFMPEG_DOWNLOADS).
set -e
cd "$(dirname "$0")"

IMAGE=localhost/frameweaver-build:latest
podman build -q -t "$IMAGE" -f container/Containerfile container >/dev/null
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
DOWNLOADS=${FFMPEG_DOWNLOADS:-$HOME/.cache/frameweaver/ffmpeg-download}

in_container() {
    podman run --rm \
        -v "$PWD":/src:Z \
        -v frameweaver-cargo-registry:/usr/local/cargo/registry \
        -v frameweaver-cargo-git:/usr/local/cargo/git \
        -w /src "$IMAGE" sh -c "$1"
}

# Unpacks ffmpeg and its license from the downloaded builds into third_party/ffmpeg/.
prepare_ffmpeg() {
    if [ ! -f third_party/ffmpeg/windows/ffmpeg.exe ]; then
        rm -rf third_party/ffmpeg/windows && mkdir -p third_party/ffmpeg/windows
        unzip -q -j "$DOWNLOADS/ffmpeg-n8.1-latest-win64-gpl-8.1.zip" '*/bin/ffmpeg.exe' '*/LICENSE.txt' -d third_party/ffmpeg/windows
    fi
    if [ ! -f third_party/ffmpeg/linux/ffmpeg ]; then
        rm -rf third_party/ffmpeg/linux && mkdir -p third_party/ffmpeg/linux
        tar -xJf "$DOWNLOADS/ffmpeg-n8.1-latest-linux64-gpl-8.1.tar.xz" -C third_party/ffmpeg/linux \
            --strip-components=1 --wildcards '*/bin/ffmpeg' '*/LICENSE.txt'
        mv third_party/ffmpeg/linux/bin/ffmpeg third_party/ffmpeg/linux/ffmpeg && rmdir third_party/ffmpeg/linux/bin
    fi
}

# Copies the licenses that go with every download.
add_licenses() {
    mkdir -p "$1/licenses"
    cp LICENSE "$1/licenses/FRAMEWEAVER-LICENSE.txt"
    cp licenses/FFMPEG-NOTICE.txt "$1/licenses/"
    cp "$2" "$1/licenses/FFMPEG-LICENSE.txt"
}

build_linux() {
    prepare_ffmpeg
    in_container "cargo build --release --locked"
    rm -rf dist/linux && mkdir -p dist/linux/FrameWeaver
    cp target/release/frameweaver third_party/ffmpeg/linux/ffmpeg dist/linux/FrameWeaver/
    add_licenses dist/linux/FrameWeaver third_party/ffmpeg/linux/LICENSE.txt
    tar -czf "dist/FrameWeaver-$VERSION-linux-x86_64.tar.gz" -C dist/linux FrameWeaver
    rm -rf dist/linux
    echo "Built dist/FrameWeaver-$VERSION-linux-x86_64.tar.gz"
}

build_windows() {
    prepare_ffmpeg
    in_container "cargo build --release --locked --target x86_64-pc-windows-gnu"
    rm -rf dist/windows && mkdir -p dist/windows/FrameWeaver
    cp target/x86_64-pc-windows-gnu/release/frameweaver.exe dist/windows/FrameWeaver/FrameWeaver.exe
    cp third_party/ffmpeg/windows/ffmpeg.exe dist/windows/FrameWeaver/
    add_licenses dist/windows/FrameWeaver third_party/ffmpeg/windows/LICENSE.txt
    in_container "makensis -V2 -DVERSION=$VERSION installer/frameweaver.nsi"
    in_container "cd dist/windows && zip -q -r ../FrameWeaver-$VERSION-windows-portable.zip FrameWeaver"
    rm -rf dist/windows
    echo "Built dist/FrameWeaver-$VERSION-setup.exe and dist/FrameWeaver-$VERSION-windows-portable.zip"
}

case "${1:-all}" in
    test)
        in_container "cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings \
            && cargo clippy --all-targets --locked --target x86_64-pc-windows-gnu -- -D warnings \
            && cargo test --locked"
        ;;
    linux) build_linux ;;
    windows) build_windows ;;
    all) build_linux; build_windows ;;
    *) echo "Unknown command: $1" >&2; exit 2 ;;
esac

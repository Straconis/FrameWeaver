#!/bin/sh
# Turns the SVG artwork into PNG and ICO files. Runs inside a throwaway container:
#   python3 make_artwork.py
#   podman run --rm -v "$PWD":/work:Z -w /work docker.io/library/alpine:3.20 sh render.sh
set -e
apk add --no-cache rsvg-convert imagemagick font-inter >/dev/null
mkdir -p out/icons
# App icon PNGs. Each size uses the version drawn for it.
for s in 16 20; do rsvg-convert -w $s -h $s frameweaver-icon-tiny.svg  -o out/icons/frameweaver-$s.png; done
for s in 24 32; do rsvg-convert -w $s -h $s frameweaver-icon-small.svg -o out/icons/frameweaver-$s.png; done
for s in 48 64 128 256 512; do rsvg-convert -w $s -h $s frameweaver-icon.svg -o out/icons/frameweaver-$s.png; done
# Windows .ico with every size Windows asks for
magick out/icons/frameweaver-16.png out/icons/frameweaver-20.png out/icons/frameweaver-24.png \
       out/icons/frameweaver-32.png out/icons/frameweaver-48.png out/icons/frameweaver-64.png \
       out/icons/frameweaver-128.png out/icons/frameweaver-256.png out/frameweaver.ico
# README banner at 2x for sharp display
rsvg-convert -w 2560 -h 800 frameweaver-banner.svg -o out/frameweaver-banner.png

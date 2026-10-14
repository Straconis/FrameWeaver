# FrameWeaver

FrameWeaver turns a set of pictures into a time-lapse video of a length you choose. It's SnapLapse's companion: SnapLapse captures a window's pictures, and FrameWeaver makes them into a video. Both live in the same GitHub account (Straconis/SnapLapse, Straconis/FrameWeaver), share a look (blue tile logo with a cyan accent), and share their build approach.

## Requirements

- **Input:** a folder of pictures, a single picture file, or SnapLapse's output.
  - SnapLapse output can be a recording folder, a recording ZIP, or a ZIP split into `… - part 1.zip`, `- part 2.zip` and so on.
  - Pictures are sorted in natural number order: 2 before 10.
  - PNG, JPG and WebP are supported.
- **Output:** an output folder, a file name (defaulting to the input's name), and a video format.
- **Length:** an exact total length entered as HH:MM:SS. FrameWeaver works out how long each picture stays on screen.
  - The frames are spread so they add up to exactly the chosen length, with no picture on screen more than one frame longer than another.
  - If there are more pictures than frames, it skips pictures evenly and says so.
  - An alternative mode sets a time per picture instead.
- **Size and aspect ratio:**
  - Sizes: same as the pictures, 720p, 1080p, 1440p, 4K, or a custom size.
  - Aspect ratios: 16:9, 4:3, 1:1, 9:16 vertical, 21:9, or the same as the pictures.
  - How pictures fit the frame: fit with bars (with a choice of bar color), fill by cropping, or stretch.
  - Width and height are always even, because video encoders need that.
- **Other settings:** frame rate (24/25/30/50/60) and quality (high, balanced or small file).
- **Platforms:** Windows 10, Windows 11 and Linux. Development is on Debian 13 GNOME, in a VirtualBox VM without 3D acceleration.
- **Light and fast,** like SnapLapse. Show progress while encoding, and allow cancelling.
- **UI:** use toggle switches for on/off settings, never checkboxes.

## Tech decisions

- **Rust with egui (eframe)**, the same stack and look as SnapLapse. Builds run in a Podman container via `./build.sh`, with Windows cross-compiled via MinGW and an NSIS installer.
- **Video encoding is done by FFmpeg, run as a separate program.**
  - The pictures are passed to FFmpeg's concat demuxer with an exact duration for each picture, so no picture is ever copied on disk.
  - On Windows, an FFmpeg build is bundled next to the exe, with its license included.
  - On Linux, FFmpeg comes from the system (`apt install ffmpeg`), or from a path set in the app.

## Working with the owner

- The owner can't read Rust. Claude owns code quality: cover behaviour with tests, including property tests, and explain changes in plain language.
- Do git work (commit, push, tag) through `git` in the Terminal panel.
- Version numbers: a new feature bumps the middle number, and each feature is released on GitHub with the installer, exe, Linux binary and SHA256SUMS.

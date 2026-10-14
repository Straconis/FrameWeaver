<p align="center">
  <img src="branding/out/frameweaver-banner.png" alt="FrameWeaver: turn pictures into a time-lapse video of any length" width="100%">
</p>

**FrameWeaver** turns a folder of pictures into a time-lapse video of exactly the length you choose. It's the companion to [SnapLapse](https://github.com/Straconis/SnapLapse): SnapLapse captures a window's pictures on a timer, and FrameWeaver weaves them into a video.

> **Status:** in development. There's nothing to download yet.

## Planned features

- **Open pictures your way:** a folder of pictures, or a SnapLapse recording folder or ZIP, including split ZIPs (part 1, part 2…). Pictures are taken in natural order: 2 comes before 10.
- **Pick the length as HH:MM:SS.** FrameWeaver works out how long each picture stays on screen, spreads the frames evenly, and shows the math before you start. You can set a time per picture instead.
- **Formats:**
  - MP4 (H.264), which plays everywhere.
  - MP4 (H.265), at about half the size.
  - WebM (VP9), an open format.
  - Animated GIF, for short clips.
- **Size and shape:**
  - **Size:** same as the pictures, 720p, 1080p, 1440p, 4K, or custom.
  - **Aspect ratio:** 16:9, 4:3, 1:1, 9:16 vertical, or 21:9.
  - **Fit:** pictures can fit with bars, fill by cropping, or stretch.
- **Frame rate and quality:** 24, 25, 30, 50 or 60 fps, and high, balanced or small file.
- **Progress and cancel** while it works, and one click to open the finished video.

## Platforms

Windows 10, Windows 11 and Linux. Video encoding uses [FFmpeg](https://ffmpeg.org). It's included with the Windows version; on Linux, install it with your package manager, for example `sudo apt install ffmpeg`.

## License

[MIT](LICENSE). The bundled FFmpeg has its own license, which is included with it.

# ShadowRust 🎮

**ShadowRust** is a high-performance, open-source video capture viewer built in Rust. It is designed as a modern replacement for proprietary capture software (such as Genki Arcade), offering low-latency display and recording for capture cards like the **Genki ShadowCast 2** and **ShadowCast 4K**.

> Works with any UVC-compatible capture card (Nintendo Switch 2, PS5, Xbox, etc.)

---

## ✨ Features

- **Ultra-low latency** preview via GPU rendering (wgpu — Vulkan / DirectX 12 / OpenGL)
- **Audio pass-through** — captures the HDMI audio and plays it on your speakers in real time
- **Volume control** with mute (stream stays alive, no reconnection needed)
- **H.264 + AAC recording** to `.mp4` via FFmpeg — audio and video stay in sync
- **Automatic resolution detection** from the capture card
- **Persistent configuration** — remembers your device, resolution, volume and recording path
- **Sleep prevention** — the OS won't suspend while you are capturing
- **Auto-update notifications** — notified in-app when a new version is available
- **Single executable on Windows** — FFmpeg DLLs are embedded, no separate install needed
- Cross-platform: **Windows** and **Linux**

---

## 📥 Download

Head to the [**Releases page**](https://github.com/Ajustor/shadowrust/releases/latest) or the [**Download page**](https://ajustor.github.io/shadowrust/) to grab the latest binary.

| Platform | File |
|---|---|
| Windows (x64) | `shadowrust-windows-x86_64.zip` → single `.exe`, no install |
| Linux (x64) | `shadowrust-linux-x86_64` → standalone binary |

### Windows

1. Download and unzip `shadowrust-windows-x86_64.zip`
2. Run `shadowrust.exe`
3. On first launch, FFmpeg is automatically extracted to `%LOCALAPPDATA%\ShadowRust\dlls`

### Linux

```bash
chmod +x shadowrust-linux-x86_64
./shadowrust-linux-x86_64
```

> **Tip (Linux):** You may need to add yourself to the `video` group: `sudo usermod -aG video $USER`

---

## 🎮 Usage

| Key / Control | Action |
|---|---|
| `Tab` | Show / hide the settings panel |
| `F11` | Toggle fullscreen |
| Settings panel | Select capture device, resolution, FPS |
| Volume slider | Adjust audio pass-through volume |
| Mute button | Mute / unmute (stream stays alive) |
| ▶ Start Capture | Begin video capture |
| ⏺ Start Recording | Record to `.mp4` |

### Recommended workflow

1. Connect your capture card
2. Launch ShadowRust
3. Select your capture device from the dropdown
4. Choose the resolution that matches your console output
5. Press **▶ Start Capture** — audio starts automatically
6. Optionally press **⏺ Start Recording** to save to a file

Your settings (device, resolution, volume, recording path) are saved automatically and restored on next launch.

---

## 🏗️ Build from source

### Prerequisites

**All platforms:**
- [Rust stable](https://rustup.rs/) (1.80+)

**Linux:**
```bash
sudo apt install \
  libavcodec-dev libavformat-dev libavutil-dev \
  libavdevice-dev libswscale-dev libswresample-dev \
  libclang-dev clang pkg-config \
  libv4l-dev libudev-dev libvulkan-dev libasound2-dev
```

**Windows:**
- Download the [pre-built FFmpeg 7.x shared build](https://github.com/BtbN/FFmpeg-Builds/releases) (win64-gpl-shared)
- Extract and set `FFMPEG_DIR` to the extracted folder
- LLVM/Clang must be on `PATH` (installed via [LLVM releases](https://github.com/llvm/llvm-project/releases))

### Build

```bash
git clone https://github.com/Ajustor/shadowrust.git
cd shadowrust
cargo build --release
```

The binary will be at `target/release/shadowrust` (Linux) or `target\release\shadowrust.exe` (Windows).

---

## 🤝 Contributing

Pull requests and issues are welcome! Please open an issue first for large changes.

---

## 📄 License

MIT — see [LICENSE](LICENSE) for details.

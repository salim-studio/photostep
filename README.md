<p align="center">
  <img src="assets/logo.svg" width="128" height="128" alt="PhotoStep logo" />
</p>

<h1 align="center">PhotoStep</h1>

<p align="center"><strong>Fast layer-based image editor in pure Rust.</strong><br/>
Layers, filters, adjustments and a clean desktop studio — small, parallel, and fully offline.</p>

## Brand

| Token | Value |
|---|---|
| Step Orange (primary) | `#FF5A28` |
| Amber (gradient) | `#FFB03A` |
| Aqua (accent) | `#35D0C5` |
| Ink (surface) | `#14142B` |

The mark — three ascending steps ending in a lens dot — stands for progress:
every edit moves the picture one step forward. The desktop app uses the same
palette (dark ink surfaces, step-orange selection and actions).

## Why PhotoStep is fast

- **Small core:** a handful of modules (`core / ops / io / app`) — builds in seconds.
- **Real parallelism:** every adjustment and filter runs on all cores via `rayon`.
- **Fast Gaussian:** 3× separable box-blur approximation instead of heavy convolution.
- **Fast path** for the Normal blend mode, no wasted float math.
- **Single texture upload** — the canvas re-uploads only when pixels change, not every frame.

## Features

- 🗂 **Layers:** add / duplicate / delete / merge / flatten, opacity, 13 blend modes, visibility
- 🎨 **Tools:** soft-edge brush + eraser, fill bucket, color picker, rectangle selection, zoom
- 📊 **Adjustments:** brightness, contrast, saturation, exposure, invert, grayscale, threshold, posterize, auto-contrast, hue, vibrance, levels, color balance
- ✨ **Filters:** Gaussian blur, sharpen (unsharp mask), edge detect, emboss, pixelate, noise, vignette
- 🔄 **Transforms:** rotate 90° CW, flip horizontal / vertical, 30-step undo / redo
- 💾 **Formats:** PNG, JPEG, TIFF, BMP, WebP, GIF, QOI + native `.pstep` project files (JSON)
- ⌨️ **Headless CLI** for batch processing

## Getting started

### 1) Install Rust (once)

```powershell
winget install -e --id Rustlang.Rustup
# then close and reopen the terminal
rustup toolchain install stable
```

### 2) Launch the studio

```powershell
cd photostep
cargo run --release
```

### 3) Batch processing (no GUI)

```powershell
cargo run --release -- --input in.png --out out.png --op "brightness:20" --op "contrast:25" --op "blur:4" --op sharpen:1.2
cargo run --release -- --input photo.jpg --out gray.png --op grayscale --op "vignette:0.6"
```

Supported ops: `brightness:N contrast:N invert grayscale threshold:N posterize:N exposure:N vibrance:N saturate:X hue:deg blur:R sharpen:A edge emboss pixelate:N noise:N vignette:X autocontrast fliph flipv rot90`

## Shortcuts

`Ctrl+Z` undo · `Ctrl+Y` redo · tools: Brush (B), Eraser (E), Fill (G), Picker (I), Move (V)

## Architecture

```
src/main.rs  → CLI + egui launcher
src/core.rs  → Document / Layer / Blend / History + tests
src/ops.rs   → adjustments + filters + transforms (rayon)
src/io.rs    → image + .pstep project I/O
src/app.rs   → studio UI (tools / canvas / layers / adjustments)
assets/      → brand logo (SVG)
```

## Roadmap

- Tile-based copy-on-write storage for huge (8K+) canvases
- PSD import via the `psd` crate
- GPU compositing via `wgpu`
- Stylus pressure support + non-destructive adjustment layers

## License

MIT — see [LICENSE-MIT](LICENSE-MIT).

© 2026 salim-slimani. All rights reserved.

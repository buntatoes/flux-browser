<div align="center">
  <img src="./assets/root-logo.png" width="96" height="96" alt="Root" />
  <h1>Root Browser</h1>
  <p>A small desktop browser Proof of Concept (PoC), written from scratch in Rust, C++20, and JavaScript.</p>
  <p>
    <img src="https://img.shields.io/badge/release-v.18-ff6b24?style=flat-square" alt="v.18" />
    <img src="https://img.shields.io/badge/status-Proof_of_Concept-d4a359?style=flat-square" alt="Proof of Concept" />
    <img src="https://img.shields.io/badge/platform-Windows_%7C_Linux-2ea44f?style=flat-square" alt="Windows and Linux" />
    <img src="https://img.shields.io/badge/rust-2024-orange?style=flat-square" alt="Rust 2024" />
    <a href="./SECURITY.md"><img src="https://img.shields.io/badge/security-policy-2ea44f?style=flat-square" alt="Security" /></a>
    <a href="./LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0_%2B_MPL--2.0_%2B_Proprietary-e8c37a?style=flat-square" alt="License" /></a>
  </p>
</div>

Version 0.18. Author: Buntos.

Root Browser loads HTML, applies a CSS cascade, lays the page out, and paints it into a CPU RGBA buffer. Windows can upload that finished buffer to Direct3D 11. Linux copies it with X11. The page is rasterized on the CPU either way.

There are no crates.io dependencies. The engine crates live in this repository. Seven companion libraries sit beside it and are linked by path.

## Download

Windows x64 builds are attached to the [v.18 release](https://github.com/buntatoes/root-browser/releases/tag/v.18).

| File | What it is |
| --- | --- |
| `RootBrowser-Setup-v.18-win64.exe` | One setup program. It installs, and it uninstalls with `--uninstall`. |
| `root.exe` | The browser by itself, with no installer. |

SHA-256 checksums are in the release notes. The setup program is not required to build from source.

## What a page can do

- HTML is parsed into one tree. Selectors, forms, images, and video elements are read from that tree.
- CSS covers the cascade and media queries. `width`, `height`, `max-width`, and offsets keep `px`, `%`, `em`, and `rem`. Any other unit becomes `auto`.
- `font-size` accepts those same four units. `%` and `em` use the parent font size. `rem` uses the `html` font size, which is 16px until `html` sets one. Margin, padding, border, and `gap` still accept only `px` and `0`.
- A percentage width uses the containing block width. A percentage height uses the containing block height when that height is set, and is `auto` when it is not.
- Layout covers block, inline, flex, table, and positioned boxes. `flex-wrap: wrap` starts another line. `flex-grow` shares positive free space on each line and does not shrink. Column flex does not wrap or grow.
- The display list is painted into an RGBA framebuffer. Optional C++20 code fills scanlines, rounded rectangles, shadows, gradients, glyph coverage, and NV12 rows when `cl.exe`, `g++`, or `clang++` is on `PATH`. Without a compiler, those paths stay in Rust.
- Script runs in a budgeted Rust interpreter. A whole script is sent to the numeric C++ VM only when that compiler accepts it and the script does not touch the DOM, `fetch()`, or storage. Loop and function bodies inside a Rust script stay on the Rust interpreter. The caller owns the VM context. This is not a general compiler.
- `fetch()` is same-origin only. A cross-origin URL returns status 403 and does not connect. The interpreter keeps at most 64 KiB of the body. The numeric VM does not perform `fetch()`.
- `localStorage` and `sessionStorage` are partitioned by origin. Each origin may store 64 KiB of key and value bytes. A write past that cap leaves the previous value in place. `sessionStorage` dies with the tab.
- HTTP/1.1 GET and POST, TLS, cookies, HSTS, CSP, CORP, and SHA-256 subresource checks go through `root-net`. A short list of host names is blocked before connect.
- An http or https load starts one `--tab-worker` process and waits 5 seconds for one `ROOTIPC1` frame. The window paints the document from that frame and does not run those scripts again. A timeout or a bad frame shows an error page. `root://` pages and local files stay in the window process. The worker does not receive the session cookie jar or `localStorage`. This is not a multi-process sandbox.

Built-in pages are `root://home`, `root://settings`, `root://security`, `root://history`, `root://downloads`, and `root://inspect`. `frontend/` is text with string replacements, plus the scripts in `frontend/scripts/`. It is not built by a Node toolchain.

## What this is not

- The page is not rasterized on the GPU. Direct3D 11, when it is available, only presents the CPU buffer. If that device or `compositor.hlsl` fails to compile, Windows copies the same buffer with GDI.
- Video frames are decoded on the CPU. There is no GPU decoder. Encrypted samples are AES-128-CTR from `root-cipher`.
- `display: grid`, `var(--token)`, `ch`, `vw`, and `calc()` are not implemented.
- One OS process does not stay open for each tab.

Limits that are enforced in code are listed in [SECURITY.md](./SECURITY.md).

## Window

Linux draws with X11 and does HTTPS with OpenSSL (`libssl` / `libcrypto`). Windows uses the system window APIs, Schannel, and DPAPI.

On Linux the window sets `PR_SET_NO_NEW_PRIVS`, turns core dumps off, and tries a 2 GiB address-space cap. `--tab-worker` asks for 256 MiB and exits if that cap does not stick.

`profile.root` stores bookmarks, history, cookies, HSTS, settings, and origin-partitioned `localStorage`. Windows seals it with DPAPI. Linux seals it with AES-256-GCM. The Linux key file is `profile.key` in the same directory, mode 0600, written before the profile.

| Keys | Action |
| --- | --- |
| Ctrl+T / Ctrl+W | New tab / close tab |
| Ctrl+L | Address bar |
| Ctrl+R / F5 | Reload |
| Alt+Left / Alt+Right | Back / forward |
| Ctrl+F | Find |
| Ctrl+J / Ctrl+H | Downloads / history |
| F12 | Inspector on Windows |

## Source layout

| Path | What it is |
| --- | --- |
| `crates/root-dom` | HTML tree and selectors |
| `crates/root-style` | CSS cascade and media queries |
| `crates/root-layout` | Block, inline, flex, table, and positioned boxes |
| `crates/root-paint` | Display list and zoom |
| `crates/root-raster` | RGBA framebuffer and the optional C++20 scanline helpers |
| `crates/root-script` | Budgeted script runtime and the optional numeric VM |
| `crates/root-net` | HTTP, TLS, and the host block list |
| `crates/root-sandbox` | Process limits and the `ROOTIPC1` frame format |
| `crates/root-browser` | Window, pages, downloads, and `profile.root` |
| `frontend/` | Built-in page text and page scripts |

`root-dom`, `root-style`, `root-layout`, `root-paint`, `root-raster`, and `root-script` are Apache-2.0. `root-net`, `root-sandbox`, `root-browser`, `frontend/`, and `assets/root-logo.png` are all rights reserved. The text is in `LICENSE`, `LICENSE-APACHE`, `LICENSE-MPL`, and `LICENSE-PROPRIETARY`.

## Build

Clone this repository and the seven libraries below as sibling directories.

```text
../root-browser
../root-cipher
../root-compress
../root-image
../root-json
../root-ttf
../root-gpu
../root-video
```

Rust 1.85 or newer. A C++20 compiler is optional. If none is found, the C++ files are skipped and the Rust paths are used.

```text
cargo test
cargo run --release -p root-browser --bin root
```

```text
root [file or http/https/root URL]
root open [--width N] [--height N] [file or http/https/root URL]
root render [--width N] <file or http/https/root URL>
root raster [--width N] [--height N] <file or http/https/root URL> <output.ppm>
root install
root uninstall
```

`root install` copies the program into the per-user install directory. It does not download a second copy.

## Companion libraries

These are separate MPL-2.0 repositories. This browser does not vendor their source.

| Repository | What it is |
| --- | --- |
| [root-cipher](https://github.com/buntatoes/root-cipher) | AES-128, AES-256, CTR, AES-256-GCM |
| [root-compress](https://github.com/buntatoes/root-compress) | DEFLATE, zlib, gzip |
| [root-image](https://github.com/buntatoes/root-image) | PNG, JPEG, GIF, WebP, ICO, BMP, PPM |
| [root-json](https://github.com/buntatoes/root-json) | JSON |
| [root-ttf](https://github.com/buntatoes/root-ttf) | TrueType and OpenType |
| [root-gpu](https://github.com/buntatoes/root-gpu) | CPU drawing of boxes, shadows, and NV12. `compositor.hlsl` is source text compiled by the browser on Windows |
| [root-video](https://github.com/buntatoes/root-video) | CPU frames in a small MP4 container |

## Author

Buntos ([@buntatoes](https://github.com/buntatoes))

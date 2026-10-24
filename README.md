<div align="center">
  <img src="./assets/flux-logo.png" width="96" height="96" alt="Flux" />
  <h1>Flux Browser</h1>
  <p>A small desktop browser Proof of Concept (PoC) written in Rust, C++20, and JavaScript.</p>
  <p>
    <img src="https://img.shields.io/badge/release-v.18-ff6b24?style=flat-square" alt="v.18" />
    <img src="https://img.shields.io/badge/status-Proof_of_Concept-d4a359?style=flat-square" alt="Proof of Concept" />
    <img src="https://img.shields.io/badge/platform-Windows_%7C_Linux-2ea44f?style=flat-square" alt="Windows and Linux" />
    <img src="https://img.shields.io/badge/rust-2024-orange?style=flat-square" alt="Rust 2024" />
    <a href="./SECURITY.md"><img src="https://img.shields.io/badge/security-policy-2ea44f?style=flat-square" alt="Security" /></a>
    <a href="./LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0_%2B_MPL--2.0_%2B_Proprietary-e8c37a?style=flat-square" alt="License" /></a>
  </p>
</div>

Version 0.18.

Linux draws the window with X11 and does HTTPS with OpenSSL (`libssl` / `libcrypto`). Windows uses the system window APIs, Schannel, and DPAPI. The Linux profile is AES-256-GCM from [flux-cipher](https://github.com/buntatoes/flux-cipher). The key file is `profile.key` in the same directory, mode 0600, written before the profile.

Video frames are converted on the CPU. Encrypted samples are AES-128-CTR from `flux-cipher`. There is no GPU decoder here.

## Layout

| Path | What it is |
| --- | --- |
| `crates/flux-dom` | HTML tree and selectors |
| `crates/flux-style` | CSS cascade and media queries. `width`, `height`, `max-width`, and offsets keep `px`, `%`, `em`, and `rem`. Other units become `auto`. |
| `crates/flux-layout` | Block, inline, flex, table, and positioned boxes. See the length and flex notes below. |
| `crates/flux-paint` | Display list and zoom |
| `crates/flux-raster` | RGBA framebuffer. Linux text uses `flux-ttf` when a system font is present. `cpp/flux_raster_accel.cpp` and `cpp/flux_raster_compositor.cpp` handle scanlines, SDF rounded rects, box shadows, gradients, glyph coverage, and NV12 conversion when a C++20 compiler is on `PATH` |
| `crates/flux-script` | Script runtime with built-in JS prelude modules (`js/runtime_core.js`, `js/runtime_async_web.js`), `Promise` microtasks, origin-partitioned `localStorage` and `sessionStorage`, and a freestanding C++20 register VM (`cpp/flux_vm.hpp`, `cpp/flux_vm_heap.cpp`, `cpp/flux_vm_ic.cpp`, `cpp/flux_vm_compiler.cpp`, `cpp/flux_jit_accel.cpp`) with NaN-boxed values, hidden-class shapes, inline caches, and mark-sweep collection |
| `crates/flux-net` | HTTP, TLS, and a short list of blocked host names |
| `crates/flux-sandbox` | Process limits |
| `crates/flux-browser` | Window, pages, downloads, and the profile file (`profile.flx`, including persisted `localStorage`) |

`frontend/` is stored as text and turned into HTML with string replacements, paired with built-in JavaScript page controllers in `frontend/scripts/` (`flux_core.js`, `home.js`, `settings.js`, `inspect.js`, `security.js`, `downloads.js`, and `history.js`). It is not built by a Node toolchain.

`font-size` accepts `px`, `%`, `em`, and `rem`. `%` and `em` use the parent font size. `rem` uses the `html` font size, which is 16px until `html` sets one. Margin, padding, border, and `gap` still accept only `px` and `0`.

A percentage width uses the containing block width. A percentage height uses the containing block height when that height is set, and is `auto` when it is not. `em` on those properties uses the element's font size. `flex-wrap: wrap` breaks a row onto another line. `flex-grow` shares positive free space on each of those lines. Items do not shrink. Column flex does not wrap or grow.

Script `fetch()` is same-origin only and runs in the Rust interpreter. A response registered for tests wins over the network. Otherwise `flux-browser` performs the GET with `flux-net`, the session cookie jar, and the existing response caps. The interpreter keeps at most 64 KiB of the body. A spent script step budget does not connect. The numeric VM does not perform `fetch()`.

`eval_script` sends a whole script to the numeric VM only when that compiler accepts it and the script does not touch the DOM, `fetch()`, or storage. Loop and function bodies inside a Rust script stay on the Rust interpreter. The caller owns the VM context. `textContent` stays on the Rust interpreter. This is not a general compiler.

An http or https navigation starts one `--tab-worker` process and waits up to 5 seconds for one `FLUXIPC1` frame. The window paints the document that frame returns, and it does not run those scripts again. A timeout, a rejected frame, or a worker that does not start shows an error page instead of that document. `flux://` pages and files stay in the window process. The worker does not receive the session cookie jar or `localStorage`. This is not a multi-process sandbox.

On Windows the CPU framebuffer can be uploaded to Direct3D 11. That path compiles `compositor.hlsl` and samples the uploaded buffer. The page is still rasterized on the CPU. If the device or the compile fails, the window copies the same buffer with GDI. Linux still copies it with X11. Video frames are still decoded on the CPU.

`profile.flx` stores `localStorage` rows with the rest of the profile. Windows seals that file with DPAPI. Linux seals it with AES-256-GCM. Each origin may store 64 KiB of key and value bytes. A write past that cap leaves the previous value in place. `sessionStorage` is not in the profile and dies with the tab.

## Build

Clone this repository and the seven libraries below as sibling directories. There are no crates.io dependencies.

```text
../flux-browser
../flux-cipher
../flux-compress
../flux-image
../flux-json
../flux-ttf
../flux-gpu
../flux-video
```

Rust 1.85 or newer. `cl.exe`, `g++`, or `clang++` is optional. The C++20 files fall back to Rust if no C++ compiler is found.

```
cargo test
cargo run --release -p flux-browser --bin flux
```

On Linux the window sets `PR_SET_NO_NEW_PRIVS`, turns core dumps off, and tries a 2 GiB address-space cap. `--tab-worker` asks for a 256 MiB cap and exits if that cap does not stick. The window starts that process for an http or https load. `flux://` pages and files are still drawn in the window process.

## Keys

| Keys | Action |
| --- | --- |
| Ctrl+T / Ctrl+W | New tab / close tab |
| Ctrl+L | Address bar |
| Ctrl+R | Reload |
| Alt+Left / Alt+Right | Back / forward |
| Ctrl+F | Find |
| Ctrl+J / Ctrl+H | Downloads / history |
| F12 | Inspector |

## The other repositories

| Repository | What it is |
| --- | --- |
| [flux-cipher](https://github.com/buntatoes/flux-cipher) | AES-128, AES-256, CTR, AES-256-GCM |
| [flux-compress](https://github.com/buntatoes/flux-compress) | DEFLATE, zlib, gzip |
| [flux-image](https://github.com/buntatoes/flux-image) | PNG, JPEG, GIF, WebP, ICO, BMP, PPM |
| [flux-json](https://github.com/buntatoes/flux-json) | JSON |
| [flux-ttf](https://github.com/buntatoes/flux-ttf) | TrueType and OpenType |
| [flux-gpu](https://github.com/buntatoes/flux-gpu) | CPU drawing of boxes, shadows, and NV12 |
| [flux-video](https://github.com/buntatoes/flux-video) | CPU frames in a small MP4 container |

## License

`flux-dom`, `flux-style`, `flux-layout`, `flux-paint`, `flux-raster`, and `flux-script` are Apache-2.0. `flux-net`, `flux-sandbox`, `flux-browser`, `frontend/`, and `assets/flux-logo.png` are all rights reserved. The seven libraries above are MPL-2.0. The text is in `LICENSE`, `LICENSE-APACHE`, `LICENSE-MPL`, and `LICENSE-PROPRIETARY`.

## Author

Buntos ([@buntatoes](https://github.com/buntatoes))

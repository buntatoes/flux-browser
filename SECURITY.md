# Security

Version 0.18. These are the limits that are actually in the code. This is not a multi-process browser sandbox.

Report problems in private:

https://github.com/buntatoes/flux-browser/security/advisories/new

Do not put the details in a public issue.

## What is on

- Linux: `PR_SET_NO_NEW_PRIVS`, core dumps off, and an attempt to cap address space at 2 GiB. `--tab-worker` asks for 256 MiB and exits if that cap does not stick.
- An http or https navigation starts one `--tab-worker` and waits 5 seconds. The window paints the returned document only when the frame says it succeeded. Otherwise it shows an error page and does not paint that document. `flux://` pages and files stay in the window process. The worker does not receive the session cookie jar. This is not a multi-process sandbox.
- Windows: a job object on the window process with a memory cap. The worker job, when used, also limits process count and UI rights.
- A panic in `execute_isolated`, or in a background page load, is caught.
- HTTPS on Linux goes through OpenSSL, with certificate checks. Windows uses Schannel.
- Linux profiles (`profile.flx`, including bookmarks, history, cookies, HSTS, settings, and origin-partitioned `localStorage`) are AES-256-GCM from `flux-cipher`. The key is `profile.key` (mode 0600) in the same directory, written before the profile. Windows profiles use DPAPI. `sessionStorage` is not in the profile. Each origin can store 64 KiB of `localStorage` or `sessionStorage` key and value bytes. A write past that cap leaves the previous value in place.
- Encrypted video samples are AES-128-CTR from `flux-cipher`. CTR has no tag.
- Parsers here and in the sibling libraries reject oversized input.
- `flux-net` blocks a fixed list of tracker host names before it connects.
- SHA-256 is used for subresource checks and download records.
- PNG chunk checksums are checked in `flux-image`.
- Stylesheet `width`, `height`, `max-width`, and offsets resolve `px`, `%`, `em`, and `rem`. Any other unit becomes `auto`. A percentage height is `auto` when the containing block height is not set. `flex-grow` only distributes positive free space.
- Script `fetch()` is same-origin only. The Rust interpreter performs it. A cross-origin URL returns status 403 and does not connect. The numeric VM does not perform `fetch()`. The interpreter keeps at most 64 KiB of a response body. `flux-net` still stops at its raw and decompressed caps.

## What is not on

- One OS process per tab. A remote navigation may start one worker and wait for it. The window process still owns the tabs.
- A GPU page rasterizer. Windows can upload the CPU framebuffer to Direct3D 11 and compile `compositor.hlsl` to sample that upload. The page is still rasterized on the CPU. Linux still copies pixels with X11. A GPU video decoder is not included. The VA-API and Direct3D names in `flux-video` are labels. Video frames are decoded on the CPU.
- Dynamic machine-code generation. The C++20 `FluxVM` next to `flux-script` runs a register bytecode interpreter, NaN-boxed generational heap, hidden-class shape table, inline caches, and numeric loop evaluator without emitting executable native machine code pages. Page scripts use it only when the compiler accepts the whole script and it does not touch the DOM, `fetch()`, or storage. Loop and function bodies inside a Rust script stay on the Rust interpreter. It is not a general compiler. The caller owns the VM context. The worker process has its own context.

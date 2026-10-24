# Security

Version 0.19. These are the limits that are actually in the code. This is not a multi-process browser sandbox.

Report problems in private:

https://github.com/buntatoes/anvil-browser/security/advisories/new

Do not put the details in a public issue.

## What is on

- Linux: `PR_SET_NO_NEW_PRIVS`, core dumps off, and an attempt to cap address space at 2 GiB. `--tab-worker` asks for 256 MiB and exits if that cap does not stick.
- An http or https navigation starts one `--tab-worker` and waits 5 seconds. The window paints the returned document only when the frame says it succeeded. Otherwise it shows an error page and does not paint that document. `anvil://` pages and files stay in the window process. The worker does not receive the session cookie jar. This is not a multi-process sandbox.
- Windows: a job object on the window process with a memory cap. The worker job, when used, also limits process count and UI rights.
- A panic in `execute_isolated`, or in a background page load, is caught.
- HTTPS on Linux goes through OpenSSL, with certificate checks. Windows uses Schannel.
- Linux profiles (`profile.flx`, including bookmarks, history, cookies, HSTS, settings, and origin-partitioned `localStorage`) are AES-256-GCM from `anvil-cipher`. The key is `profile.key` (mode 0600) in the same directory, written before the profile. Windows profiles use DPAPI. `sessionStorage` is not in the profile. Each origin can store 64 KiB of `localStorage` or `sessionStorage` key and value bytes. A write past that cap leaves the previous value in place.
- Encrypted video samples are AES-128-CTR from `anvil-cipher`. CTR has no tag.
- Parsers here and in the sibling libraries reject oversized input.
- `anvil-net` blocks a fixed list of tracker host names before it connects.
- SHA-256 is used for subresource checks and download records.
- PNG chunk checksums are checked in `anvil-image`.
- Stylesheet `width`, `height`, `max-width`, and offsets resolve `px`, `%`, `em`, and `rem`. Any other unit becomes `auto`. A percentage height is `auto` when the containing block height is not set. `flex-grow` only distributes positive free space.
- Script `fetch()` is same-origin only. The Rust interpreter performs it. A cross-origin URL returns status 403 and does not connect. The numeric compiler does not perform `fetch()`. The interpreter keeps at most 64 KiB of a response body. `anvil-net` still stops at its raw and decompressed caps.
- Numeric x86-64 machine code. A `while` or `for` whose body is only numbers, with no objects, calls, or property reads, is compiled and the numbers are written back into the page script. A whole script with no DOM, `fetch()`, or storage can take the same path. On Linux the page is mapped writable, filled, then marked read and execute. On Windows it is allocated with `VirtualAlloc`. The caller owns the VM context. The worker process has its own context. This is not a general compiler.

## What is not on

- One OS process per tab. A remote navigation may start one worker and wait for it. The window process still owns the tabs.
- A hardware GPU page rasterizer. Shadows, rounded boxes, and gradients are drawn by `anvil-gpu`'s software device. The bytes match the CPU compositor. Square fills, text, and ordinary images stay on the older path. Windows can upload the finished CPU framebuffer to Direct3D 11 and compile `compositor.hlsl` to sample that upload. Linux still copies pixels with X11. There is no GPU video decoder. The VA-API and Direct3D names in `anvil-video` are labels. Video frames are decoded on the CPU, then drawn through the same software device before the controls are painted.

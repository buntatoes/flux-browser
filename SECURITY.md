# Security

Version 0.17. These are the limits that are actually in the code. This is not a multi-process browser sandbox.

Report problems in private:

https://github.com/buntatoes/flux-browser/security/advisories/new

Do not put the details in a public issue.

## What is on

- Linux: `PR_SET_NO_NEW_PRIVS`, core dumps off, and an attempt to cap address space at 2 GiB. `--tab-worker` asks for 256 MiB and exits if that cap does not stick. Normal browsing does not start the worker.
- Windows: a job object on the window process with a memory cap. The worker job, when used, also limits process count and UI rights.
- A panic in `execute_isolated`, or in a background page load, is caught.
- HTTPS on Linux goes through OpenSSL, with certificate checks. Windows uses Schannel.
- Linux profiles (`profile.flx`, including bookmarks, history, cookies, HSTS, settings, and origin-partitioned `localStorage`) are AES-256-GCM from `flux-cipher`. The key is `profile.key` (mode 0600) in the same directory, written before the profile. Windows profiles use DPAPI.
- Encrypted video samples are AES-128-CTR from `flux-cipher`. CTR has no tag.
- Parsers here and in the sibling libraries reject oversized input.
- `flux-net` blocks a fixed list of tracker host names before it connects.
- SHA-256 is used for subresource checks and download records.
- PNG chunk checksums are checked in `flux-image`.

## What is not on

- One OS process per tab.
- A GPU video decoder. The VA-API and Direct3D names in `flux-video` are labels. `flux-gpu` does not open a GPU device. Its shader files are source text.
- Dynamic machine-code generation. The C++20 `FluxVM` next to `flux-script` runs a register bytecode interpreter, NaN-boxed generational heap, hidden-class shape table, inline caches, and numeric loop evaluator without emitting executable native machine code pages.
- A 5 second worker timeout. The constant exists. The window does not enforce it.

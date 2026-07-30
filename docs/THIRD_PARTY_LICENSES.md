# Third-Party Licenses

This file is a generated summary of every crates.io/git dependency's license
across the humm workspace, grouped by license expression. It complements
`NOTICE` (repo root), which covers the non-crate components (models, fonts,
Homebrew system libraries) and the components the M9 spec calls out for
attribution-obligation detail (e.g. speakrs's Apache-2.0 §4 terms).

**Regenerate with** (from the repo root, after `cargo install cargo-license`
— crates.io is sandbox-allowlisted so the one-time install needs no
elevated network access):

```sh
cargo license --avoid-dev-deps
```

`--avoid-dev-deps` scopes this report to what actually ships in humm's
built binaries (`humm`, `humm-gui`) — test-only/benchmark-only dependencies
(e.g. `insta`, `criterion`, `assert_cmd`) are excluded. Run without that flag
to additionally see dev/test-only crates.

Regenerated: 2026-07-30, against this workspace's `Cargo.lock`
(`cargo-license 0.7.0`, `cargo 1.97.1`). humm's own workspace crates
(`humm-core`, `humm-media`, `humm-whisper`, `humm-audio`, `humm-store`,
`humm-dfn`, `humm-diarize`, `humm-cli`, `humm-gui`) appear below under `MIT`
because that is their own declared license (`license.workspace = true`) —
they are not third-party and are called out here only because the tool
reports the whole resolved dependency graph, itself included.

```
(Apache-2.0 OR MIT) AND OFL-1.1 AND Ubuntu-font-1.0 (1): epaint_default_fonts
(Apache-2.0 OR MIT) AND Unicode-3.0 (1): unicode-ident
0BSD OR Apache-2.0 OR MIT (1): adler2
Apache-2.0 (25): ab_glyph, ab_glyph_rasterizer, accesskit_winit, approx, clang-sys, codespan-reporting, cpal, gethostname, gl_generator, glutin, glutin_egl_sys, glutin_glx_sys, glutin_wgl_sys, khronos_api, lzma-rust2, openssl, owned_ttf_parser, prost, prost-derive, speakrs, spirv, sync_wrapper, unicode-general-category, winit, zopfli
Apache-2.0 AND ISC (1): ring
Apache-2.0 AND MIT (1): dpi
Apache-2.0 OR Apache-2.0 WITH LLVM-exception OR MIT (7): linux-raw-sys, linux-raw-sys, rustix, rustix, wasi, wasip2, wit-bindgen
Apache-2.0 OR BSD-2-Clause OR MIT (3): mach2, zerocopy, zerocopy-derive
Apache-2.0 OR BSD-3-Clause (2): moxcms, pxfm
Apache-2.0 OR BSD-3-Clause OR MIT (2): num_enum, num_enum_derive
Apache-2.0 OR BSL-1.0 (1): ryu
Apache-2.0 OR GPL-2.0 (1): self_cell
Apache-2.0 OR ISC OR MIT (2): hyper-rustls, rustls
Apache-2.0 OR LGPL-2.1-or-later OR MIT (2): r-efi, r-efi
Apache-2.0 OR MIT (462): accesskit, accesskit_atspi_common, accesskit_consumer, accesskit_consumer, accesskit_consumer, accesskit_macos, accesskit_unix, accesskit_windows, aes, ahash, ahash, allocator-api2, alsa, android-activity, android_system_properties, anstream, anstyle, anstyle-parse, anstyle-query, anstyle-wincon, anyhow, anymap2, apple-native-keyring-store, arbitrary, arboard, arrayvec, as-raw-xcb-connection, ash, async-broadcast, async-channel, async-executor, async-io, async-lock, async-process, async-recursion, async-signal, async-task, async-trait, atomic-waker, atspi, atspi-common, atspi-proxies, audio-core, audioadapter, audioadapter-buffers, audioadapter-sample, autocfg, base64, base64ct, bit-set, bit-set, bit-vec, bit-vec, bitflags, bitflags, block-buffer, block-padding, blocking, bumpalo, cbc, cblas-sys, cc, cexpr, cfg-if, cgl, chacha20, chrono, cipher, clap, clap_builder, clap_derive, clap_lex, cmake, color, colorchoice, concurrent-queue, const-random, const-random-macro, core-foundation, core-foundation, core-foundation-sys, core-graphics, core-graphics-types, coreaudio-rs, cpufeatures, cpufeatures, crc32fast, crossbeam-channel, crossbeam-deque, crossbeam-epoch, crossbeam-utils, crypto-common, ctrlc, dasp_sample, deep_filter, der, deranged, derive_arbitrary, digest, directories, dirs, dirs-sys, displaydoc, dlv-list, document-features, downcast-rs, dyn-clone, ecolor, eframe, egui, egui-wgpu, egui-winit, egui_extras, egui_glow, either, emath, enum-map, enum-map-derive, enumflags2, enumflags2_derive, enumn, epaint, equivalent, errno, euclid, event-listener, event-listener-strategy, fallible-iterator, fallible-streaming-iterator, fastrand, fd-lock, fdeflate, fearless_simd, filetime, find-msvc-tools, flate2, fluent, fluent-bundle, fluent-langneg, fluent-syntax, flume, font-types, foreign-types, foreign-types, foreign-types-macros, foreign-types-shared, foreign-types-shared, form_urlencoded, futures-channel, futures-core, futures-io, futures-lite, futures-macro, futures-sink, futures-task, futures-util, getrandom, getrandom, getrandom, glifo, glob, gpu-allocator, gpu-descriptor, gpu-descriptor-types, guillotiere, half, hashbrown, hashbrown, hashbrown, hashbrown, hashbrown, hashlink, heck, hermit-abi, hex, hkdf, hmac, home, http, httparse, iana-time-zone, iana-time-zone-haiku, idna, idna_adapter, image, indexmap, inout, intl-memoizer, intl_pluralrules, ipnet, is_terminal_polyfill, itertools, itertools, itertools, itoa, jni, jni-macros, jni-sys, jni-sys, jni-sys-macros, jobserver, js-sys, katexit, keyring, keyring-core, khronos-egl, kstring, kurbo, lapack-sys, lax, lazy_static, libc, linebender_resource_handle, liquid, liquid-core, liquid-derive, liquid-lib, litrs, lock_api, log, maplit, matrixmultiply, memmap2, memmap2, mime, minimal-lexical, naga, native-tls, ndarray, ndarray, ndarray-linalg, ndarray-npy, ndk, ndk-context, ndk-sys, nohash-hasher, notify-types, num, num-bigint, num-complex, num-conv, num-derive, num-integer, num-iter, num-rational, num-traits, once_cell, once_cell_polyfill, openblas-build, openblas-src, openssl-macros, openssl-probe, ordered-stream, ort, ort-sys, parking, parking_lot, parking_lot_core, paste, pem-rfc7468, peniko, percent-encoding, pest, pest_derive, pest_generator, pest_meta, pin-project, pin-project-internal, pin-project-lite, piper, pkg-config, plain, png, polling, pollster, polycool, portable-atomic, portable-atomic-util, powerfmt, ppv-lite86, presser, prettyplease, primal-check, proc-macro-crate, proc-macro-hack, proc-macro2, profiling, py_literal, quick-error, quinn, quinn-proto, quinn-udp, quote, rand, rand, rand_chacha, rand_core, rand_core, rand_distr, rand_pcg, range-alloc, raw-window-metal, rawpointer, rayon, rayon-core, read-fonts, regex, regex-automata, regex-syntax, renderdoc-sys, reqwest, ron, rtrb, rubato, rustc-hash, rustc-hash, rustc_version, rustfft, rustls-pki-types, rustversion, scoped-tls, scopeguard, secret-service, security-framework, security-framework-sys, semver, serde, serde_core, serde_derive, serde_json, serde_repr, serde_spanned, serde_urlencoded, sha2, shlex, shlex, signal-hook-registry, simd_cesu8, simdutf8, siphasher, skrifa, smallvec, smol_str, socket2, socks, stable_deref_trait, static_assertions, strength_reduce, string-interner, syn, syn, syn, tar, tempfile, thiserror, thiserror, thiserror-impl, thiserror-impl, time, time-core, time-macros, tokio-rustls, toml, toml_datetime, toml_datetime, toml_edit, toml_parser, toml_writer, tract-core, tract-data, tract-hir, tract-linalg, tract-nnef, tract-onnx, tract-onnx-opl, tract-pulse, tract-pulse-opl, transpose, ttf-parser, type-map, typed-path, typeid, typenum, ucd-trie, unic-langid, unic-langid-impl, unic-langid-macros, unic-langid-macros-impl, unicase, unicode-normalization, unicode-segmentation, unicode-width, ureq, ureq-proto, url, utf8-zero, utf8_iter, utf8parse, uuid, vcpkg, vello_common, vello_cpu, version_check, wasm-bindgen, wasm-bindgen-futures, wasm-bindgen-macro, wasm-bindgen-macro-support, wasm-bindgen-shared, web-sys, web-time, webbrowser, weezl, wgpu, wgpu-core, wgpu-core-deps-apple, wgpu-core-deps-emscripten, wgpu-core-deps-wasm, wgpu-core-deps-windows-linux-android, wgpu-hal, wgpu-naga-bridge, wgpu-types, winapi, winapi-i686-pc-windows-gnu, winapi-x86_64-pc-windows-gnu, windows, windows-collections, windows-core, windows-future, windows-implement, windows-interface, windows-link, windows-native-keyring-store, windows-numerics, windows-result, windows-strings, windows-sys, windows-sys, windows-sys, windows-targets, windows-targets, windows-threading, windows_aarch64_gnullvm, windows_aarch64_gnullvm, windows_aarch64_msvc, windows_aarch64_msvc, windows_i686_gnu, windows_i686_gnu, windows_i686_gnullvm, windows_i686_gnullvm, windows_i686_msvc, windows_i686_msvc, windows_x86_64_gnu, windows_x86_64_gnu, windows_x86_64_gnullvm, windows_x86_64_gnullvm, windows_x86_64_msvc, windows_x86_64_msvc, x11rb, x11rb-protocol, xattr, zbus-secret-service-keyring-store, zeroize
Apache-2.0 OR MIT OR Zlib (28): bytemuck, bytemuck_derive, cursor-icon, dispatch2, glow, lru-slab, miniz_oxide, objc2-app-kit, objc2-audio-toolbox, objc2-avf-audio, objc2-core-audio, objc2-core-audio-types, objc2-core-foundation, objc2-core-graphics, objc2-core-ml, objc2-core-video, objc2-image-io, objc2-io-surface, objc2-metal, objc2-quartz-core, objc2-ui-kit, raw-window-handle, tinyvec, tinyvec_macros, visibility, xkeysym, zune-core, zune-jpeg
BSD-2-Clause (1): arrayref
BSD-3-Clause (4): bindgen, subtle, tiny-skia, tiny-skia-path
BSL-1.0 (2): clipboard-win, error-code
CC0-1.0 (3): hexf-parse, notify, tiny-keccak
CDLA-Permissive-2.0 (2): webpki-root-certs, webpki-roots
ISC (6): hmac-sha256, inotify, inotify-sys, libloading, rustls-webpki, untrusted
MIT (150): alsa-sys, android-properties, block2, block2, bytes, calloop, calloop, calloop-wayland-source, calloop-wayland-source, cauchy, cfg_aliases, combine, crunchy, derive-new, dispatch, dlib, educe, endi, enum-ordinalize, fax, fs_extra, fsevent-sys, generic-array, glutin-winit, harfrust, http-body, http-body-util, humm-audio, humm-cli, humm-core, humm-dfn, humm-diarize, humm-gui, humm-media, humm-store, humm-whisper, hyper, hyper-util, is-docker, is-wsl, kodama, kqueue, kqueue-sys, libm, libredox, libsqlite3-sys, memoffset, mime_guess, mime_guess2, mio, nix, nom, objc-sys, objc2, objc2, objc2-app-kit, objc2-cloud-kit, objc2-contacts, objc2-core-data, objc2-core-image, objc2-core-location, objc2-encode, objc2-foundation, objc2-foundation, objc2-link-presentation, objc2-metal, objc2-quartz-core, objc2-symbols, objc2-ui-kit, objc2-uniform-type-identifiers, objc2-user-notifications, open, openssl-sys, orbclient, ordered-float, ordered-multimap, phf, phf, phf_generator, phf_generator, phf_macros, phf_macros, phf_shared, phf_shared, quick-xml, realfft, redox_syscall, redox_syscall, redox_syscall, redox_users, rfd, rsqlite-vfs, rubato, rusqlite, rust-ini, scan_fmt, schannel, sctk-adwaita, simd-adler32, slab, smithay-client-toolkit, smithay-client-toolkit, smithay-clipboard, spin, sqlite-wasm-rs, strict-num, strsim, synstructure, tiff, tokio, tower, tower-http, tower-layer, tower-service, tracing, tracing-attributes, tracing-core, try-lock, uds_windows, want, wayland-backend, wayland-client, wayland-csd-frame, wayland-cursor, wayland-protocols, wayland-protocols-experimental, wayland-protocols-misc, wayland-protocols-plasma, wayland-protocols-wlr, wayland-scanner, wayland-sys, windowfunctions, winnow, winnow, x11-dl, xcursor, xkbcommon-dl, xml-rs, zbus, zbus-lockstep, zbus-lockstep-macros, zbus_macros, zbus_names, zbus_xml, zip, zip, zmij, zvariant, zvariant_derive, zvariant_utils
MIT OR Unlicense (10): aho-corasick, byteorder, byteorder-lite, csv, csv-core, memchr, same-file, termcolor, walkdir, winapi-util
MPL-2.0 (1): option-ext
Unicode-3.0 (18): icu_collections, icu_locale_core, icu_normalizer, icu_normalizer_data, icu_properties, icu_properties_data, icu_provider, litemap, potential_utf, tinystr, writeable, yoke, yoke-derive, zerofrom, zerofrom-derive, zerotrie, zerovec, zerovec-derive
Unlicense (2): whisper-rs, whisper-rs-sys
Zlib (5): foldhash, foldhash, nanorand, slotmap, zlib-rs
```

Notable entries cross-referenced in `NOTICE`: `speakrs` (Apache-2.0),
`deep_filter` (Apache-2.0 OR MIT — DeepFilterNet's own dual license),
`whisper-rs`/`whisper-rs-sys` (Unlicense), `ort`/`ort-sys` (Apache-2.0 OR
MIT), `egui`/`eframe` (Apache-2.0 OR MIT), `rusqlite`/`libsqlite3-sys` (MIT
— the bundled SQLite C amalgamation itself is public domain, separate from
the `rusqlite`/`libsqlite3-sys` Rust binding crates' own MIT license),
`cpal` (Apache-2.0), `rubato` (MIT).

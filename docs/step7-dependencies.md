# 第7步新增依賴查證（2026-11-15）

以官方 crates.io API 查版本、發布日 UTC、撤回狀態與 SPDX 授權；所有新增版本均發布滿14天。
直接外掛在 Cargo.toml 以 `=` 精確鎖定；所有間接版本固定於 Cargo.lock。
現有 Tauri 2.12.5 / tauri-build 2.7.4 / Rust 1.99.1 維持不變。

- updater 2.14.1、single-instance 2.5.5：2026-10-28；autostart 2.7.0：2026-10-01。三者 MIT OR Apache-2.0。
- tauri 的 tray-icon feature 使用既有 Tauri 系列與鎖定的 tray-icon，不新增另一套桌面框架。
- reqwest 0.13.5 原在 Cargo.lock 間接存在，現以 `=0.13.5` 直接引用；2026-09-08 發布，MIT OR Apache-2.0、未撤回。用於更新器 HTTPS/逐跳轉址白名單，`default-features=false` 沿用外掛的 TLS provider。[官方版本](https://crates.io/api/v1/crates/reqwest/0.13.5)
- 避開未滿兩週的 async-executor 1.14.1、async-lock 3.5.0、errno 0.3.15、webpki-root-certs 1.0.12、zip 9.5.1，分別固定到下表版本。
- webpki-root-certs 使用 CDLA-Permissive-2.0；這是可隨程式散布的寬鬆資料授權，正式打包時連同完整授權全文列入 SBOM/第三方聲明。
- 官方 API 格式：`https://crates.io/api/v1/crates/<name>/<version>`；下表各列均直接連至實際版本。

| 套件 | 版本 | 發布日（UTC） | SPDX 授權 | 官方來源 |
|---|---|---|---|---|
| async-broadcast | 0.7.2 | 2024-12-23 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/async-broadcast/0.7.2) |
| async-channel | 2.5.0 | 2025-07-06 | Apache-2.0 OR MIT | [crates.io](https://crates.io/api/v1/crates/async-channel/2.5.0) |
| async-executor | 1.13.3 | 2025-08-27 | Apache-2.0 OR MIT | [crates.io](https://crates.io/api/v1/crates/async-executor/1.13.3) |
| async-io | 2.6.0 | 2025-09-14 | Apache-2.0 OR MIT | [crates.io](https://crates.io/api/v1/crates/async-io/2.6.0) |
| async-lock | 3.4.2 | 2025-12-21 | Apache-2.0 OR MIT | [crates.io](https://crates.io/api/v1/crates/async-lock/3.4.2) |
| async-process | 2.5.0 | 2025-09-14 | Apache-2.0 OR MIT | [crates.io](https://crates.io/api/v1/crates/async-process/2.5.0) |
| async-recursion | 1.2.0 | 2026-10-03 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/async-recursion/1.2.0) |
| async-signal | 0.2.14 | 2026-04-07 | Apache-2.0 OR MIT | [crates.io](https://crates.io/api/v1/crates/async-signal/0.2.14) |
| async-task | 4.7.1 | 2024-04-27 | Apache-2.0 OR MIT | [crates.io](https://crates.io/api/v1/crates/async-task/4.7.1) |
| async-trait | 0.1.92 | 2026-08-08 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/async-trait/0.1.92) |
| auto-launch | 0.6.0 | 2026-01-10 | MIT | [crates.io](https://crates.io/api/v1/crates/auto-launch/0.6.0) |
| blocking | 1.7.0 | 2026-08-23 | Apache-2.0 OR MIT | [crates.io](https://crates.io/api/v1/crates/blocking/1.7.0) |
| cfg_aliases | 0.2.2 | 2026-07-16 | MIT | [crates.io](https://crates.io/api/v1/crates/cfg_aliases/0.2.2) |
| concurrent-queue | 2.5.0 | 2024-04-26 | Apache-2.0 OR MIT | [crates.io](https://crates.io/api/v1/crates/concurrent-queue/2.5.0) |
| core-foundation | 0.9.4 | 2023-11-30 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/core-foundation/0.9.4) |
| dirs | 6.0.0 | 2025-01-12 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/dirs/6.0.0) |
| endi | 1.1.1 | 2025-11-26 | MIT | [crates.io](https://crates.io/api/v1/crates/endi/1.1.1) |
| enumflags2 | 0.7.12 | 2025-06-09 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/enumflags2/0.7.12) |
| enumflags2_derive | 0.7.12 | 2025-06-09 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/enumflags2_derive/0.7.12) |
| errno | 0.3.14 | 2025-09-09 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/errno/0.3.14) |
| event-listener | 5.4.2 | 2026-07-27 | Apache-2.0 OR MIT | [crates.io](https://crates.io/api/v1/crates/event-listener/5.4.2) |
| event-listener-strategy | 0.5.4 | 2025-03-27 | Apache-2.0 OR MIT | [crates.io](https://crates.io/api/v1/crates/event-listener-strategy/0.5.4) |
| filetime | 0.2.29 | 2026-05-12 | MIT/Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/filetime/0.2.29) |
| futures-lite | 2.6.1 | 2025-08-04 | Apache-2.0 OR MIT | [crates.io](https://crates.io/api/v1/crates/futures-lite/2.6.1) |
| hermit-abi | 0.5.3 | 2026-08-31 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/hermit-abi/0.5.3) |
| hyper-rustls | 0.27.10 | 2026-09-20 | Apache-2.0 OR ISC OR MIT | [crates.io](https://crates.io/api/v1/crates/hyper-rustls/0.27.10) |
| jni | 0.22.4 | 2026-03-16 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/jni/0.22.4) |
| jni-macros | 0.22.4 | 2026-03-16 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/jni-macros/0.22.4) |
| linux-raw-sys | 0.12.1 | 2025-12-23 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | [crates.io](https://crates.io/api/v1/crates/linux-raw-sys/0.12.1) |
| minisign-verify | 0.3.0 | 2026-09-25 | MIT | [crates.io](https://crates.io/api/v1/crates/minisign-verify/0.3.0) |
| nix | 0.31.3 | 2026-05-11 | MIT | [crates.io](https://crates.io/api/v1/crates/nix/0.31.3) |
| objc2-osa-kit | 0.3.2 | 2025-10-04 | Zlib OR Apache-2.0 OR MIT | [crates.io](https://crates.io/api/v1/crates/objc2-osa-kit/0.3.2) |
| objc2-security | 0.3.2 | 2025-10-04 | Zlib OR Apache-2.0 OR MIT | [crates.io](https://crates.io/api/v1/crates/objc2-security/0.3.2) |
| objc2-service-management | 0.3.2 | 2025-10-04 | Zlib OR Apache-2.0 OR MIT | [crates.io](https://crates.io/api/v1/crates/objc2-service-management/0.3.2) |
| openssl-probe | 0.2.2 | 2026-10-26 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/openssl-probe/0.2.2) |
| ordered-stream | 0.2.0 | 2023-02-11 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/ordered-stream/0.2.0) |
| os_info | 3.15.0 | 2026-05-19 | MIT | [crates.io](https://crates.io/api/v1/crates/os_info/3.15.0) |
| osakit | 0.3.1 | 2025-03-01 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/osakit/0.3.1) |
| parking | 2.2.1 | 2024-09-07 | Apache-2.0 OR MIT | [crates.io](https://crates.io/api/v1/crates/parking/2.2.1) |
| piper | 0.2.5 | 2026-02-28 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/piper/0.2.5) |
| polling | 3.11.0 | 2025-09-14 | Apache-2.0 OR MIT | [crates.io](https://crates.io/api/v1/crates/polling/3.11.0) |
| rustix | 1.1.6 | 2026-10-27 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | [crates.io](https://crates.io/api/v1/crates/rustix/1.1.6) |
| rustls-native-certs | 0.8.5 | 2026-10-13 | Apache-2.0 OR ISC OR MIT | [crates.io](https://crates.io/api/v1/crates/rustls-native-certs/0.8.5) |
| rustls-platform-verifier | 0.7.1 | 2026-09-24 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/rustls-platform-verifier/0.7.1) |
| rustls-platform-verifier-android | 0.2.0 | 2026-09-24 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/rustls-platform-verifier-android/0.2.0) |
| schannel | 0.1.30 | 2026-10-16 | MIT | [crates.io](https://crates.io/api/v1/crates/schannel/0.1.30) |
| security-framework | 3.7.0 | 2026-02-20 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/security-framework/3.7.0) |
| security-framework-sys | 2.17.0 | 2026-02-20 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/security-framework-sys/2.17.0) |
| signal-hook-registry | 1.4.8 | 2025-12-25 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/signal-hook-registry/1.4.8) |
| simd_cesu8 | 1.2.0 | 2026-07-13 | Apache-2.0 OR MIT | [crates.io](https://crates.io/api/v1/crates/simd_cesu8/1.2.0) |
| simdutf8 | 0.1.5 | 2024-09-22 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/simdutf8/0.1.5) |
| smappservice-rs | 0.1.3 | 2025-05-27 | MIT | [crates.io](https://crates.io/api/v1/crates/smappservice-rs/0.1.3) |
| system-configuration | 0.7.0 | 2025-12-02 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/system-configuration/0.7.0) |
| system-configuration-sys | 0.6.0 | 2024-01-31 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/system-configuration-sys/0.6.0) |
| tar | 0.4.46 | 2026-05-18 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/tar/0.4.46) |
| tauri-plugin-autostart | 2.7.0 | 2026-10-01 | Apache-2.0 OR MIT | [crates.io](https://crates.io/api/v1/crates/tauri-plugin-autostart/2.7.0) |
| tauri-plugin-single-instance | 2.5.5 | 2026-10-28 | Apache-2.0 OR MIT | [crates.io](https://crates.io/api/v1/crates/tauri-plugin-single-instance/2.5.5) |
| tauri-plugin-updater | 2.14.1 | 2026-10-28 | Apache-2.0 OR MIT | [crates.io](https://crates.io/api/v1/crates/tauri-plugin-updater/2.14.1) |
| tempfile | 3.27.0 | 2026-03-11 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/tempfile/3.27.0) |
| tokio-rustls | 0.26.6 | 2026-09-28 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/tokio-rustls/0.26.6) |
| tracing-attributes | 0.1.31 | 2025-11-26 | MIT | [crates.io](https://crates.io/api/v1/crates/tracing-attributes/0.1.31) |
| typed-path | 0.13.0 | 2026-10-20 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/typed-path/0.13.0) |
| uds_windows | 1.2.1 | 2026-03-14 | MIT | [crates.io](https://crates.io/api/v1/crates/uds_windows/1.2.1) |
| webpki-root-certs | 1.0.11 | 2026-10-23 | CDLA-Permissive-2.0 | [crates.io](https://crates.io/api/v1/crates/webpki-root-certs/1.0.11) |
| windows-registry | 0.6.1 | 2025-10-06 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/windows-registry/0.6.1) |
| xattr | 1.7.0 | 2026-10-28 | MIT OR Apache-2.0 | [crates.io](https://crates.io/api/v1/crates/xattr/1.7.0) |
| zbus | 5.19.0 | 2026-08-09 | MIT | [crates.io](https://crates.io/api/v1/crates/zbus/5.19.0) |
| zbus_macros | 5.19.0 | 2026-08-09 | MIT | [crates.io](https://crates.io/api/v1/crates/zbus_macros/5.19.0) |
| zbus_names | 4.3.5 | 2026-10-16 | MIT | [crates.io](https://crates.io/api/v1/crates/zbus_names/4.3.5) |
| zcheapstr | 1.2.0 | 2026-10-29 | MIT | [crates.io](https://crates.io/api/v1/crates/zcheapstr/1.2.0) |
| zip | 9.2.0 | 2026-10-31 | MIT | [crates.io](https://crates.io/api/v1/crates/zip/9.2.0) |
| zvariant | 5.15.1 | 2026-10-16 | MIT | [crates.io](https://crates.io/api/v1/crates/zvariant/5.15.1) |
| zvariant_derive | 5.15.1 | 2026-10-16 | MIT | [crates.io](https://crates.io/api/v1/crates/zvariant_derive/5.15.1) |
| zvariant_utils | 4.2.0 | 2026-08-18 | MIT | [crates.io](https://crates.io/api/v1/crates/zvariant_utils/4.2.0) |

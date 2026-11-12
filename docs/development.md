# Development

## Building and testing

Requires Rust (MSVC toolchain) and the Tauri CLI (`cargo install tauri-cli`).

```
cargo build --release -p kit-apo            # first: the app's build checks this DLL exists (it bundles it)
cargo build -p kit && target\debug\kit.exe  # run the app (debug)
cd app/src-tauri && cargo tauri build       # release exe + installer (needs the updater signing key)
cargo test                                  # unit tests (VIA codes and macros, EQ and lighting config, denoise, surround)
cargo clippy --workspace --all-targets      # lints; kept free of warnings
cargo run --release -p spatial --example demo -- out   # surround demo WAVs and renderer timing
cargo run -p mouse-probe                    # mouse hardware test: device info
cargo run -p mouse-probe -- dpi 800         # set DPI directly
cargo run -p mouse-probe -- keyboard        # Logitech keyboard: features, lighting zones and effects
cargo run -p headset-probe                  # headset hardware test: status, EQ, settings
cargo run -p headset-probe -- devices       # audio endpoints and defaults
cargo run -p headset-probe -- capture out game direct   # measure Sonar's Game path (see docs/sonar-eq.md)
cargo run -p headset-probe -- kit-dll target/release/kit_apo.dll out   # kit's EQ DLL, offline, against that
```

`scripts/ui-preview.sh` serves the UI in a normal browser against a fake backend (`scripts/ui-mock.js`), for UI work
on a machine without the devices.

Releases are built by GitHub Actions (`.github/workflows/release.yml`). `scripts/release.sh X.Y.Z` takes the list in
`UNRELEASED.md` as the release notes, bumps the version, tags and pushes; CI builds, signs and publishes the
installer, and the in-app updater shows the notes.

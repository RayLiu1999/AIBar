# 列出可用的任務
default:
    @just --list

# 格式、clippy（警告視為錯誤）與測試，內容與 CI 相同
check:
    cargo fmt --check
    cargo clippy --all-targets --locked -- -D warnings
    cargo test --locked

# 打包 dist/AIBar.app；簽章身分由 AIBAR_SIGN_IDENTITY 指定，未設定時為 ad-hoc
[macos]
bundle-macos:
    #!/usr/bin/env bash
    set -euo pipefail
    export MACOSX_DEPLOYMENT_TARGET=13.0
    target_dir="${CARGO_TARGET_DIR:-target}"
    version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
    cargo build --release --locked
    app=dist/AIBar.app
    rm -rf "$app"
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
    cp "$target_dir/release/aibar" "$app/Contents/MacOS/aibar"
    sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
    cargo run --quiet --locked --example make_icns -- "$app/Contents/Resources/AIBar.icns"
    plutil -lint -s "$app/Contents/Info.plist"
    codesign --force --options runtime --sign "${AIBAR_SIGN_IDENTITY:--}" "$app"
    codesign --verify --strict "$app"
    echo "已產生 ${app}（${version}）"

# 打包並安裝到 ~/Applications
[macos]
install-macos: bundle-macos
    mkdir -p ~/Applications
    rm -rf ~/Applications/AIBar.app
    cp -R dist/AIBar.app ~/Applications/

#!/usr/bin/env bash
# Sourced by build-apk.sh after its SDK paths, runtime pins and source root are loaded.

# link_apk_resources <manifest> <assets> <out_dir>: compile the shared product icon and link an
# unsigned APK. Uses the caller's build_tools, android_jar, min_sdk, repo_root and pin function.
link_apk_resources() {
    local manifest="$1" assets="$2" output="$3"
    local resources="$output/launcher-res"
    local icon_size=192
    rm -rf "$resources"
    mkdir -p "$resources/mipmap-xxxhdpi" "$resources/drawable-nodpi" "$resources/drawable" "$resources/values" "$resources/values-v31"
    install -m 0644 "$repo_root/assets/branding/title.png" "$assets/startup-title.png"
    install -m 0644 "$repo_root/assets/startup-screen.json" "$assets/startup-screen.json"
    python3 "$repo_root/packaging/android/startup-resources.py" "$repo_root/assets/startup-screen.json" "$resources"
    bash "$repo_root/packaging/icons/render.sh" "$output/icons" "$icon_size"
    install -m 0644 "$output/icons/icon-$icon_size.png" "$resources/mipmap-xxxhdpi/ic_launcher.png"
    "$build_tools/aapt2" compile --dir "$resources" -o "$output/resources.zip"
    "$build_tools/aapt2" link -o "$output/unsigned.apk" -I "$android_jar" \
        --manifest "$manifest" -A "$assets" -R "$output/resources.zip" \
        --min-sdk-version "$min_sdk" --target-sdk-version "$(pin target_sdk)"
}

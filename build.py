#!/usr/bin/env python3
"""
Cross-platform build script for Unity Native BLE plugin.
Builds for macOS, iOS, Windows, Linux, and Android from any platform.
"""

import argparse
import os
import platform
import subprocess
import shutil
import sys
from pathlib import Path
from typing import List, Tuple

# Platform-specific configurations
PLATFORMS = {
    "macos-intel": {
        "target": "x86_64-apple-darwin",
        "output_dir": "Runtime/Plugins/macOS",
        "output_name": "unity_native_ble.bundle",
        "type": "cdylib",
        "rustup_target": "x86_64-apple-darwin",
        "host_platforms": ["darwin"],
    },
    "macos-arm": {
        "target": "aarch64-apple-darwin",
        "output_dir": "Runtime/Plugins/macOS",
        "output_name": "unity_native_ble.bundle",
        "type": "cdylib",
        "rustup_target": "aarch64-apple-darwin",
        "host_platforms": ["darwin"],
    },
    "macos-universal": {
        "target": None,  # Special case
        "output_dir": "Runtime/Plugins/macOS",
        "output_name": "unity_native_ble.bundle",
        "type": "universal",
        "host_platforms": ["darwin"],
        "optional": True,
    },
    "ios": {
        "target": "aarch64-apple-ios",
        "output_dir": "Runtime/Plugins/iOS",
        "output_name": "unity_native_ble.framework",
        "type": "staticlib",
        "rustup_target": "aarch64-apple-ios",
        "host_platforms": ["darwin"],
    },
    "android-arm64": {
        "target": "aarch64-linux-android",
        "output_dir": "Runtime/Plugins/Android/arm64-v8a",
        "output_name": "libunity_native_ble.so",
        "type": "cdylib",
        "rustup_target": "aarch64-linux-android",
        "host_platforms": ["darwin", "linux", "windows"],
        "optional": True,
    },
    "android-arm": {
        "target": "armv7-linux-androideabi",
        "output_dir": "Runtime/Plugins/Android/armeabi-v7a",
        "output_name": "libunity_native_ble.so",
        "type": "cdylib",
        "rustup_target": "armv7-linux-androideabi",
        "host_platforms": ["darwin", "linux", "windows"],
        "optional": True,
    },
    "windows": {
        "target": "x86_64-pc-windows-gnu",
        "output_dir": "Runtime/Plugins/Windows",
        "output_name": "unity_native_ble.dll",
        "type": "cdylib",
        "rustup_target": "x86_64-pc-windows-gnu",
        "host_platforms": ["darwin", "linux"],
        "requires": ["x86_64-w64-mingw32-dlltool"],
        "optional": True,
    },
    "linux": {
        "target": "x86_64-unknown-linux-gnu",
        "output_dir": "Runtime/Plugins/Linux",
        "output_name": "libunity_native_ble.so",
        "type": "cdylib",
        "rustup_target": "x86_64-unknown-linux-gnu",
        "host_platforms": ["darwin", "windows"],
        "requires_custom_setup": True,
        "optional": True,
    },
}

# Current platform detection
CURRENT_PLATFORM = platform.system().lower()


def run_command(cmd: List[str], cwd: Path = None) -> Tuple[bool, str]:
    """Run a command and return success status and output."""
    try:
        result = subprocess.run(
            cmd,
            cwd=cwd,
            capture_output=True,
            text=True,
            check=False
        )
        return result.returncode == 0, result.stdout + result.stderr
    except Exception as e:
        return False, str(e)


def check_tool(tool: str) -> bool:
    """Check if a tool is available."""
    return shutil.which(tool) is not None


def check_rustup() -> bool:
    """Check if rustup is available."""
    return check_tool("rustup")


def check_cargo() -> bool:
    """Check if cargo is available."""
    return check_tool("cargo")


def install_rust_target(target: str) -> bool:
    """Install a Rust target if not already installed."""
    success, output = run_command([
        "rustup", "target", "list", "--installed"])
    if success and target in output:
        print(f"  ✓ Target {target} already installed")
        return True

    print(f"  → Installing Rust target: {target}")
    success, output = run_command([
        "rustup", "target", "add", target])
    if not success:
        print(f"  ✗ Failed to install target {target}: {output}")
        return False
    print(f"  ✓ Target {target} installed")
    return True


def build_for_target(target: str, build_type: str = "release", crate_type: str = None) -> Tuple[bool, Path]:
    """Build the library for a specific target."""
    print(f"\n🔨 Building for target: {target}")

    if crate_type:
        cmd = ["cargo", "rustc", f"--{build_type}", "--crate-type", crate_type]
    else:
        cmd = ["cargo", "build", f"--{build_type}"]

    if target != CURRENT_PLATFORM:
        cmd.extend(["--target", target])

    success, output = run_command(cmd)
    if not success:
        print(f"  ✗ Build failed: {output}")
        return False, None

    # Determine output path
    if target != CURRENT_PLATFORM:
        output_path = Path("target") / target / build_type
    else:
        output_path = Path("target") / build_type

    return True, output_path


def create_macos_bundle(output_path: Path, dest_path: Path) -> bool:
    """Create a macOS bundle from the dylib."""
    print(f"  → Creating macOS bundle: {dest_path}")

    try:
        # Remove existing bundle
        if dest_path.exists():
            shutil.rmtree(dest_path)

        # Create bundle structure
        dest_path.mkdir(parents=True, exist_ok=True)
        contents_dir = dest_path / "Contents"
        contents_dir.mkdir(exist_ok=True)

        macos_dir = contents_dir / "MacOS"
        macos_dir.mkdir(exist_ok=True)

        # Copy dylib
        dylib_name = "libunity_native_ble.dylib"
        src_dylib = output_path / dylib_name
        dst_dylib = macos_dir / "unity_native_ble"

        if not src_dylib.exists():
            print(f"  ✗ Source dylib not found: {src_dylib}")
            return False

        shutil.copy2(src_dylib, dst_dylib)

        # Create Info.plist
        info_plist = contents_dir / "Info.plist"
        info_plist.write_text("""<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>English</string>
    <key>CFBundleExecutable</key>
    <string>unity_native_ble</string>
    <key>CFBundleIdentifier</key>
    <string>com.unity.native.ble</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>Unity Native BLE</string>
    <key>CFBundlePackageType</key>
    <string>BNDL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>CSResourcesFileMapped</key>
    <true/>
    <key>NSBluetoothAlwaysUsageDescription</key>
    <string>Bluetooth is used to connect to nearby BLE devices.</string>
    <key>NSBluetoothPeripheralUsageDescription</key>
    <string>Bluetooth is used to connect to nearby BLE devices.</string>
</dict>
</plist>
""")

        print(f"  ✓ Bundle created successfully")
        return True

    except Exception as e:
        print(f"  ✗ Failed to create bundle: {e}")
        return False


def create_ios_framework(output_path: Path, dest_path: Path) -> bool:
    """Create an iOS framework from the staticlib."""
    print(f"  → Creating iOS framework: {dest_path}")

    try:
        # Remove existing framework
        if dest_path.exists():
            shutil.rmtree(dest_path)

        # Create framework structure
        dest_path.mkdir(parents=True, exist_ok=True)

        # Copy staticlib
        lib_name = "libunity_native_ble.a"
        src_lib = output_path / lib_name
        dst_lib = dest_path / "unity_native_ble"

        if not src_lib.exists():
            print(f"  ✗ Source staticlib not found: {src_lib}")
            return False

        shutil.copy2(src_lib, dst_lib)

        # Create Info.plist
        info_plist = dest_path / "Info.plist"
        info_plist.write_text("""<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleExecutable</key>
    <string>unity_native_ble</string>
    <key>CFBundleIdentifier</key>
    <string>com.unity.native.ble</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>Unity Native BLE</string>
    <key>CFBundlePackageType</key>
    <string>FMWK</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>MinimumOSVersion</key>
    <string>12.0</string>
    <key>NSBluetoothAlwaysUsageDescription</key>
    <string>Bluetooth is used to connect to nearby BLE devices.</string>
    <key>NSBluetoothPeripheralUsageDescription</key>
    <string>Bluetooth is used to connect to nearby BLE devices.</string>
</dict>
</plist>
""")

        print(f"  ✓ Framework created successfully")
        return True

    except Exception as e:
        print(f"  ✗ Failed to create framework: {e}")
        return False


def copy_file(src_file: Path, dest_path: Path) -> bool:
    """Copy a file to the destination."""
    print(f"  → Copying to: {dest_path}")
    try:
        dest_path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(src_file, dest_path)
        print(f"  ✓ Copied successfully")
        return True
    except Exception as e:
        print(f"  ✗ Failed to copy: {e}")
        return False


def can_build_platform(platform_key: str) -> Tuple[bool, str]:
    """Check if a platform can be built on the current host."""
    config = PLATFORMS[platform_key]

    # Check if host platform is supported
    if "host_platforms" in config:
        if CURRENT_PLATFORM not in config["host_platforms"]:
            return False, f"Cannot build {platform_key} on {CURRENT_PLATFORM} (requires: {', '.join(config['host_platforms'])})"

    # Check for Android NDK
    if platform_key.startswith("android"):
        ndk_path = os.environ.get("ANDROID_NDK_HOME") or os.environ.get("NDK_HOME")
        if not ndk_path:
            return False, "Android NDK not found. Set ANDROID_NDK_HOME environment variable"
        if not Path(ndk_path).exists():
            return False, f"Android NDK not found at {ndk_path}"

    # Check for required tools
    if "requires" in config:
        for tool in config["requires"]:
            if not check_tool(tool):
                return False, f"Missing required tool '{tool}'. Install with: brew install mingw-w64"

    # Check for custom setup requirements
    if config.get("requires_custom_setup", False):
        return False, f"{platform_key} requires custom cross-compilation setup (sysroot, pkg-config)"

    return True, ""


def build_platform(platform_key: str) -> bool:
    """Build for a specific platform."""
    config = PLATFORMS[platform_key]

    # Handle universal macOS
    if platform_key == "macos-universal":
        print(f"\n🔨 Building macOS universal binary")

        # Build for both architectures
        if not build_platform("macos-intel"):
            return False
        if not build_platform("macos-arm"):
            return False

        # Use lipo to create universal dynamic library
        if not check_tool("lipo"):
            print("  ⚠️  lipo not found. Skipping universal binary")
            return True

        print("  → Creating universal dylib with lipo")
        intel_dylib = Path("target") / "x86_64-apple-darwin" / "release" / "libunity_native_ble.dylib"
        arm_dylib = Path("target") / "aarch64-apple-darwin" / "release" / "libunity_native_ble.dylib"
        universal_dir = Path("target") / "universal-apple-darwin" / "release"
        universal_dylib = universal_dir / "libunity_native_ble.dylib"

        if not intel_dylib.exists() or not arm_dylib.exists():
            print(f"  ⚠️  Required dylibs not found. Skipping universal binary")
            return True

        universal_dir.mkdir(parents=True, exist_ok=True)
        success, output = run_command([
            "lipo", "-create", str(intel_dylib), str(arm_dylib),
            "-output", str(universal_dylib)])
        if not success:
            print(f"  ⚠️  Failed to create universal dylib: {output[:200]}")
            print("  ⚠️  Skipping universal binary (you still have separate ARM and Intel builds)")
            return True

        dest_path = Path(config["output_dir"]) / config["output_name"]
        return create_macos_bundle(universal_dir, dest_path)

    # Install Rust target if needed
    if "rustup_target" in config:
        if not install_rust_target(config["rustup_target"]):
            return False

    # Build the library
    target = config.get("target")
    if target:
        crate_type = config.get("type") if config.get("type") == "staticlib" else None
        success, output_path = build_for_target(target, crate_type=crate_type)
        if not success:
            return False

        # Determine source file based on target platform
        if config["type"] == "cdylib":
            if platform_key.startswith("macos"):
                src_file = output_path / "libunity_native_ble.dylib"
            elif platform_key == "windows":
                # MinGW or MSVC may produce libunity_native_ble.dll or unity_native_ble.dll
                src_file = output_path / "unity_native_ble.dll"
                if not src_file.exists():
                    src_file = output_path / "libunity_native_ble.dll"
            elif platform_key.startswith("android") or platform_key == "linux":
                src_file = output_path / "libunity_native_ble.so"
            else:
                src_file = output_path / config["output_name"]
        else:  # staticlib
            src_file = output_path / "libunity_native_ble.a"

        if not src_file.exists():
            print(f"  ✗ Build output not found: {src_file}")
            return False

        # Copy to destination based on platform
        dest_path = Path(config["output_dir"]) / config["output_name"]

        if platform_key == "ios":
            return create_ios_framework(output_path, dest_path)
        elif platform_key.startswith("macos"):
            return create_macos_bundle(output_path, dest_path)
        else:
            return copy_file(src_file, dest_path)

    return True


def main():
    parser = argparse.ArgumentParser(
        description="Build Unity Native BLE plugin for multiple platforms",
        epilog="Note: Cross-compilation may require additional tools. See README for details."
    )
    parser.add_argument(
        "--all",
        action="store_true",
        help="Build for all platforms (skips unsupported cross-compilation)"
    )
    parser.add_argument(
        "--target",
        choices=list(PLATFORMS.keys()),
        action="append",
        help="Build for specific platform (can be used multiple times)"
    )
    parser.add_argument(
        "--current",
        action="store_true",
        help="Build for current platform only"
    )

    args = parser.parse_args()

    # Check prerequisites
    if not check_cargo():
        print("❌ Cargo not found. Please install Rust: https://rustup.rs/")
        return 1

    # Determine platforms to build
    platforms_to_build = []

    if args.all:
        platforms_to_build = list(PLATFORMS.keys())
    elif args.target:
        platforms_to_build = args.target
    elif args.current:
        # Detect current platform
        if CURRENT_PLATFORM == "darwin":
            # Check if we're on Apple Silicon or Intel
            if platform.machine() == "arm64":
                platforms_to_build = ["macos-arm"]
            else:
                platforms_to_build = ["macos-intel"]
        elif CURRENT_PLATFORM == "windows":
            platforms_to_build = ["windows"]
        elif CURRENT_PLATFORM == "linux":
            platforms_to_build = ["linux"]
        else:
            print(f"❌ Unknown platform: {CURRENT_PLATFORM}")
            return 1
    else:
        # Default: build for current platform
        parser.print_help()
        return 0

    # If macos-universal is requested, exclude individual macos builds to avoid duplication
    if "macos-universal" in platforms_to_build:
        platforms_to_build = [p for p in platforms_to_build if p not in ["macos-intel", "macos-arm"]]

    # Build for each platform
    print(f"\n🚀 Building Unity Native BLE plugin")
    print(f"📦 Platforms: {', '.join(platforms_to_build)}\n")

    failed_platforms = []
    skipped_platforms = []
    for platform_key in platforms_to_build:
        # Check if platform can be built before attempting
        can_build, reason = can_build_platform(platform_key)
        if not can_build:
            if PLATFORMS[platform_key].get("optional", False):
                print(f"⚠️  Skipping {platform_key}: {reason}")
                skipped_platforms.append(platform_key)
                continue
            else:
                print(f"❌ Cannot build {platform_key}: {reason}")
                failed_platforms.append(platform_key)
                continue

        result = build_platform(platform_key)
        if not result:
            failed_platforms.append(platform_key)

    # Summary
    print(f"\n{'='*60}")
    if failed_platforms:
        print(f"❌ Build failed for: {', '.join(failed_platforms)}")
        return 1
    else:
        successful = [p for p in platforms_to_build if p not in failed_platforms and p not in skipped_platforms]
        print(f"✅ All builds successful!")
        if skipped_platforms:
            print(f"⚠️  Skipped (optional/cross-compile): {', '.join(skipped_platforms)}")
        print(f"\n📦 Output directories:")
        for platform_key in successful:
            config = PLATFORMS[platform_key]
            print(f"  • {config['output_dir']}/{config['output_name']}")
        return 0


if __name__ == "__main__":
    sys.exit(main())

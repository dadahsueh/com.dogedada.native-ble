# Unity Native BLE (`com.dogedada.native-ble`)

A high-performance, cross-platform native Bluetooth Low Energy (BLE) plugin for Unity built with Rust, [`btleplug`](https://crates.io/crates/btleplug), and [`interoptopus`](https://crates.io/crates/interoptopus).

Provides thread-safe, non-blocking BLE operations with a native event queue, JSON serialization, and idiomatic C# Unity integration.

---

## Features

### Platform Readiness Matrix

| Platform | Native BLE Backend | Status | Plugin Output | Notes |
| :--- | :--- | :---: | :--- | :--- |
| **macOS** (Apple Silicon + Intel) | Apple CoreBluetooth | **Production Ready** | `unity_native_ble.bundle` | Universal binary (`arm64` + `x86_64`). Works in **Unity Editor** and **macOS Standalone**. |
| **iOS** (`arm64`) | Apple CoreBluetooth | **Supported** | `unity_native_ble.framework` | Compiles as static library. Requires linking `CoreBluetooth.framework` in Xcode. |
| **Windows** (10 / 11 64-bit) | Windows.Devices.Bluetooth (WinRT) | **Supported** | `unity_native_ble.dll` | Requires Windows 10 (Build 15063+) or later. |
| **Linux** (x86_64) | BlueZ (via D-Bus) | **Supported** | `libunity_native_ble.so` | Requires system `libdbus-1` and running `bluetoothd`. |
| **Android** (arm64, armv7) | Android BluetoothGatt | **Incomplete** | `libunity_native_ble.so` | `btleplug` supports Android, but requires a Java/Kotlin JNI bridge (`init(&jvm, activity)`) in Unity's `UnityPlayerActivity`. |
| **OpenHarmony (OHOS)** | OHOS Native BLE C API | **Roadmap / Planned** | `libunity_native_ble.so` | Targets `aarch64-unknown-linux-ohos`. Requires OHOS NDK C binding. See [OpenHarmony Support](#openharmony-ohos-support). |
| **WebGL / Consoles** | N/A | **Not Supported** | N/A | Browser sandboxing and console OS restrictions block native BLE. |

### Key Architecture Highlights
- **Thread-Safe Event Queue**: Rust streams scan, connection, and characteristic notifications into a thread-safe event queue polled cleanly each frame by Unity's `Update()` loop. No main-thread deadlocks or crashes.
- **Safe Memory Marshaling**: Safe buffer copying and pointer bounds checking to prevent stack overflows and memory corruption.
- **Platform-Independent Device IDs**: Resolves macOS/iOS anonymized peripheral UUIDs alongside standard MAC addresses on Windows/Linux.
- **Dual-Layer API**:
  - **High-Level C# API**: `BluetoothManager` MonoBehaviour with C# events (`OnDeviceDiscovered`, `OnConnected`, `OnNotificationReceived`).
  - **Low-Level Native FFI**: Direct P/Invoke bindings via `NativeBLE`.

---

### OpenHarmony (OHOS) Support

OpenHarmony / HarmonyOS NEXT does **not** use BlueZ or Android's Java BLE stack. Instead, OpenHarmony provides its own native subsystem (`communication_bluetooth`) and Native C APIs in the OpenHarmony NDK.

#### How to Add OHOS Support:
1. **Rust Toolchain**: Use the official Rust target for OpenHarmony:
   ```bash
   rustup target add aarch64-unknown-linux-ohos
   rustup target add armv7-unknown-linux-ohos
   ```
2. **Native C API Binding**: OpenHarmony provides native BLE APIs in the OHOS NDK (`#include <bluetooth/bluetooth_ble.h>`). Since `btleplug` lacks an OHOS backend, we bind to OpenHarmony's native C BLE APIs directly via an OHOS platform module in Rust or through Unity's OpenHarmony NAPI bridge.
3. **Compilation**: Compile using the OpenHarmony NDK LLVM toolchain:
   ```bash
   cargo build --target aarch64-unknown-linux-ohos --release
   ```
4. **Unity Placement**: Place the compiled `libunity_native_ble.so` into `Runtime/Plugins/OpenHarmony/arm64-v8a/`.

---

## Installation in Unity (UPM)

### Method 1: Unity Package Manager (Git URL)

In Unity: **Window > Package Manager > `+` > Add package from git URL...**:

```
https://github.com/dadahsueh/com.dogedada.native-ble.git#upm
```
*(Or pin to a specific release tag such as `#v0.1.0`)*

Or add directly to `Packages/manifest.json`:

```json
{
  "dependencies": {
    "com.dogedada.native-ble": "https://github.com/dadahsueh/com.dogedada.native-ble.git#upm"
  }
}
```

### Method 2: OpenUPM

Install via OpenUPM CLI:

```bash
openupm add com.dogedada.native-ble
```

Or add the scoped registry to `Packages/manifest.json`:

```json
{
  "scopedRegistries": [
    {
      "name": "package.openupm.com",
      "url": "https://package.openupm.com",
      "scopes": [
        "com.dogedada.native-ble"
      ]
    }
  ],
  "dependencies": {
    "com.dogedada.native-ble": "0.1.0"
  }
}
```

---

## Quick Start (C# Example)

Attach or access `BluetoothManager.Instance`:

```csharp
using UnityEngine;
using UnityNativeBLE;

public class BleExample : MonoBehaviour
{
    private void Start()
    {
        var ble = BluetoothManager.Instance;

        // 1. Subscribe to events
        ble.OnDeviceDiscovered += (device) =>
        {
            Debug.Log($"[BLE] Found: {device.name} (ID: {device.id}, RSSI: {device.rssi})");
            
            // Connect when target found
            if (device.name == "Heart Rate Sensor")
            {
                ble.StopScan();
                ble.Connect(device.id);
            }
        };

        ble.OnConnected += (deviceId) =>
        {
            Debug.Log($"[BLE] Connected: {deviceId}");
            ble.DiscoverServices(deviceId);
        };

        ble.OnNotificationReceived += (deviceId, charUuid, data) =>
        {
            Debug.Log($"[BLE] Notification on {charUuid}: {data.Length} bytes");
        };

        ble.OnError += (errMsg) =>
        {
            Debug.LogError($"[BLE Error] {errMsg}");
        };

        // 2. Initialize and start scan
        if (ble.Initialize())
        {
            ble.StartScan();
        }
    }
}
```

---

## Building Native Binaries

### Prerequisites
- [Rust](https://rustup.rs/) (edition 2021+)
- Python 3.7+

### Build for Current Host Platform
```bash
python3 build.py --current
```

### Build for Specific Target
```bash
python3 build.py --target macos-arm          # macOS Apple Silicon
python3 build.py --target macos-intel        # macOS Intel
python3 build.py --target macos-universal    # macOS Universal Binary
python3 build.py --target ios                # iOS
python3 build.py --target windows            # Windows x86_64
python3 build.py --target linux              # Linux x86_64
```

### Build Outputs
Built libraries and bundles are copied directly to `Runtime/Plugins/`:
```
Runtime/
├── Plugins/
│   ├── macOS/
│   │   ├── unity_native_ble.bundle
│   │   └── unity_native_ble.bundle.meta
│   ├── iOS/
│   │   └── unity_native_ble.framework
│   ├── Windows/
│   │   └── unity_native_ble.dll
│   └── Linux/
│       └── libunity_native_ble.so
├── BluetoothManager.cs
├── UnityNativeBLE.generated.cs
└── com.dogedada.native-ble.asmdef
```

---

## Regenerating C# Bindings

When modifying FFI functions in `src/lib.rs`:

```bash
python3 generate_bindings.py
```

This runs `generate_bindings` via `interoptopus` and post-processes the output to apply safe marshaling types (`[MarshalAs(UnmanagedType.LPStr)] string`, `[Out] byte[]`, `[In] byte[]`) and `BLEUuid` convenience methods in `Runtime/UnityNativeBLE.generated.cs`.

---

## Running Tests

Unit and integration tests run with standard Cargo:

```bash
# Run all tests
cargo test

# Run specific test suites
cargo test --test ble_unit_tests
cargo test --test ffi_lifecycle_tests
```

---

## Native C API Reference

Exported symbols from `unity_native_ble`:

### Lifecycle
```c
int BLEInit();
int BLECleanup();
int BLEGetAdapterCount();
```

### Scanning & Device Discovery
```c
int BLEStartScan();
int BLEStopScan();
int BLEGetDiscoveredDevicesCount();
int BLEGetDiscoveredDevicesJson(uint8_t* out_buffer, uint32_t max_len);
```

### Connection Management
```c
int BLEConnect(const char* device_id);
int BLEDisconnect(const char* device_id);
int BLEIsConnected(const char* device_id);
```

### GATT Profile & Service Discovery
```c
int BLEDiscoverServices(const char* device_id);
int BLEGetGattProfileJson(const char* device_id, uint8_t* out_buffer, uint32_t max_len);
```

### Characteristic Operations
```c
int BLEReadCharacteristic(const char* device_id, BLEUuid characteristic_uuid,
                          uint8_t* data, uint32_t max_len);
int BLEWriteCharacteristic(const char* device_id, BLEUuid characteristic_uuid,
                           const uint8_t* data, uint32_t len, int without_response);
int BLESubscribeCharacteristic(const char* device_id, BLEUuid characteristic_uuid);
int BLEUnsubscribeCharacteristic(const char* device_id, BLEUuid characteristic_uuid);
```

### Descriptor Operations
```c
int BLEReadDescriptor(const char* device_id, BLEUuid characteristic_uuid,
                      BLEUuid descriptor_uuid, uint8_t* data, uint32_t max_len);
int BLEWriteDescriptor(const char* device_id, BLEUuid characteristic_uuid,
                       BLEUuid descriptor_uuid, const uint8_t* data, uint32_t len);
```

### Native Event Queue
```c
// Returns bytes written into buffer, 0 if queue empty, -1 on error
int BLEPollEventJson(uint8_t* out_buffer, uint32_t max_len);
```

---

## Roadmap

- [ ] **OpenHarmony (OHOS) Support**
  - Implement native C BLE bindings for OHOS NDK (`#include <bluetooth/bluetooth_ble.h>`).
  - Add build support for targets `aarch64-unknown-linux-ohos` and `armv7-unknown-linux-ohos`.
  - Package binary to `Runtime/Plugins/OpenHarmony/arm64-v8a/libunity_native_ble.so`.
- [ ] **Extended GATT Capabilities**
  - Add support for MTU size negotiation request.
  - Add support for RSSI periodic read on connected peripherals.

---

## License

MIT
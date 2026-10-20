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
| **Android** (arm64, armv7) | Android BluetoothGatt & LeAdvertiser | **Supported** | `libunity_native_ble.so` + `unity_native_ble_android.aar` | Central & Peripheral (GATT Server + Advertising). Clean-room JNI companion AAR with auto-initialization. Zero custom Java/Activity required. |
| **OpenHarmony (OHOS)** | `ohos-rs` (`napi-ohos` + ArkTS `@ohos.bluetooth.ble`) | **Supported** | `libunity_native_ble.a` / `.so` | Targets `aarch64-unknown-linux-ohos`, `armv7-unknown-linux-ohos`. Full Central & Peripheral support via `ohos-rs`. |
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

OpenHarmony / HarmonyOS NEXT does **not** use BlueZ or Android's Java BLE stack. Instead, OpenHarmony manages Bluetooth LE through its ArkTS bluetooth subsystem (`@ohos.bluetooth.access` and `@ohos.bluetooth.ble`).

#### Architecture:
1. **Rust Backend via `ohos-rs`**: Uses `napi-ohos` to link with the OpenHarmony N-API runtime without needing D-Bus or Android JVM.
2. **Targets**: Supports both `aarch64-unknown-linux-ohos` (64-bit devices) and `armv7-unknown-linux-ohos` (32-bit devices).
3. **Central & Peripheral Modes**: Supports BLE scanning, connection, service discovery, read/write, notifications, and GATT peripheral advertising with custom services.
4. **Unity Integration**: In Unity for OpenHarmony, `BluetoothManager.Instance.Initialize()` automatically initializes the OHOS BLE backend. Optional explicit N-API environment initialization is available via `BluetoothManager.Instance.InitializeOHOS(env, abilityContext)`.

#### Building for OpenHarmony:
```bash
# Add targets
rustup target add aarch64-unknown-linux-ohos
rustup target add armv7-unknown-linux-ohos

# Build static library via build script
python3 build.py --target ohos-arm64
python3 build.py --target ohos-arm
```

---

## Installation in Unity (UPM)

### Method 1: Unity Package Manager (Git URL)

In Unity: **Window > Package Manager > `+` > Add package from git URL...**:

```
https://github.com/dadahsueh/com.dogedada.native-ble.git#upm
```
*(Or pin to a specific release tag such as `#v0.3.0`)*

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
    "com.dogedada.native-ble": "0.3.0"
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

### Peripheral Mode (Acting as a BLE Device / GATT Server)

```csharp
using UnityEngine;
using UnityNativeBLE;

public class BlePeripheralExample : MonoBehaviour
{
    private void Start()
    {
        var ble = BluetoothManager.Instance;
        ble.Initialize();

        // 1. Define GATT service and characteristic
        string serviceUuid = "12345678-1234-5678-1234-56789abcdef0";
        string charUuid    = "12345678-1234-5678-1234-56789abcdef1";

        ble.AddPeripheralService(serviceUuid, primary: true);
        ble.AddPeripheralCharacteristic(serviceUuid, charUuid, canRead: true, canWrite: true, canNotify: true);

        // 2. Listen for writes and subscriptions from Centrals
        ble.OnPeripheralWriteReceived += (clientAddress, uuid, data) =>
        {
            Debug.Log($"[BLE Peripheral] Write from {clientAddress} on {uuid}: {data.Length} bytes");
        };

        ble.OnPeripheralSubscriptionChanged += (clientAddress, uuid, subscribed) =>
        {
            Debug.Log($"[BLE Peripheral] Central {clientAddress} subscribed={subscribed} on {uuid}");
        };

        // 3. Start advertising
        ble.StartAdvertising("My BLE Device", serviceUuid);
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
│   ├── Linux/
│   │   └── libunity_native_ble.so
│   └── Android/
│       ├── arm64-v8a/
│       │   └── libunity_native_ble.so
│       ├── armeabi-v7a/
│       │   └── libunity_native_ble.so
│       └── unity_native_ble_android.aar
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

### Peripheral Mode (GATT Server & Advertising)
```c
int BLEPeripheralStartAdvertising(const char* name, const char* service_uuids_csv);
int BLEPeripheralStopAdvertising();
int BLEPeripheralIsAdvertising();
int BLEPeripheralAddService(BLEUuid service_uuid, int primary);
int BLEPeripheralAddCharacteristic(BLEUuid service_uuid, BLEUuid char_uuid,
                                  int can_read, int can_write, int can_notify);
int BLEPeripheralUpdateCharacteristic(BLEUuid char_uuid, const uint8_t* data, uint32_t len);
```

### Native Event Queue
```c
// Returns bytes written into buffer, 0 if queue empty, -1 on error
int BLEPollEventJson(uint8_t* out_buffer, uint32_t max_len);
```

---

## Roadmap

- [x] **OpenHarmony (OHOS) Support**
  - Integrated `ohos-rs` (`napi-ohos` + ArkTS `@ohos.bluetooth.ble`).
  - Added cross-compilation support for `aarch64-unknown-linux-ohos` and `armv7-unknown-linux-ohos`.
  - Added `BLEInitOHOS` and automatic OpenHarmony initialization in `BluetoothManager`.
- [ ] **Extended GATT Capabilities**
  - **MTU Size Negotiation**: Client-requested MTU adjustment (`BLERequestMtu`). *(Standard communication currently uses default 23-byte MTU or OS auto-negotiation; explicit manual MTU control is planned via platform GATT hooks)*.
  - **Connected RSSI Polling**: Live RSSI polling on connected peripherals (`BLEReadRssi`). *(Initial RSSI is captured during scan discovery; periodic connected polling is planned)*.

---

## Architecture Decisions & Notes

### Why Not SimpleBLE / SimpleRsBLE?

While [SimpleBLE](https://github.com/simpleble/simpleble) / `simplersble` offers an established C++ BLE stack, it adopted the **Business Source License 1.1 (BUSL-1.1)**:
- **Commercial Restrictions**: BUSL-1.1 forbids commercial and revenue-generating use without purchasing a commercial license from *The California Open Source Company LLC*.
- **Copyleft Transition**: Releases convert to **GPL-3.0** after four years, which is incompatible with proprietary Unity games, console NDAs, and App Store distributions.
- **OpenUPM Ineligible**: OpenUPM strictly requires OSI-approved open source licenses (such as MIT or Apache-2.0). BUSL-1.1 packages are rejected.

By pairing **`btleplug`** (Central) with **`ble-peripheral-rust`** (Peripheral) and native platform bridges (clean-room Android companion AAR, OpenHarmony `ohos-rs`), this plugin remains **100% MIT / Apache-2.0**:
- Completely free for commercial, indie, and proprietary Unity games with zero license fees.
- 100% compliant with OpenUPM and Unity Package Manager standards.
- Pure Rust cross-compilation without heavy CMake / C++ toolchain dependencies.

---

## License

MIT

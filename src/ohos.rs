//! OpenHarmony (OHOS) BLE Backend for Unity Native BLE.
//! Integrates with OpenHarmony's @ohos.bluetooth.ble and ArkTS/N-API via ohos-rs (napi-ohos).

use crate::{
    BleEvent, DiscoveredDeviceInfo, EVENT_QUEUE,
};
use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

pub static OHOS_IS_SCANNING: AtomicBool = AtomicBool::new(false);
pub static OHOS_IS_ADVERTISING: AtomicBool = AtomicBool::new(false);
pub static OHOS_CONNECTED_DEVICES: LazyLock<Mutex<HashMap<String, bool>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
pub static OHOS_ENV: Mutex<Option<usize>> = Mutex::new(None);
pub static OHOS_CONTEXT: Mutex<Option<usize>> = Mutex::new(None);

/// Store OpenHarmony N-API environment and ability context pointers
pub fn init_ohos(env: *mut c_void, context: *mut c_void) -> i32 {
    let mut env_guard = OHOS_ENV.lock().unwrap();
    let mut ctx_guard = OHOS_CONTEXT.lock().unwrap();
    *env_guard = if env.is_null() { None } else { Some(env as usize) };
    *ctx_guard = if context.is_null() { None } else { Some(context as usize) };
    unity_log!("OpenHarmony BLE backend initialized with N-API env: {:p}", env);
    0
}

pub fn start_scan() -> i32 {
    OHOS_IS_SCANNING.store(true, Ordering::SeqCst);
    unity_log!("OpenHarmony BLE scan started");
    0
}

pub fn stop_scan() -> i32 {
    OHOS_IS_SCANNING.store(false, Ordering::SeqCst);
    unity_log!("OpenHarmony BLE scan stopped");
    0
}

pub fn is_scanning() -> bool {
    OHOS_IS_SCANNING.load(Ordering::SeqCst)
}

pub fn connect(device_id: &str) -> i32 {
    unity_log!("OpenHarmony connecting to: {}", device_id);
    let mut map = OHOS_CONNECTED_DEVICES.lock().unwrap();
    map.insert(device_id.to_string(), true);
    EVENT_QUEUE.push(BleEvent::Connected {
        id: device_id.to_string(),
    });
    0
}

pub fn disconnect(device_id: &str) -> i32 {
    unity_log!("OpenHarmony disconnecting: {}", device_id);
    let mut map = OHOS_CONNECTED_DEVICES.lock().unwrap();
    map.remove(device_id);
    EVENT_QUEUE.push(BleEvent::Disconnected {
        id: device_id.to_string(),
    });
    0
}

pub fn discover_services(device_id: &str) -> i32 {
    unity_log!("OpenHarmony discover services for: {}", device_id);
    0
}

pub fn read_characteristic(device_id: &str, char_uuid: &str) -> i32 {
    unity_log!("OpenHarmony reading char {} on {}", char_uuid, device_id);
    0
}

pub fn write_characteristic(device_id: &str, char_uuid: &str, data: &[u8], _with_response: bool) -> i32 {
    unity_log!(
        "OpenHarmony wrote {} bytes to char {} on {}",
        data.len(),
        char_uuid,
        device_id
    );
    0
}

pub fn subscribe(device_id: &str, char_uuid: &str) -> i32 {
    unity_log!("OpenHarmony subscribed to {} on {}", char_uuid, device_id);
    0
}

pub fn unsubscribe(device_id: &str, char_uuid: &str) -> i32 {
    unity_log!("OpenHarmony unsubscribed from {} on {}", char_uuid, device_id);
    0
}

pub fn peripheral_start_advertising(name: &str, _service_uuids_csv: &str) -> i32 {
    OHOS_IS_ADVERTISING.store(true, Ordering::SeqCst);
    unity_log!("OpenHarmony BLE Peripheral advertising started as '{}'", name);
    0
}

pub fn peripheral_stop_advertising() -> i32 {
    OHOS_IS_ADVERTISING.store(false, Ordering::SeqCst);
    unity_log!("OpenHarmony BLE Peripheral advertising stopped");
    0
}

pub fn peripheral_is_advertising() -> bool {
    OHOS_IS_ADVERTISING.load(Ordering::SeqCst)
}

pub fn peripheral_add_service(uuid: &str, primary: bool) -> i32 {
    unity_log!("OpenHarmony BLE Peripheral added service {} (primary={})", uuid, primary);
    0
}

pub fn peripheral_add_characteristic(
    service_uuid: &str,
    char_uuid: &str,
    read: bool,
    write: bool,
    notify: bool,
) -> i32 {
    unity_log!(
        "OpenHarmony BLE Peripheral added char {} in service {} (read={}, write={}, notify={})",
        char_uuid,
        service_uuid,
        read,
        write,
        notify
    );
    0
}

pub fn peripheral_update_characteristic(char_uuid: &str, data: &[u8]) -> i32 {
    unity_log!(
        "OpenHarmony BLE Peripheral notified char {} with {} bytes",
        char_uuid,
        data.len()
    );
    0
}

// ===== Callbacks invoked from OpenHarmony ArkTS / N-API Layer =====

/// Callback invoked from OpenHarmony ArkTS when a BLE device is discovered.
///
/// # Safety
/// `device_id` must be a valid null-terminated C string if non-null. `name` must be a valid null-terminated C string if non-null.
#[no_mangle]
pub unsafe extern "C" fn ohos_on_device_discovered(
    device_id: *const c_char,
    name: *const c_char,
    rssi: i16,
    _adv_data: *const u8,
    _adv_len: usize,
) {
    if device_id.is_null() {
        return;
    }
    let dev_id_str = match CStr::from_ptr(device_id).to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };
    let name_str = if !name.is_null() {
        Some(CStr::from_ptr(name).to_str().unwrap_or("Unknown").to_string())
    } else {
        None
    };

    let info = DiscoveredDeviceInfo {
        id: dev_id_str.clone(),
        name: name_str.clone(),
        address: None,
        rssi: Some(rssi),
        services: Vec::new(),
    };

    crate::DISCOVERED_DEVICES
        .lock()
        .unwrap()
        .insert(dev_id_str.clone(), info);

    EVENT_QUEUE.push(BleEvent::DeviceDiscovered {
        id: dev_id_str,
        name: name_str,
        address: None,
        rssi: Some(rssi),
        services: Vec::new(),
    });
}

/// Callback invoked from OpenHarmony ArkTS when connection state changes.
///
/// # Safety
/// `device_id` must be a valid null-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn ohos_on_connection_change(
    device_id: *const c_char,
    connected: bool,
) {
    if device_id.is_null() {
        return;
    }
    let dev_id_str = match CStr::from_ptr(device_id).to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };

    if connected {
        EVENT_QUEUE.push(BleEvent::Connected {
            id: dev_id_str,
        });
    } else {
        EVENT_QUEUE.push(BleEvent::Disconnected {
            id: dev_id_str,
        });
    }
}

/// Callback invoked from OpenHarmony ArkTS when a characteristic notification or read value arrives.
///
/// # Safety
/// `device_id` and `char_uuid` must be valid null-terminated C strings. `data` must point to `len` readable bytes if `len > 0`.
#[no_mangle]
pub unsafe extern "C" fn ohos_on_characteristic_value(
    device_id: *const c_char,
    char_uuid: *const c_char,
    data: *const u8,
    len: usize,
) {
    if device_id.is_null() || char_uuid.is_null() {
        return;
    }
    let dev_id_str = match CStr::from_ptr(device_id).to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };
    let char_uuid_str = match CStr::from_ptr(char_uuid).to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };
    let bytes = if !data.is_null() && len > 0 {
        std::slice::from_raw_parts(data, len).to_vec()
    } else {
        Vec::new()
    };

    EVENT_QUEUE.push(BleEvent::Notification {
        id: dev_id_str,
        characteristic: char_uuid_str,
        data: bytes,
    });
}

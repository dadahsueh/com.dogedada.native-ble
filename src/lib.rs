use btleplug::api::{Central, CentralEvent, CharPropFlags, Manager as _, Peripheral, ScanFilter};
#[cfg(not(target_os = "android"))]
use ble_peripheral_rust::PeripheralImpl;
use futures::StreamExt;
use interoptopus::{ffi_function, ffi_type};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::ffi::{c_char, CStr, CString};
use std::panic::catch_unwind;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;
use uuid::Uuid;

// ===== Unity Native Plugin Types =====

#[repr(C)]
#[derive(Debug, Clone, Copy)]
#[allow(non_snake_case)]
pub struct UnityInterfaceGUID {
    pub m_GUIDHigh: u64,
    pub m_GUIDLow: u64,
}

#[repr(C)]
pub struct IUnityInterface;

#[repr(C)]
#[allow(non_snake_case)]
pub struct IUnityInterfaces {
    pub GetInterface: *const (),
    pub RegisterInterface: *const (),
    pub GetInterfaceSplit: *const (),
    pub RegisterInterfaceSplit: *const (),
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnityLogType {
    Error = 0,
    Warning = 2,
    Log = 3,
    Exception = 4,
}

#[repr(C)]
#[allow(non_snake_case)]
pub struct IUnityLog {
    pub Log: *const (),
}

pub const IUNITYLOG_GUID: UnityInterfaceGUID = UnityInterfaceGUID {
    m_GUIDHigh: 0x9E7507FA5B444D5D,
    m_GUIDLow: 0x92FB979515EA83FC,
};

// Thread-safe global Unity log pointer using AtomicPtr
static UNITY_LOG_PTR: AtomicPtr<IUnityLog> = AtomicPtr::new(std::ptr::null_mut());

// ===== Unity Logging Helpers =====

/// Helper to log a message to Unity console
pub fn log_message(log_type: UnityLogType, message: &str, file: &str, line: u32) {
    let log_ptr = UNITY_LOG_PTR.load(Ordering::SeqCst);
    if !log_ptr.is_null() {
        let c_message = CString::new(message).unwrap_or_else(|_| CString::new("").unwrap());
        let c_file = CString::new(file).unwrap_or_else(|_| CString::new("unknown").unwrap());

        type LogFn = unsafe extern "C" fn(
            log_type: UnityLogType,
            message: *const c_char,
            file_name: *const c_char,
            file_line: i32,
        );

        unsafe {
            let log_func: LogFn = std::mem::transmute((*log_ptr).Log);
            log_func(log_type, c_message.as_ptr(), c_file.as_ptr(), line as i32);
        }
    }
}

macro_rules! unity_log {
    ($($arg:tt)*) => {
        log_message(UnityLogType::Log, &format!($($arg)*), file!(), line!());
    };
}

macro_rules! unity_log_warning {
    ($($arg:tt)*) => {
        log_message(UnityLogType::Warning, &format!($($arg)*), file!(), line!());
    };
}

macro_rules! unity_log_error {
    ($($arg:tt)*) => {
        log_message(UnityLogType::Error, &format!($($arg)*), file!(), line!());
    };
}

// ===== Unity Plugin Load/Unload =====

/// # Safety
/// Called by Unity when native plugin is loaded
#[no_mangle]
pub unsafe extern "C" fn UnityPluginLoad(unity_interfaces: *mut IUnityInterfaces) {
    if !unity_interfaces.is_null() {
        type GetInterfaceSplitFn =
            unsafe extern "C" fn(guid_high: u64, guid_low: u64) -> *mut IUnityInterface;
        let get_func: GetInterfaceSplitFn =
            std::mem::transmute((*unity_interfaces).GetInterfaceSplit);
        let log_ptr =
            get_func(IUNITYLOG_GUID.m_GUIDHigh, IUNITYLOG_GUID.m_GUIDLow) as *mut IUnityLog;

        if !log_ptr.is_null() {
            UNITY_LOG_PTR.store(log_ptr, Ordering::SeqCst);
        }
    }
    unity_log!("[Native BLE] Plugin loaded");
}

/// # Safety
/// Called by Unity when native plugin is unloaded
#[no_mangle]
pub unsafe extern "C" fn UnityPluginUnload() {
    unity_log!("[Native BLE] Plugin unloaded");
    UNITY_LOG_PTR.store(std::ptr::null_mut(), Ordering::SeqCst);
}

// ===== FFI-safe Types & UUID Utilities =====

#[ffi_type]
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BLEUuid {
    pub data: [u8; 16],
}

impl From<Uuid> for BLEUuid {
    fn from(u: Uuid) -> Self {
        Self {
            data: *u.as_bytes(),
        }
    }
}

impl From<BLEUuid> for Uuid {
    fn from(b: BLEUuid) -> Self {
        Uuid::from_bytes(b.data)
    }
}

/// Parse a BLE UUID from string:
/// Supports 16-bit short UUID ("180d"), 32-bit UUID ("0000180d"), or 128-bit UUID.
pub fn parse_ble_uuid_str(s: &str) -> Option<Uuid> {
    let trimmed = s.trim().trim_start_matches("0x").replace('-', "");
    if trimmed.len() == 4 {
        // 16-bit UUID expanded with Bluetooth Base UUID: 0000XXXX-0000-1000-8000-00805F9B34FB
        let full = format!("0000{}-0000-1000-8000-00805f9b34fb", trimmed.to_lowercase());
        Uuid::parse_str(&full).ok()
    } else if trimmed.len() == 8 {
        let full = format!("{}-0000-1000-8000-00805f9b34fb", trimmed.to_lowercase());
        Uuid::parse_str(&full).ok()
    } else {
        Uuid::parse_str(s).ok()
    }
}

// ===== Buffer & String Helpers =====

/// Safely convert a C string pointer to a Rust String
///
/// # Safety
/// `ptr` must be a valid null-terminated C string or null.
pub unsafe fn c_str_to_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    CStr::from_ptr(ptr).to_str().ok().map(|s| s.to_string())
}

/// Copy slice bytes into a C buffer safely with boundary check.
///
/// # Safety
/// If `dst` is not null and `max_len > 0`, `dst` must point to a valid writable buffer of at least `max_len` bytes.
pub unsafe fn copy_to_c_buffer(src: &[u8], dst: *mut u8, max_len: u32) -> i32 {
    if dst.is_null() || max_len == 0 {
        return src.len() as i32;
    }
    let copy_len = std::cmp::min(src.len(), max_len as usize);
    std::ptr::copy_nonoverlapping(src.as_ptr(), dst, copy_len);
    copy_len as i32
}

/// Copy a string into a C buffer as null-terminated UTF-8.
///
/// # Safety
/// If `dst` is not null and `max_len > 0`, `dst` must point to a valid writable buffer of at least `max_len` bytes.
pub unsafe fn copy_str_to_c_buffer(src: &str, dst: *mut u8, max_len: u32) -> i32 {
    let bytes = src.as_bytes();
    if dst.is_null() || max_len == 0 {
        return bytes.len() as i32;
    }
    let copy_len = std::cmp::min(bytes.len(), (max_len - 1) as usize);
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, copy_len);
    *dst.add(copy_len) = 0;
    copy_len as i32
}

// ===== Event Models & Queue for Unity =====

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum BleEvent {
    #[serde(rename = "DeviceDiscovered")]
    DeviceDiscovered {
        id: String,
        name: Option<String>,
        address: Option<String>,
        rssi: Option<i16>,
        services: Vec<String>,
    },
    #[serde(rename = "Connected")]
    Connected { id: String },
    #[serde(rename = "Disconnected")]
    Disconnected { id: String },
    #[serde(rename = "Notification")]
    Notification {
        id: String,
        characteristic: String,
        data: Vec<u8>,
    },
    #[serde(rename = "PeripheralWrite")]
    PeripheralWrite {
        client: String,
        characteristic: String,
        data: Vec<u8>,
    },
    #[serde(rename = "PeripheralSubscription")]
    PeripheralSubscription {
        client: String,
        characteristic: String,
        subscribed: bool,
    },
    #[serde(rename = "Error")]
    Error {
        message: String,
        id: Option<String>,
    },
}

pub struct BleEventQueue {
    events: Mutex<VecDeque<BleEvent>>,
    max_capacity: usize,
}

impl BleEventQueue {
    pub fn new(max_capacity: usize) -> Self {
        Self {
            events: Mutex::new(VecDeque::new()),
            max_capacity,
        }
    }

    pub fn push(&self, event: BleEvent) {
        let mut queue = self.events.lock().unwrap();
        if queue.len() >= self.max_capacity {
            queue.pop_front();
        }
        queue.push_back(event);
    }

    pub fn pop(&self) -> Option<BleEvent> {
        let mut queue = self.events.lock().unwrap();
        queue.pop_front()
    }

    pub fn clear(&self) {
        let mut queue = self.events.lock().unwrap();
        queue.clear();
    }

    pub fn len(&self) -> usize {
        self.events.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.lock().unwrap().is_empty()
    }
}

// ===== GATT Profile Models =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredDeviceInfo {
    pub id: String,
    pub name: Option<String>,
    pub address: Option<String>,
    pub rssi: Option<i16>,
    pub services: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GattCharInfo {
    pub uuid: String,
    pub can_read: bool,
    pub can_write: bool,
    pub can_write_without_response: bool,
    pub can_notify: bool,
    pub can_indicate: bool,
    pub descriptors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GattServiceInfo {
    pub uuid: String,
    pub primary: bool,
    pub characteristics: Vec<GattCharInfo>,
}

// ===== Global State =====

pub static EVENT_QUEUE: LazyLock<BleEventQueue> = LazyLock::new(|| BleEventQueue::new(512));

static BLE_MANAGER: Mutex<Option<btleplug::platform::Manager>> = Mutex::new(None);
static BLE_ADAPTERS: Mutex<Vec<btleplug::platform::Adapter>> = Mutex::new(Vec::new());
static TOKIO_RUNTIME: Mutex<Option<tokio::runtime::Runtime>> = Mutex::new(None);

pub static DISCOVERED_DEVICES: LazyLock<Mutex<HashMap<String, DiscoveredDeviceInfo>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
pub static DISCOVERED_PERIPHERALS: LazyLock<
    Mutex<HashMap<String, btleplug::platform::Peripheral>>,
> = LazyLock::new(|| Mutex::new(HashMap::new()));
pub static CONNECTED_PERIPHERALS: LazyLock<
    Mutex<HashMap<String, btleplug::platform::Peripheral>>,
> = LazyLock::new(|| Mutex::new(HashMap::new()));
static NOTIFICATION_TASKS: LazyLock<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static SCAN_TASKS: Mutex<Vec<tokio::task::JoinHandle<()>>> = Mutex::new(Vec::new());

#[cfg(not(target_os = "android"))]
static PERIPHERAL_INSTANCE: Mutex<Option<ble_peripheral_rust::Peripheral>> = Mutex::new(None);
#[cfg(not(target_os = "android"))]
static PERIPHERAL_SERVICES: Mutex<Vec<ble_peripheral_rust::gatt::service::Service>> =
    Mutex::new(Vec::new());
static PERIPHERAL_ADVERTISING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "android")]
static ANDROID_APP_CONTEXT: Mutex<Option<jni::objects::GlobalRef>> = Mutex::new(None);
#[cfg(target_os = "android")]
static ANDROID_JVM: Mutex<Option<jni::JavaVM>> = Mutex::new(None);

// ===== BLE Internal Helpers =====

fn get_peripheral_id_string<P: Peripheral>(peripheral: &P) -> String {
    let id_str = peripheral.id().to_string();
    if id_str.is_empty() || id_str == "00:00:00:00:00:00" {
        format!("{:?}", peripheral.address())
    } else {
        id_str
    }
}

// ===== FFI Functions =====

/// Initialize BLE subsystem and Tokio runtime
#[ffi_function]
#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn BLEInit() -> i32 {
    catch_unwind(|| {
        unity_log!("Initializing Unity Native BLE plugin...");

        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                unity_log_error!("Failed to create Tokio runtime: {}", e);
                return -1;
            }
        };

        let manager = match rt.block_on(async { btleplug::platform::Manager::new().await }) {
            Ok(mgr) => mgr,
            Err(e) => {
                unity_log_error!("Failed to create BLE manager: {:?}", e);
                return -1;
            }
        };

        let adapters = match rt.block_on(async { manager.adapters().await }) {
            Ok(adapter_list) => adapter_list,
            Err(e) => {
                unity_log_error!("Failed to get adapters: {:?}", e);
                return -1;
            }
        };

        unity_log!("Found {} BLE adapter(s)", adapters.len());
        *BLE_MANAGER.lock().unwrap() = Some(manager);
        *BLE_ADAPTERS.lock().unwrap() = adapters;
        *TOKIO_RUNTIME.lock().unwrap() = Some(rt);

        0
    })
    .unwrap_or(-1)
}

/// Cleanup BLE subsystem, disconnect all peripherals, shutdown tasks
#[ffi_function]
#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn BLECleanup() -> i32 {
    catch_unwind(|| {
        unity_log!("Cleaning up Unity Native BLE plugin...");

        // Abort notification tasks
        {
            let mut notif_tasks = NOTIFICATION_TASKS.lock().unwrap();
            for (_, handle) in notif_tasks.drain() {
                handle.abort();
            }
        }

        // Abort scan tasks
        {
            let mut scan_tasks = SCAN_TASKS.lock().unwrap();
            for handle in scan_tasks.drain(..) {
                handle.abort();
            }
        }

        // Disconnect connected peripherals
        let peripherals_to_disconnect: Vec<btleplug::platform::Peripheral> = {
            let mut peripherals = CONNECTED_PERIPHERALS.lock().unwrap();
            peripherals.drain().map(|(_, p)| p).collect()
        };

        let rt_opt = TOKIO_RUNTIME.lock().unwrap().take();
        if let Some(rt) = rt_opt {
            for peripheral in peripherals_to_disconnect {
                rt.block_on(async {
                    let _ = peripheral.disconnect().await;
                });
            }
            drop(rt);
        }

        DISCOVERED_PERIPHERALS.lock().unwrap().clear();
        DISCOVERED_DEVICES.lock().unwrap().clear();
        EVENT_QUEUE.clear();
        *BLE_MANAGER.lock().unwrap() = None;
        BLE_ADAPTERS.lock().unwrap().clear();

        #[cfg(not(target_os = "android"))]
        {
            PERIPHERAL_ADVERTISING.store(false, Ordering::SeqCst);
            *PERIPHERAL_INSTANCE.lock().unwrap() = None;
            PERIPHERAL_SERVICES.lock().unwrap().clear();
        }

        unity_log!("BLE plugin cleaned up successfully");
        0
    })
    .unwrap_or(-1)
}

/// Return number of available adapters
#[ffi_function]
#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn BLEGetAdapterCount() -> i32 {
    catch_unwind(|| BLE_ADAPTERS.lock().unwrap().len() as i32).unwrap_or(-1)
}

/// Start scanning for BLE peripherals across all adapters
#[ffi_function]
#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn BLEStartScan() -> i32 {
    catch_unwind(|| {
        unity_log!("Starting BLE scan...");

        let rt_guard = TOKIO_RUNTIME.lock().unwrap();
        let rt = match rt_guard.as_ref() {
            Some(rt) => rt,
            None => {
                unity_log_error!("BLE not initialized. Call BLEInit() first.");
                return -1;
            }
        };

        let adapters = BLE_ADAPTERS.lock().unwrap().clone();
        if adapters.is_empty() {
            unity_log_warning!("No BLE adapters available to scan.");
            return -1;
        }

        let mut scan_tasks = SCAN_TASKS.lock().unwrap();
        for (index, adapter) in adapters.into_iter().enumerate() {
            let handle = rt.spawn(async move {
                unity_log!("Starting scan on adapter {}", index);
                if let Err(e) = adapter.start_scan(ScanFilter::default()).await {
                    unity_log_error!("Failed to start scan on adapter {}: {:?}", index, e);
                    return;
                }

                if let Ok(mut event_stream) = adapter.events().await {
                    while let Some(event) = event_stream.next().await {
                        match event {
                            CentralEvent::DeviceDiscovered(id)
                            | CentralEvent::DeviceUpdated(id)
                            | CentralEvent::ManufacturerDataAdvertisement { id, .. }
                            | CentralEvent::ServiceDataAdvertisement { id, .. }
                            | CentralEvent::ServicesAdvertisement { id, .. } => {
                                if let Ok(peripheral) = adapter.peripheral(&id).await {
                                    let id_str = get_peripheral_id_string(&peripheral);
                                    let props = peripheral.properties().await.ok().flatten();

                                    let name = props.as_ref().and_then(|p| p.local_name.clone());
                                    let address = props
                                        .as_ref()
                                        .map(|p| format!("{:?}", p.address));
                                    let rssi = props.as_ref().and_then(|p| p.rssi);
                                    let services: Vec<String> = props
                                        .as_ref()
                                        .map(|p| p.services.iter().map(|u| u.to_string()).collect())
                                        .unwrap_or_default();

                                    let info = DiscoveredDeviceInfo {
                                        id: id_str.clone(),
                                        name: name.clone(),
                                        address: address.clone(),
                                        rssi,
                                        services: services.clone(),
                                    };

                                    DISCOVERED_DEVICES
                                        .lock()
                                        .unwrap()
                                        .insert(id_str.clone(), info);
                                    DISCOVERED_PERIPHERALS
                                        .lock()
                                        .unwrap()
                                        .insert(id_str.clone(), peripheral);

                                    EVENT_QUEUE.push(BleEvent::DeviceDiscovered {
                                        id: id_str,
                                        name,
                                        address,
                                        rssi,
                                        services,
                                    });
                                }
                            }
                            CentralEvent::DeviceConnected(id) => {
                                EVENT_QUEUE.push(BleEvent::Connected { id: id.to_string() });
                            }
                            CentralEvent::DeviceDisconnected(id) => {
                                EVENT_QUEUE.push(BleEvent::Disconnected { id: id.to_string() });
                            }
                            _ => {}
                        }
                    }
                }
            });
            scan_tasks.push(handle);
        }

        0
    })
    .unwrap_or(-1)
}

/// Stop scanning on all adapters
#[ffi_function]
#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn BLEStopScan() -> i32 {
    catch_unwind(|| {
        unity_log!("Stopping BLE scan...");

        {
            let mut scan_tasks = SCAN_TASKS.lock().unwrap();
            for handle in scan_tasks.drain(..) {
                handle.abort();
            }
        }

        let rt_guard = TOKIO_RUNTIME.lock().unwrap();
        let rt = match rt_guard.as_ref() {
            Some(rt) => rt,
            None => return -1,
        };

        let adapters = BLE_ADAPTERS.lock().unwrap().clone();
        for (index, adapter) in adapters.into_iter().enumerate() {
            if let Err(e) = rt.block_on(async { adapter.stop_scan().await }) {
                unity_log_error!("Failed to stop scan on adapter {}: {:?}", index, e);
            }
        }

        unity_log!("Scan stopped");
        0
    })
    .unwrap_or(-1)
}

/// Return number of discovered devices
#[ffi_function]
#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn BLEGetDiscoveredDevicesCount() -> i32 {
    catch_unwind(|| DISCOVERED_DEVICES.lock().unwrap().len() as i32).unwrap_or(-1)
}

/// Return JSON array of discovered devices
///
/// # Safety
/// `out_buffer` must point to a valid writable buffer of at least `max_len` bytes.
#[ffi_function]
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn BLEGetDiscoveredDevicesJson(out_buffer: *mut u8, max_len: u32) -> i32 {
    catch_unwind(|| {
        let devices = DISCOVERED_DEVICES.lock().unwrap();
        let list: Vec<&DiscoveredDeviceInfo> = devices.values().collect();
        let json_str = serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string());
        copy_str_to_c_buffer(&json_str, out_buffer, max_len)
    })
    .unwrap_or(-1)
}

/// Connect to a device by ID
///
/// # Safety
/// `device_id` must be a valid null-terminated C string.
#[ffi_function]
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn BLEConnect(device_id: *const c_char) -> i32 {
    catch_unwind(|| {
        let device_id_str = match c_str_to_string(device_id) {
            Some(s) => s,
            None => return -1,
        };

        unity_log!("Connecting to device: {}", device_id_str);

        let peripheral = {
            let cache = DISCOVERED_PERIPHERALS.lock().unwrap();
            cache.get(&device_id_str).cloned()
        };

        let peripheral = match peripheral {
            Some(p) => p,
            None => {
                unity_log_error!("Device not found in discovery cache: {}", device_id_str);
                return -1;
            }
        };

        let rt_guard = TOKIO_RUNTIME.lock().unwrap();
        let rt = match rt_guard.as_ref() {
            Some(rt) => rt,
            None => return -1,
        };

        let connect_result = rt.block_on(async {
            tokio::time::timeout(Duration::from_secs(10), peripheral.connect()).await
        });

        match connect_result {
            Ok(Ok(_)) => {
                CONNECTED_PERIPHERALS
                    .lock()
                    .unwrap()
                    .insert(device_id_str.clone(), peripheral);
                EVENT_QUEUE.push(BleEvent::Connected {
                    id: device_id_str.clone(),
                });
                unity_log!("Connected to device: {}", device_id_str);
                0
            }
            Ok(Err(e)) => {
                unity_log_error!("Failed to connect to {}: {:?}", device_id_str, e);
                EVENT_QUEUE.push(BleEvent::Error {
                    message: format!("Failed to connect: {:?}", e),
                    id: Some(device_id_str),
                });
                -1
            }
            Err(_) => {
                unity_log_error!("Connection to {} timed out", device_id_str);
                EVENT_QUEUE.push(BleEvent::Error {
                    message: "Connection timed out".into(),
                    id: Some(device_id_str),
                });
                -1
            }
        }
    })
    .unwrap_or(-1)
}

/// Disconnect from a device by ID
///
/// # Safety
/// `device_id` must be a valid null-terminated C string.
#[ffi_function]
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn BLEDisconnect(device_id: *const c_char) -> i32 {
    catch_unwind(|| {
        let device_id_str = match c_str_to_string(device_id) {
            Some(s) => s,
            None => return -1,
        };

        unity_log!("Disconnecting from device: {}", device_id_str);

        let peripheral = CONNECTED_PERIPHERALS.lock().unwrap().remove(&device_id_str);
        let peripheral = match peripheral {
            Some(p) => p,
            None => return -1,
        };

        let rt_guard = TOKIO_RUNTIME.lock().unwrap();
        let rt = match rt_guard.as_ref() {
            Some(rt) => rt,
            None => return -1,
        };

        match rt.block_on(async { peripheral.disconnect().await }) {
            Ok(_) => {
                EVENT_QUEUE.push(BleEvent::Disconnected {
                    id: device_id_str.clone(),
                });
                unity_log!("Disconnected from device: {}", device_id_str);
                0
            }
            Err(e) => {
                unity_log_error!("Failed to disconnect: {:?}", e);
                -1
            }
        }
    })
    .unwrap_or(-1)
}

/// Check if a peripheral is connected (1 = connected, 0 = disconnected, -1 = error)
///
/// # Safety
/// `device_id` must be a valid null-terminated C string.
#[ffi_function]
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn BLEIsConnected(device_id: *const c_char) -> i32 {
    catch_unwind(|| {
        let device_id_str = match c_str_to_string(device_id) {
            Some(s) => s,
            None => return -1,
        };

        let peripheral = CONNECTED_PERIPHERALS
            .lock()
            .unwrap()
            .get(&device_id_str)
            .cloned();

        let peripheral = match peripheral {
            Some(p) => p,
            None => return 0,
        };

        let rt_guard = TOKIO_RUNTIME.lock().unwrap();
        let rt = match rt_guard.as_ref() {
            Some(rt) => rt,
            None => return -1,
        };

        match rt.block_on(async { peripheral.is_connected().await }) {
            Ok(connected) => {
                if connected {
                    1
                } else {
                    0
                }
            }
            Err(_) => -1,
        }
    })
    .unwrap_or(-1)
}

/// Discover GATT services for a connected peripheral
///
/// # Safety
/// `device_id` must be a valid null-terminated C string.
#[ffi_function]
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn BLEDiscoverServices(device_id: *const c_char) -> i32 {
    catch_unwind(|| {
        let device_id_str = match c_str_to_string(device_id) {
            Some(s) => s,
            None => return -1,
        };

        let peripheral = CONNECTED_PERIPHERALS
            .lock()
            .unwrap()
            .get(&device_id_str)
            .cloned();

        let peripheral = match peripheral {
            Some(p) => p,
            None => return -1,
        };

        let rt_guard = TOKIO_RUNTIME.lock().unwrap();
        let rt = match rt_guard.as_ref() {
            Some(rt) => rt,
            None => return -1,
        };

        match rt.block_on(async { peripheral.discover_services().await }) {
            Ok(_) => {
                unity_log!("Services discovered for: {}", device_id_str);
                0
            }
            Err(e) => {
                unity_log_error!("Failed to discover services: {:?}", e);
                -1
            }
        }
    })
    .unwrap_or(-1)
}

/// Get GATT services, characteristics, and descriptors profile as JSON string
///
/// # Safety
/// `device_id` must be a valid null-terminated C string. `out_buffer` must point to a valid buffer of at least `max_len` bytes.
#[ffi_function]
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn BLEGetGattProfileJson(
    device_id: *const c_char,
    out_buffer: *mut u8,
    max_len: u32,
) -> i32 {
    catch_unwind(|| {
        let device_id_str = match c_str_to_string(device_id) {
            Some(s) => s,
            None => return -1,
        };

        let peripheral = CONNECTED_PERIPHERALS
            .lock()
            .unwrap()
            .get(&device_id_str)
            .cloned();

        let peripheral = match peripheral {
            Some(p) => p,
            None => return -1,
        };

        let mut services_info = Vec::new();
        for service in peripheral.services() {
            let mut chars_info = Vec::new();
            for c in &service.characteristics {
                let descriptors: Vec<String> =
                    c.descriptors.iter().map(|d| d.uuid.to_string()).collect();
                chars_info.push(GattCharInfo {
                    uuid: c.uuid.to_string(),
                    can_read: c.properties.contains(CharPropFlags::READ),
                    can_write: c.properties.contains(CharPropFlags::WRITE),
                    can_write_without_response: c
                        .properties
                        .contains(CharPropFlags::WRITE_WITHOUT_RESPONSE),
                    can_notify: c.properties.contains(CharPropFlags::NOTIFY),
                    can_indicate: c.properties.contains(CharPropFlags::INDICATE),
                    descriptors,
                });
            }

            services_info.push(GattServiceInfo {
                uuid: service.uuid.to_string(),
                primary: service.primary,
                characteristics: chars_info,
            });
        }

        let json_str = serde_json::to_string(&services_info).unwrap_or_else(|_| "[]".to_string());
        copy_str_to_c_buffer(&json_str, out_buffer, max_len)
    })
    .unwrap_or(-1)
}

/// Read characteristic value from a peripheral
///
/// # Safety
/// `device_id` must be a valid null-terminated C string. `data` must point to a valid writable buffer of at least `max_len` bytes.
#[ffi_function]
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn BLEReadCharacteristic(
    device_id: *const c_char,
    characteristic_uuid: BLEUuid,
    data: *mut u8,
    max_len: u32,
) -> i32 {
    catch_unwind(|| {
        let device_id_str = match c_str_to_string(device_id) {
            Some(s) => s,
            None => return -1,
        };

        let uuid = Uuid::from(characteristic_uuid);
        let peripheral = CONNECTED_PERIPHERALS
            .lock()
            .unwrap()
            .get(&device_id_str)
            .cloned();

        let peripheral = match peripheral {
            Some(p) => p,
            None => return -1,
        };

        let rt_guard = TOKIO_RUNTIME.lock().unwrap();
        let rt = match rt_guard.as_ref() {
            Some(rt) => rt,
            None => return -1,
        };

        let result = rt.block_on(async {
            for char in peripheral.characteristics() {
                if char.uuid == uuid {
                    return peripheral.read(&char).await;
                }
            }
            Err(btleplug::Error::Other("Characteristic not found".into()))
        });

        match result {
            Ok(vec_data) => copy_to_c_buffer(&vec_data, data, max_len),
            Err(e) => {
                unity_log_error!("Failed to read characteristic {}: {:?}", uuid, e);
                -1
            }
        }
    })
    .unwrap_or(-1)
}

/// Write data to a characteristic
///
/// # Safety
/// `device_id` must be a valid null-terminated C string. `data` must point to a valid readable buffer of at least `len` bytes if `len > 0`.
#[ffi_function]
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn BLEWriteCharacteristic(
    device_id: *const c_char,
    characteristic_uuid: BLEUuid,
    data: *const u8,
    len: u32,
    without_response: i32,
) -> i32 {
    catch_unwind(|| {
        let device_id_str = match c_str_to_string(device_id) {
            Some(s) => s,
            None => return -1,
        };

        if data.is_null() && len > 0 {
            return -1;
        }

        let slice = if len > 0 {
            std::slice::from_raw_parts(data, len as usize)
        } else {
            &[]
        };

        let uuid = Uuid::from(characteristic_uuid);
        let peripheral = CONNECTED_PERIPHERALS
            .lock()
            .unwrap()
            .get(&device_id_str)
            .cloned();

        let peripheral = match peripheral {
            Some(p) => p,
            None => return -1,
        };

        let rt_guard = TOKIO_RUNTIME.lock().unwrap();
        let rt = match rt_guard.as_ref() {
            Some(rt) => rt,
            None => return -1,
        };

        let write_type = if without_response != 0 {
            btleplug::api::WriteType::WithoutResponse
        } else {
            btleplug::api::WriteType::WithResponse
        };

        let result = rt.block_on(async {
            for char in peripheral.characteristics() {
                if char.uuid == uuid {
                    return peripheral.write(&char, slice, write_type).await;
                }
            }
            Err(btleplug::Error::Other("Characteristic not found".into()))
        });

        match result {
            Ok(_) => 0,
            Err(e) => {
                unity_log_error!("Failed to write characteristic {}: {:?}", uuid, e);
                -1
            }
        }
    })
    .unwrap_or(-1)
}

/// Subscribe to characteristic notifications/indications and stream updates to event queue
///
/// # Safety
/// `device_id` must be a valid null-terminated C string.
#[ffi_function]
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn BLESubscribeCharacteristic(
    device_id: *const c_char,
    characteristic_uuid: BLEUuid,
) -> i32 {
    catch_unwind(|| {
        let device_id_str = match c_str_to_string(device_id) {
            Some(s) => s,
            None => return -1,
        };

        let uuid = Uuid::from(characteristic_uuid);
        let peripheral = CONNECTED_PERIPHERALS
            .lock()
            .unwrap()
            .get(&device_id_str)
            .cloned();

        let peripheral = match peripheral {
            Some(p) => p,
            None => return -1,
        };

        let rt_guard = TOKIO_RUNTIME.lock().unwrap();
        let rt = match rt_guard.as_ref() {
            Some(rt) => rt,
            None => return -1,
        };

        let target_char = peripheral
            .characteristics()
            .into_iter()
            .find(|c| c.uuid == uuid);

        let target_char = match target_char {
            Some(c) => c,
            None => {
                unity_log_error!("Characteristic not found: {}", uuid);
                return -1;
            }
        };

        let sub_result = rt.block_on(async { peripheral.subscribe(&target_char).await });
        if let Err(e) = sub_result {
            unity_log_error!("Failed to subscribe to characteristic: {:?}", e);
            return -1;
        }

        // Spawn notification stream reader task
        let dev_id_clone = device_id_str.clone();
        let periph_clone = peripheral.clone();
        let task_key = format!("{}_{}", device_id_str, uuid);

        let task_handle = rt.spawn(async move {
            if let Ok(mut notif_stream) = periph_clone.notifications().await {
                while let Some(notif) = notif_stream.next().await {
                    if notif.uuid == uuid {
                        EVENT_QUEUE.push(BleEvent::Notification {
                            id: dev_id_clone.clone(),
                            characteristic: notif.uuid.to_string(),
                            data: notif.value,
                        });
                    }
                }
            }
        });

        NOTIFICATION_TASKS
            .lock()
            .unwrap()
            .insert(task_key, task_handle);
        0
    })
    .unwrap_or(-1)
}

/// Unsubscribe from characteristic notifications
///
/// # Safety
/// `device_id` must be a valid null-terminated C string.
#[ffi_function]
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn BLEUnsubscribeCharacteristic(
    device_id: *const c_char,
    characteristic_uuid: BLEUuid,
) -> i32 {
    catch_unwind(|| {
        let device_id_str = match c_str_to_string(device_id) {
            Some(s) => s,
            None => return -1,
        };

        let uuid = Uuid::from(characteristic_uuid);
        let task_key = format!("{}_{}", device_id_str, uuid);

        if let Some(handle) = NOTIFICATION_TASKS.lock().unwrap().remove(&task_key) {
            handle.abort();
        }

        let peripheral = CONNECTED_PERIPHERALS
            .lock()
            .unwrap()
            .get(&device_id_str)
            .cloned();

        let peripheral = match peripheral {
            Some(p) => p,
            None => return -1,
        };

        let rt_guard = TOKIO_RUNTIME.lock().unwrap();
        let rt = match rt_guard.as_ref() {
            Some(rt) => rt,
            None => return -1,
        };

        let target_char = peripheral
            .characteristics()
            .into_iter()
            .find(|c| c.uuid == uuid);

        if let Some(c) = target_char {
            let _ = rt.block_on(async { peripheral.unsubscribe(&c).await });
        }

        0
    })
    .unwrap_or(-1)
}

/// Read descriptor value
///
/// # Safety
/// `device_id` must be a valid null-terminated C string. `data` must point to a valid writable buffer of at least `max_len` bytes.
#[ffi_function]
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn BLEReadDescriptor(
    device_id: *const c_char,
    characteristic_uuid: BLEUuid,
    descriptor_uuid: BLEUuid,
    data: *mut u8,
    max_len: u32,
) -> i32 {
    catch_unwind(|| {
        let device_id_str = match c_str_to_string(device_id) {
            Some(s) => s,
            None => return -1,
        };

        let char_uuid = Uuid::from(characteristic_uuid);
        let desc_uuid = Uuid::from(descriptor_uuid);

        let peripheral = CONNECTED_PERIPHERALS
            .lock()
            .unwrap()
            .get(&device_id_str)
            .cloned();

        let peripheral = match peripheral {
            Some(p) => p,
            None => return -1,
        };

        let rt_guard = TOKIO_RUNTIME.lock().unwrap();
        let rt = match rt_guard.as_ref() {
            Some(rt) => rt,
            None => return -1,
        };

        let result = rt.block_on(async {
            for char in peripheral.characteristics() {
                if char_uuid.is_nil() || char.uuid == char_uuid {
                    for desc in char.descriptors {
                        if desc.uuid == desc_uuid {
                            return peripheral.read_descriptor(&desc).await;
                        }
                    }
                }
            }
            Err(btleplug::Error::Other("Descriptor not found".into()))
        });

        match result {
            Ok(vec_data) => copy_to_c_buffer(&vec_data, data, max_len),
            Err(e) => {
                unity_log_error!("Failed to read descriptor {}: {:?}", desc_uuid, e);
                -1
            }
        }
    })
    .unwrap_or(-1)
}

/// Write descriptor value
///
/// # Safety
/// `device_id` must be a valid null-terminated C string. `data` must point to a valid readable buffer of at least `len` bytes if `len > 0`.
#[ffi_function]
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn BLEWriteDescriptor(
    device_id: *const c_char,
    characteristic_uuid: BLEUuid,
    descriptor_uuid: BLEUuid,
    data: *const u8,
    len: u32,
) -> i32 {
    catch_unwind(|| {
        let device_id_str = match c_str_to_string(device_id) {
            Some(s) => s,
            None => return -1,
        };

        if data.is_null() && len > 0 {
            return -1;
        }

        let slice = if len > 0 {
            std::slice::from_raw_parts(data, len as usize)
        } else {
            &[]
        };

        let char_uuid = Uuid::from(characteristic_uuid);
        let desc_uuid = Uuid::from(descriptor_uuid);

        let peripheral = CONNECTED_PERIPHERALS
            .lock()
            .unwrap()
            .get(&device_id_str)
            .cloned();

        let peripheral = match peripheral {
            Some(p) => p,
            None => return -1,
        };

        let rt_guard = TOKIO_RUNTIME.lock().unwrap();
        let rt = match rt_guard.as_ref() {
            Some(rt) => rt,
            None => return -1,
        };

        let result = rt.block_on(async {
            for char in peripheral.characteristics() {
                if char_uuid.is_nil() || char.uuid == char_uuid {
                    for desc in char.descriptors {
                        if desc.uuid == desc_uuid {
                            return peripheral.write_descriptor(&desc, slice).await;
                        }
                    }
                }
            }
            Err(btleplug::Error::Other("Descriptor not found".into()))
        });

        match result {
            Ok(_) => 0,
            Err(e) => {
                unity_log_error!("Failed to write descriptor {}: {:?}", desc_uuid, e);
                -1
            }
        }
    })
    .unwrap_or(-1)
}

/// Poll the next event from the event queue as JSON.
/// Returns number of bytes written, 0 if queue is empty, or -1 on error.
///
/// # Safety
/// `out_buffer` must point to a valid writable buffer of at least `max_len` bytes.
#[ffi_function]
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn BLEPollEventJson(out_buffer: *mut u8, max_len: u32) -> i32 {
    catch_unwind(|| {
        if let Some(event) = EVENT_QUEUE.pop() {
            if let Ok(json_str) = serde_json::to_string(&event) {
                return copy_str_to_c_buffer(&json_str, out_buffer, max_len);
            }
        }
        0
    })
    .unwrap_or(-1)
}

// ===== Android JNI Helper =====

/// Initialize Android JNI environment for BLE
/// Called automatically by Unity C# on Android startup
///
/// # Safety
/// `_jvm` and `_activity` must be valid JavaVM and Activity pointers provided by Android JNI.
#[ffi_function]
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn BLEInitAndroid(
    _jvm: *mut std::ffi::c_void,
    _activity: *mut std::ffi::c_void,
) -> i32 {
    #[cfg(target_os = "android")]
    {
        if _jvm.is_null() {
            return -1;
        }
        let jvm_raw = _jvm as *mut jni::sys::JavaVM;
        let vm = match jni::JavaVM::from_raw(jvm_raw) {
            Ok(v) => v,
            Err(_) => return -1,
        };
        *ANDROID_JVM.lock().unwrap() = Some(vm);

        let jvm_guard = ANDROID_JVM.lock().unwrap();
        let vm_ref = jvm_guard.as_ref().unwrap();
        let env = match vm_ref.attach_current_thread() {
            Ok(e) => e,
            Err(_) => return -1,
        };

        if !_activity.is_null() {
            let activity_obj = jni::objects::JObject::from(_activity as jni::sys::jobject);
            if let Ok(global_ref) = env.new_global_ref(activity_obj) {
                *ANDROID_APP_CONTEXT.lock().unwrap() = Some(global_ref);
            }
        }

        if btleplug::platform::init(&env).is_err() {
            return -1;
        }
    }
    0
}

#[cfg(target_os = "android")]
#[no_mangle]
pub unsafe extern "system" fn Java_com_dogedada_nativeble_BlePeripheralHelper_onWriteRequest(
    env: jni::JNIEnv,
    _class: jni::objects::JClass,
    client: jni::objects::JString,
    char_uuid: jni::objects::JString,
    data: jni::sys::jbyteArray,
) {
    let client_str: String = env.get_string(client).map(|s| s.into()).unwrap_or_default();
    let char_str: String = env.get_string(char_uuid).map(|s| s.into()).unwrap_or_default();
    let bytes = if !data.is_null() {
        let len = env.get_array_length(data).unwrap_or(0);
        let mut buf = vec![0i8; len as usize];
        let _ = env.get_byte_array_region(data, 0, &mut buf);
        buf.into_iter().map(|b| b as u8).collect()
    } else {
        Vec::new()
    };

    EVENT_QUEUE.push(BleEvent::PeripheralWrite {
        client: client_str,
        characteristic: char_str,
        data: bytes,
    });
}

#[cfg(target_os = "android")]
#[no_mangle]
pub unsafe extern "system" fn Java_com_dogedada_nativeble_BlePeripheralHelper_onSubscriptionChange(
    env: jni::JNIEnv,
    _class: jni::objects::JClass,
    client: jni::objects::JString,
    char_uuid: jni::objects::JString,
    subscribed: jni::sys::jboolean,
) {
    let client_str: String = env.get_string(client).map(|s| s.into()).unwrap_or_default();
    let char_str: String = env.get_string(char_uuid).map(|s| s.into()).unwrap_or_default();

    EVENT_QUEUE.push(BleEvent::PeripheralSubscription {
        client: client_str,
        characteristic: char_str,
        subscribed: subscribed != 0,
    });
}

// ===== Peripheral (GATT Server) Mode =====

/// Start advertising as a BLE peripheral
///
/// # Safety
/// `name` and `service_uuids_csv` must be null or valid null-terminated C strings.
#[ffi_function]
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn BLEPeripheralStartAdvertising(
    name: *const std::os::raw::c_char,
    service_uuids_csv: *const std::os::raw::c_char,
) -> i32 {
    #[cfg(target_os = "android")]
    {
        catch_unwind(|| {
            let name_str = if name.is_null() {
                "UnityBLE"
            } else {
                unsafe { CStr::from_ptr(name) }.to_str().unwrap_or("UnityBLE")
            };
            let csv_str = if service_uuids_csv.is_null() {
                ""
            } else {
                unsafe { CStr::from_ptr(service_uuids_csv) }.to_str().unwrap_or("")
            };

            let jvm_guard = ANDROID_JVM.lock().unwrap();
            let vm = match jvm_guard.as_ref() {
                Some(v) => v,
                None => {
                    unity_log_error!("Android JVM not initialized");
                    return -1;
                }
            };
            let env = match vm.attach_current_thread() {
                Ok(e) => e,
                Err(_) => return -1,
            };

            let ctx_guard = ANDROID_APP_CONTEXT.lock().unwrap();
            let ctx = match ctx_guard.as_ref() {
                Some(c) => c.as_obj(),
                None => {
                    unity_log_error!("Android App Context not initialized");
                    return -1;
                }
            };

            let helper_class = match env.find_class("com/dogedada/nativeble/BlePeripheralHelper") {
                Ok(c) => c,
                Err(e) => {
                    unity_log_error!("BlePeripheralHelper not found: {:?}", e);
                    return -1;
                }
            };

            let name_jstr = match env.new_string(name_str) {
                Ok(s) => s,
                Err(_) => return -1,
            };
            let csv_jstr = match env.new_string(csv_str) {
                Ok(s) => s,
                Err(_) => return -1,
            };

            let res = env.call_static_method(
                helper_class,
                "startAdvertising",
                "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)I",
                &[
                    jni::objects::JValue::Object(ctx),
                    jni::objects::JValue::Object(name_jstr.into()),
                    jni::objects::JValue::Object(csv_jstr.into()),
                ],
            );

            match res {
                Ok(v) => {
                    let code = v.i().unwrap_or(-1);
                    if code == 0 {
                        PERIPHERAL_ADVERTISING.store(true, Ordering::SeqCst);
                    }
                    code
                }
                Err(e) => {
                    unity_log_error!("startAdvertising JNI call failed: {:?}", e);
                    -1
                }
            }
        })
        .unwrap_or(-1)
    }
    #[cfg(not(target_os = "android"))]
    {
        catch_unwind(|| {
            let name_str = if name.is_null() {
                "UnityBLE"
            } else {
                unsafe { CStr::from_ptr(name) }.to_str().unwrap_or("UnityBLE")
            };

            let mut uuids = Vec::new();
            if !service_uuids_csv.is_null() {
                let csv = unsafe { CStr::from_ptr(service_uuids_csv) }.to_str().unwrap_or("");
                for part in csv.split(',') {
                    let trimmed = part.trim();
                    if !trimmed.is_empty() {
                        if let Some(u) = parse_ble_uuid_str(trimmed) {
                            uuids.push(u);
                        }
                    }
                }
            }

            let rt_guard = TOKIO_RUNTIME.lock().unwrap();
            let rt = match rt_guard.as_ref() {
                Some(rt) => rt,
                None => {
                    unity_log_error!("BLE subsystem not initialized. Call BLEInit first.");
                    return -1;
                }
            };

            let mut periph_guard = PERIPHERAL_INSTANCE.lock().unwrap();
            if periph_guard.is_none() {
                let (sender_tx, mut sender_rx) = tokio::sync::mpsc::channel(256);
                let p = match rt.block_on(async { ble_peripheral_rust::Peripheral::new(sender_tx).await }) {
                    Ok(p) => p,
                    Err(e) => {
                        unity_log_error!("Failed to create peripheral: {:?}", e);
                        return -1;
                    }
                };

                // Spawn background event listener for peripheral
                rt.spawn(async move {
                    while let Some(ev) = sender_rx.recv().await {
                        match ev {
                            ble_peripheral_rust::gatt::peripheral_event::PeripheralEvent::WriteRequest {
                                request,
                                value,
                                responder,
                                ..
                            } => {
                                EVENT_QUEUE.push(BleEvent::PeripheralWrite {
                                    client: request.client,
                                    characteristic: request.characteristic.to_string(),
                                    data: value,
                                });
                                let _ = responder.send(
                                    ble_peripheral_rust::gatt::peripheral_event::WriteRequestResponse {
                                        response: ble_peripheral_rust::gatt::peripheral_event::RequestResponse::Success,
                                    },
                                );
                            }
                            ble_peripheral_rust::gatt::peripheral_event::PeripheralEvent::CharacteristicSubscriptionUpdate {
                                request,
                                subscribed,
                            } => {
                                EVENT_QUEUE.push(BleEvent::PeripheralSubscription {
                                    client: request.client,
                                    characteristic: request.characteristic.to_string(),
                                    subscribed,
                                });
                            }
                            ble_peripheral_rust::gatt::peripheral_event::PeripheralEvent::ReadRequest {
                                responder,
                                ..
                            } => {
                                let _ = responder.send(
                                    ble_peripheral_rust::gatt::peripheral_event::ReadRequestResponse {
                                        value: Vec::new(),
                                        response: ble_peripheral_rust::gatt::peripheral_event::RequestResponse::Success,
                                    },
                                );
                            }
                            _ => {}
                        }
                    }
                });

                *periph_guard = Some(p);
            }

            if let Some(p) = periph_guard.as_mut() {
                let services_lock = PERIPHERAL_SERVICES.lock().unwrap();
                for s in services_lock.iter() {
                    let _ = rt.block_on(async { p.add_service(s).await });
                }

                let res = rt.block_on(async {
                    p.start_advertising(name_str, &uuids).await
                });
                match res {
                    Ok(_) => {
                        PERIPHERAL_ADVERTISING.store(true, Ordering::SeqCst);
                        unity_log!("Peripheral advertising started: '{}'", name_str);
                        0
                    }
                    Err(e) => {
                        unity_log_error!("Failed to start peripheral advertising: {:?}", e);
                        -1
                    }
                }
            } else {
                -1
            }
        })
        .unwrap_or(-1)
    }
}

/// Stop peripheral advertising
#[ffi_function]
#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn BLEPeripheralStopAdvertising() -> i32 {
    #[cfg(target_os = "android")]
    {
        catch_unwind(|| {
            let jvm_guard = ANDROID_JVM.lock().unwrap();
            if let Some(vm) = jvm_guard.as_ref() {
                if let Ok(env) = vm.attach_current_thread() {
                    if let Ok(helper_class) = env.find_class("com/dogedada/nativeble/BlePeripheralHelper") {
                        let _ = env.call_static_method(helper_class, "stopAdvertising", "()I", &[]);
                    }
                }
            }
            PERIPHERAL_ADVERTISING.store(false, Ordering::SeqCst);
            0
        })
        .unwrap_or(-1)
    }
    #[cfg(not(target_os = "android"))]
    {
        catch_unwind(|| {
            let rt_guard = TOKIO_RUNTIME.lock().unwrap();
            if let Some(rt) = rt_guard.as_ref() {
                let mut periph_guard = PERIPHERAL_INSTANCE.lock().unwrap();
                if let Some(p) = periph_guard.as_mut() {
                    let _ = rt.block_on(async { p.stop_advertising().await });
                }
            }
            PERIPHERAL_ADVERTISING.store(false, Ordering::SeqCst);
            unity_log!("Peripheral advertising stopped");
            0
        })
        .unwrap_or(-1)
    }
}

/// Query whether peripheral is advertising
#[ffi_function]
#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn BLEPeripheralIsAdvertising() -> i32 {
    if PERIPHERAL_ADVERTISING.load(Ordering::SeqCst) {
        1
    } else {
        0
    }
}

/// Define a GATT Service on the peripheral
#[ffi_function]
#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn BLEPeripheralAddService(service_uuid: BLEUuid, primary: i32) -> i32 {
    #[cfg(target_os = "android")]
    {
        catch_unwind(|| {
            let u: Uuid = service_uuid.into();
            let u_str = u.to_string();

            let jvm_guard = ANDROID_JVM.lock().unwrap();
            let vm = match jvm_guard.as_ref() {
                Some(v) => v,
                None => return -1,
            };
            let env = match vm.attach_current_thread() {
                Ok(e) => e,
                Err(_) => return -1,
            };

            let helper_class = match env.find_class("com/dogedada/nativeble/BlePeripheralHelper") {
                Ok(c) => c,
                Err(_) => return -1,
            };

            let u_jstr = match env.new_string(&u_str) {
                Ok(s) => s,
                Err(_) => return -1,
            };

            let res = env.call_static_method(
                helper_class,
                "addService",
                "(Ljava/lang/String;Z)I",
                &[
                    jni::objects::JValue::Object(u_jstr.into()),
                    jni::objects::JValue::Bool((primary != 0) as u8),
                ],
            );
            res.map(|v| v.i().unwrap_or(-1)).unwrap_or(-1)
        })
        .unwrap_or(-1)
    }
    #[cfg(not(target_os = "android"))]
    {
        catch_unwind(|| {
            let u: Uuid = service_uuid.into();
            let mut services = PERIPHERAL_SERVICES.lock().unwrap();
            if !services.iter().any(|s| s.uuid == u) {
                services.push(ble_peripheral_rust::gatt::service::Service {
                    uuid: u,
                    primary: primary != 0,
                    characteristics: Vec::new(),
                });
            }
            0
        })
        .unwrap_or(-1)
    }
}

/// Define a GATT Characteristic on a peripheral service
#[ffi_function]
#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn BLEPeripheralAddCharacteristic(
    service_uuid: BLEUuid,
    char_uuid: BLEUuid,
    can_read: i32,
    can_write: i32,
    can_notify: i32,
) -> i32 {
    #[cfg(target_os = "android")]
    {
        catch_unwind(|| {
            let s_uuid: Uuid = service_uuid.into();
            let c_uuid: Uuid = char_uuid.into();

            let jvm_guard = ANDROID_JVM.lock().unwrap();
            let vm = match jvm_guard.as_ref() {
                Some(v) => v,
                None => return -1,
            };
            let env = match vm.attach_current_thread() {
                Ok(e) => e,
                Err(_) => return -1,
            };

            let helper_class = match env.find_class("com/dogedada/nativeble/BlePeripheralHelper") {
                Ok(c) => c,
                Err(_) => return -1,
            };

            let s_jstr = match env.new_string(s_uuid.to_string()) {
                Ok(s) => s,
                Err(_) => return -1,
            };
            let c_jstr = match env.new_string(c_uuid.to_string()) {
                Ok(s) => s,
                Err(_) => return -1,
            };

            let res = env.call_static_method(
                helper_class,
                "addCharacteristic",
                "(Ljava/lang/String;Ljava/lang/String;ZZZ)I",
                &[
                    jni::objects::JValue::Object(s_jstr.into()),
                    jni::objects::JValue::Object(c_jstr.into()),
                    jni::objects::JValue::Bool((can_read != 0) as u8),
                    jni::objects::JValue::Bool((can_write != 0) as u8),
                    jni::objects::JValue::Bool((can_notify != 0) as u8),
                ],
            );
            res.map(|v| v.i().unwrap_or(-1)).unwrap_or(-1)
        })
        .unwrap_or(-1)
    }
    #[cfg(not(target_os = "android"))]
    {
        catch_unwind(|| {
            let s_uuid: Uuid = service_uuid.into();
            let c_uuid: Uuid = char_uuid.into();

            use ble_peripheral_rust::gatt::properties::{AttributePermission, CharacteristicProperty};

            let mut props = Vec::new();
            let mut perms = Vec::new();

            if can_read != 0 {
                props.push(CharacteristicProperty::Read);
                perms.push(AttributePermission::Readable);
            }
            if can_write != 0 {
                props.push(CharacteristicProperty::Write);
                perms.push(AttributePermission::Writeable);
            }
            if can_notify != 0 {
                props.push(CharacteristicProperty::Notify);
            }

            let mut services = PERIPHERAL_SERVICES.lock().unwrap();
            let service = match services.iter_mut().find(|s| s.uuid == s_uuid) {
                Some(s) => s,
                None => {
                    services.push(ble_peripheral_rust::gatt::service::Service {
                        uuid: s_uuid,
                        primary: true,
                        characteristics: Vec::new(),
                    });
                    services.last_mut().unwrap()
                }
            };

            service.characteristics.push(ble_peripheral_rust::gatt::characteristic::Characteristic {
                uuid: c_uuid,
                properties: props,
                permissions: perms,
                value: None,
                descriptors: Vec::new(),
            });

            0
        })
        .unwrap_or(-1)
    }
}

/// Update characteristic value and notify subscribed centrals
///
/// # Safety
/// If `len > 0`, `data` must point to a valid readable byte buffer of at least `len` bytes.
#[ffi_function]
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn BLEPeripheralUpdateCharacteristic(
    char_uuid: BLEUuid,
    data: *const u8,
    len: u32,
) -> i32 {
    #[cfg(target_os = "android")]
    {
        catch_unwind(|| {
            let c_uuid: Uuid = char_uuid.into();
            let bytes = if data.is_null() || len == 0 {
                Vec::new()
            } else {
                unsafe { std::slice::from_raw_parts(data, len as usize) }.to_vec()
            };

            let jvm_guard = ANDROID_JVM.lock().unwrap();
            let vm = match jvm_guard.as_ref() {
                Some(v) => v,
                None => return -1,
            };
            let env = match vm.attach_current_thread() {
                Ok(e) => e,
                Err(_) => return -1,
            };

            let helper_class = match env.find_class("com/dogedada/nativeble/BlePeripheralHelper") {
                Ok(c) => c,
                Err(_) => return -1,
            };

            let c_jstr = match env.new_string(c_uuid.to_string()) {
                Ok(s) => s,
                Err(_) => return -1,
            };

            let byte_arr = match env.byte_array_from_slice(&bytes) {
                Ok(a) => a,
                Err(_) => return -1,
            };

            let res = env.call_static_method(
                helper_class,
                "updateCharacteristic",
                "(Ljava/lang/String;[B)I",
                &[
                    jni::objects::JValue::Object(c_jstr.into()),
                    jni::objects::JValue::Object(byte_arr.into()),
                ],
            );
            res.map(|v| v.i().unwrap_or(-1)).unwrap_or(-1)
        })
        .unwrap_or(-1)
    }
    #[cfg(not(target_os = "android"))]
    {
        catch_unwind(|| {
            let c_uuid: Uuid = char_uuid.into();
            let bytes = if data.is_null() || len == 0 {
                Vec::new()
            } else {
                unsafe { std::slice::from_raw_parts(data, len as usize) }.to_vec()
            };

            let rt_guard = TOKIO_RUNTIME.lock().unwrap();
            if let Some(rt) = rt_guard.as_ref() {
                let mut periph_guard = PERIPHERAL_INSTANCE.lock().unwrap();
                if let Some(p) = periph_guard.as_mut() {
                    let res = rt.block_on(async {
                        p.update_characteristic(c_uuid, bytes).await
                    });
                    if res.is_ok() {
                        return 0;
                    }
                }
            }
            -1
        })
        .unwrap_or(-1)
    }
}

/// Test logging to Unity console
#[ffi_function]
#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn TestLogging() -> i32 {
    unity_log!("Test: Info log from Rust");
    unity_log_warning!("Test: Warning log from Rust");
    unity_log_error!("Test: Error log from Rust");
    0
}

// ===== Interoptopus Inventory =====

pub fn ffi_inventory() -> interoptopus::Inventory {
    let builder = interoptopus::InventoryBuilder::new();
    builder
        .register(interoptopus::function!(BLEInit))
        .register(interoptopus::function!(BLECleanup))
        .register(interoptopus::function!(BLEGetAdapterCount))
        .register(interoptopus::function!(BLEStartScan))
        .register(interoptopus::function!(BLEStopScan))
        .register(interoptopus::function!(BLEGetDiscoveredDevicesCount))
        .register(interoptopus::function!(BLEGetDiscoveredDevicesJson))
        .register(interoptopus::function!(BLEConnect))
        .register(interoptopus::function!(BLEDisconnect))
        .register(interoptopus::function!(BLEIsConnected))
        .register(interoptopus::function!(BLEDiscoverServices))
        .register(interoptopus::function!(BLEGetGattProfileJson))
        .register(interoptopus::function!(BLEReadCharacteristic))
        .register(interoptopus::function!(BLEWriteCharacteristic))
        .register(interoptopus::function!(BLESubscribeCharacteristic))
        .register(interoptopus::function!(BLEUnsubscribeCharacteristic))
        .register(interoptopus::function!(BLEReadDescriptor))
        .register(interoptopus::function!(BLEWriteDescriptor))
        .register(interoptopus::function!(BLEPollEventJson))
        .register(interoptopus::function!(BLEInitAndroid))
        .register(interoptopus::function!(BLEPeripheralStartAdvertising))
        .register(interoptopus::function!(BLEPeripheralStopAdvertising))
        .register(interoptopus::function!(BLEPeripheralIsAdvertising))
        .register(interoptopus::function!(BLEPeripheralAddService))
        .register(interoptopus::function!(BLEPeripheralAddCharacteristic))
        .register(interoptopus::function!(BLEPeripheralUpdateCharacteristic))
        .register(interoptopus::function!(TestLogging))
        .inventory()
}

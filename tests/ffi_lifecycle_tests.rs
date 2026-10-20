use unity_native_ble::{
    BLECleanup, BLEGetAdapterCount, BLEGetDiscoveredDevicesCount,
    BLEGetDiscoveredDevicesJson, BLEInit, BLEPollEventJson, TestLogging,
    EVENT_QUEUE, BleEvent,
};

#[test]
fn test_logging_ffi() {
    assert_eq!(TestLogging(), 0);
}

#[test]
fn test_event_poll_ffi() {
    EVENT_QUEUE.clear();
    let mut buffer = [0u8; 256];

    // When empty, returns 0
    let res = unsafe { BLEPollEventJson(buffer.as_mut_ptr(), 256) };
    assert_eq!(res, 0);

    // Push an event
    EVENT_QUEUE.push(BleEvent::Connected { id: "test_dev".into() });

    // Poll event
    let res = unsafe { BLEPollEventJson(buffer.as_mut_ptr(), 256) };
    assert!(res > 0);
    let json_str = std::str::from_utf8(&buffer[..res as usize]).unwrap();
    assert!(json_str.contains("\"type\":\"Connected\""));
    assert!(json_str.contains("\"id\":\"test_dev\""));

    // Next poll should be 0 (empty)
    let res = unsafe { BLEPollEventJson(buffer.as_mut_ptr(), 256) };
    assert_eq!(res, 0);
}

#[test]
fn test_discovered_devices_ffi_empty() {
    let count = BLEGetDiscoveredDevicesCount();
    assert_eq!(count, 0);

    let mut buffer = [0u8; 128];
    let len = unsafe { BLEGetDiscoveredDevicesJson(buffer.as_mut_ptr(), 128) };
    assert!(len > 0);
    let json_str = std::str::from_utf8(&buffer[..len as usize]).unwrap();
    assert_eq!(json_str, "[]");
}

#[test]
fn test_init_and_cleanup_lifecycle() {
    let init_res = BLEInit();
    // In headless environments without bluetooth hardware/daemon (e.g. Linux CI container),
    // BLEInit returns -1. On systems with active BLE stacks (macOS, Windows, or Linux with BlueZ), it returns 0.
    if init_res == 0 {
        let count = BLEGetAdapterCount();
        assert!(count >= 0);
    }

    // Cleanup must always succeed safely regardless of init result
    let cleanup_res = BLECleanup();
    assert_eq!(cleanup_res, 0);
}

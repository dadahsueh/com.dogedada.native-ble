use unity_native_ble::{
    copy_str_to_c_buffer, copy_to_c_buffer, parse_ble_uuid_str, BLEUuid,
    BleEvent, BleEventQueue, DiscoveredDeviceInfo, GattCharInfo, GattServiceInfo,
};
use uuid::Uuid;

#[test]
fn test_ble_uuid_conversions() {
    let raw = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    let ble_uuid = BLEUuid { data: raw };
    let uuid = Uuid::from(ble_uuid);
    assert_eq!(*uuid.as_bytes(), raw);

    let roundtrip = BLEUuid::from(uuid);
    assert_eq!(roundtrip.data, raw);
}

#[test]
fn test_parse_ble_uuid_short_and_long() {
    // 16-bit UUID (e.g. Heart Rate Service 0x180D)
    let parsed_short = parse_ble_uuid_str("180d").expect("Failed to parse short UUID");
    let expected = Uuid::parse_str("0000180d-0000-1000-8000-00805f9b34fb").unwrap();
    assert_eq!(parsed_short, expected);

    // 128-bit UUID
    let full_str = "6e400001-b5a3-f393-e0a9-e50e24dcca9e";
    let parsed_long = parse_ble_uuid_str(full_str).expect("Failed to parse long UUID");
    assert_eq!(parsed_long.to_string(), full_str);

    // Invalid string
    assert!(parse_ble_uuid_str("invalid_uuid").is_none());
}

#[test]
fn test_c_buffer_copying() {
    let mut buffer = [0u8; 10];
    let src = b"hello";

    // Query size when buffer is null
    let needed = unsafe { copy_to_c_buffer(src, std::ptr::null_mut(), 0) };
    assert_eq!(needed, 5);

    // Normal copy
    let copied = unsafe { copy_to_c_buffer(src, buffer.as_mut_ptr(), 10) };
    assert_eq!(copied, 5);
    assert_eq!(&buffer[..5], b"hello");

    // Truncated copy
    let mut small_buffer = [0u8; 3];
    let copied_small = unsafe { copy_to_c_buffer(src, small_buffer.as_mut_ptr(), 3) };
    assert_eq!(copied_small, 3);
    assert_eq!(&small_buffer, b"hel");
}

#[test]
fn test_copy_str_to_c_buffer_with_null_terminator() {
    let mut buffer = [0u8; 10];
    let text = "hello";

    let copied = unsafe { copy_str_to_c_buffer(text, buffer.as_mut_ptr(), 10) };
    assert_eq!(copied, 5);
    assert_eq!(&buffer[..6], b"hello\0");

    // Buffer too small: max_len 4 can only hold 3 chars + '\0'
    let mut small_buffer = [0u8; 4];
    let copied_small = unsafe { copy_str_to_c_buffer(text, small_buffer.as_mut_ptr(), 4) };
    assert_eq!(copied_small, 3);
    assert_eq!(&small_buffer, b"hel\0");
}

#[test]
fn test_event_queue_fifo_and_capacity() {
    let queue = BleEventQueue::new(3);
    assert_eq!(queue.len(), 0);

    queue.push(BleEvent::Connected { id: "dev1".into() });
    queue.push(BleEvent::Connected { id: "dev2".into() });
    queue.push(BleEvent::Connected { id: "dev3".into() });
    assert_eq!(queue.len(), 3);

    // Overflow should pop the oldest (dev1)
    queue.push(BleEvent::Connected { id: "dev4".into() });
    assert_eq!(queue.len(), 3);

    let first = queue.pop().expect("Expected event");
    assert_eq!(first, BleEvent::Connected { id: "dev2".into() });

    let second = queue.pop().expect("Expected event");
    assert_eq!(second, BleEvent::Connected { id: "dev3".into() });

    let third = queue.pop().expect("Expected event");
    assert_eq!(third, BleEvent::Connected { id: "dev4".into() });

    assert!(queue.pop().is_none());
}

#[test]
fn test_event_serialization() {
    let dev_info = DiscoveredDeviceInfo {
        id: "AA:BB:CC:DD:EE:FF".into(),
        name: Some("MySensor".into()),
        address: Some("AA:BB:CC:DD:EE:FF".into()),
        rssi: Some(-65),
        services: vec!["0000180d-0000-1000-8000-00805f9b34fb".into()],
    };
    let json_dev = serde_json::to_string(&dev_info).unwrap();
    assert!(json_dev.contains("\"id\":\"AA:BB:CC:DD:EE:FF\""));

    let ev = BleEvent::DeviceDiscovered {
        id: "AA:BB:CC:DD:EE:FF".into(),
        name: Some("MySensor".into()),
        address: Some("AA:BB:CC:DD:EE:FF".into()),
        rssi: Some(-65),
        services: vec!["0000180d-0000-1000-8000-00805f9b34fb".into()],
    };

    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains("\"type\":\"DeviceDiscovered\""));
    assert!(json.contains("\"name\":\"MySensor\""));
    assert!(json.contains("\"rssi\":-65"));

    let notif = BleEvent::Notification {
        id: "dev1".into(),
        characteristic: "00002a37-0000-1000-8000-00805f9b34fb".into(),
        data: vec![0, 75],
    };
    let json_notif = serde_json::to_string(&notif).unwrap();
    assert!(json_notif.contains("\"type\":\"Notification\""));
    assert!(json_notif.contains("\"data\":[0,75]"));
}

#[test]
fn test_gatt_profile_serialization() {
    let profile = vec![GattServiceInfo {
        uuid: "0000180d-0000-1000-8000-00805f9b34fb".into(),
        primary: true,
        characteristics: vec![GattCharInfo {
            uuid: "00002a37-0000-1000-8000-00805f9b34fb".into(),
            can_read: false,
            can_write: false,
            can_write_without_response: false,
            can_notify: true,
            can_indicate: false,
            descriptors: vec!["00002902-0000-1000-8000-00805f9b34fb".into()],
        }],
    }];

    let json = serde_json::to_string(&profile).unwrap();
    assert!(json.contains("\"can_notify\":true"));
    assert!(json.contains("00002902-0000-1000-8000-00805f9b34fb"));
}

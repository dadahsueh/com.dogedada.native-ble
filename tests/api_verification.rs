use btleplug::api::{Characteristic, Service, ValueNotification};
use uuid::Uuid;

#[test]
fn test_service_and_char_types() {
    let s = Service {
        uuid: Uuid::from_bytes([0; 16]),
        primary: true,
        characteristics: std::collections::BTreeSet::new(),
    };
    assert!(s.primary);

    let c = Characteristic {
        uuid: Uuid::from_bytes([0; 16]),
        service_uuid: Uuid::from_bytes([0; 16]),
        properties: btleplug::api::CharPropFlags::READ | btleplug::api::CharPropFlags::NOTIFY,
        descriptors: std::collections::BTreeSet::new(),
    };
    assert!(c.properties.contains(btleplug::api::CharPropFlags::READ));

    let vn = ValueNotification {
        uuid: Uuid::from_bytes([0; 16]),
        value: vec![1, 2, 3],
    };
    assert_eq!(vn.value.len(), 3);
}

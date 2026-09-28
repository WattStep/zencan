//! The generated EDS parses with zencan-eds and describes the node the device config builds

use std::str::FromStr;

use zencan_build::{device_config::DeviceConfig, device_config_to_eds};
use zencan_eds::{DataTypeCode, ElectronicDataSheet, Object};

const DEVCFG: &str = r#"
    device_name = "test drive"
    device_type = 0x20192
    vendor_name = "Test Vendor"
    bit_rates = [250, 500]
    heartbeat_period = 500

    [identity]
    vendor_id = 0xCAFE
    product_code = 7
    revision_number = 3

    [pdos]
    num_rpdo = 2
    num_tpdo = 1

    [pdos.rpdo.0]
    enabled = true
    cob_id = 0x200
    add_node_id = true
    transmission_type = 254
    mappings = [
        { index = 0x6040, sub = 0, size = 16 },
        { index = 0x2001, sub = 2, size = 32 },
    ]

    [pdos.tpdo.0]
    enabled = true
    cob_id = 0x180
    add_node_id = true
    transmission_type = 254
    mappings = [{ index = 0x6041, sub = 0, size = 16 }]

    [[objects]]
    index = 0x6040
    parameter_name = "Controlword"
    object_type = "var"
    data_type = "uint16"
    access_type = "rw"
    pdo_mapping = "rpdo"
    default_value = 0

    [[objects]]
    index = 0x6041
    parameter_name = "Statusword"
    object_type = "var"
    data_type = "uint16"
    access_type = "ro"
    pdo_mapping = "tpdo"

    [[objects]]
    index = 0x607A
    parameter_name = "Target position"
    object_type = "var"
    data_type = "int32"
    access_type = "rw"
    pdo_mapping = "both"
    default_value = -5

    [[objects]]
    index = 0x2000
    parameter_name = "Limits"
    object_type = "array"
    data_type = "int16"
    access_type = "rw"
    array_size = 2
    default_value = [10, -10]

    [[objects]]
    index = 0x2001
    parameter_name = "Setpoints"
    object_type = "record"
    [[objects.subs]]
    sub_index = 1
    parameter_name = "Gain"
    data_type = "real32"
    access_type = "rw"
    default_value = 1.5
    [[objects.subs]]
    sub_index = 2
    parameter_name = "Offset"
    data_type = "uint32"
    access_type = "rw"
    pdo_mapping = "rpdo"
"#;

fn eds() -> ElectronicDataSheet {
    let config = DeviceConfig::load_from_str(DEVCFG).expect("device config");
    let text = device_config_to_eds(&config, "test.eds").expect("EDS generation");
    ElectronicDataSheet::from_str(&text).expect("the EDS should parse")
}

fn find(eds: &ElectronicDataSheet, index: u16) -> &Object {
    eds.mandatory_objects
        .iter()
        .chain(&eds.optional_objects)
        .chain(&eds.manufacturer_objects)
        .find(|o| o.object_number == index)
        .unwrap_or_else(|| panic!("object 0x{index:04X} missing"))
}

fn default(eds: &ElectronicDataSheet, index: u16, sub: u8) -> String {
    find(eds, index).subs[&sub]
        .default_value
        .clone()
        .unwrap_or_else(|| panic!("0x{index:04X}sub{sub} has no default"))
}

#[test]
fn device_info_comes_from_the_config() {
    let eds = eds();
    let info = &eds.device_info;
    assert_eq!(info.vendor_name, "Test Vendor");
    assert_eq!(info.product_name, "test drive");
    assert_eq!(info.vendor_number, Some(0xCAFE));
    assert_eq!(info.product_number, Some(7));
    assert_eq!(info.revision_number, Some(3));
    assert!(info.baudrate_250 && info.baudrate_500);
    assert!(!info.baudrate_125 && !info.baudrate_1000);
    assert_eq!((info.rpdo_count, info.tpdo_count), (2, 1));
    assert_eq!(eds.file_info.file_name, "test.eds");
}

#[test]
fn objects_are_sorted_into_their_sections() {
    let eds = eds();
    let indices = |objects: &[Object]| objects.iter().map(|o| o.object_number).collect::<Vec<_>>();
    assert_eq!(indices(&eds.mandatory_objects), [0x1000, 0x1001, 0x1018]);
    // 0x5000 is zencan's auto-start object
    assert_eq!(indices(&eds.manufacturer_objects), [0x2000, 0x2001, 0x5000]);
    let optional = indices(&eds.optional_objects);
    for index in [
        0x1008, 0x1017, 0x1400, 0x1401, 0x1600, 0x1800, 0x1A00, 0x6040, 0x607A,
    ] {
        assert!(
            optional.contains(&index),
            "0x{index:04X} not in OptionalObjects"
        );
    }
    assert!(!optional.contains(&0x1402) && !optional.contains(&0x1801));
}

#[test]
fn system_objects_carry_their_config_values() {
    let eds = eds();
    assert_eq!(default(&eds, 0x1000, 0), "0x20192");
    assert_eq!(default(&eds, 0x1008, 0), "test drive");
    assert_eq!(default(&eds, 0x1017, 0), "0x1F4");
    assert_eq!(default(&eds, 0x1018, 0), "0x4");
    assert_eq!(default(&eds, 0x1018, 1), "0xCAFE");
}

#[test]
fn pdo_defaults_are_declared() {
    let eds = eds();
    assert_eq!(default(&eds, 0x1400, 1), "$NODEID+0x200");
    assert_eq!(default(&eds, 0x1400, 2), "0xFE");
    assert_eq!(default(&eds, 0x1600, 0), "0x2");
    assert_eq!(default(&eds, 0x1600, 1), "0x60400010");
    assert_eq!(default(&eds, 0x1600, 2), "0x20010220");
    assert_eq!(default(&eds, 0x1600, 3), "0x00000000");
    assert_eq!(find(&eds, 0x1600).subs.len(), 9);
    // An RPDO with no defaults starts invalid and unmapped
    assert_eq!(default(&eds, 0x1401, 1), "0x80000000");
    assert_eq!(default(&eds, 0x1601, 0), "0x0");
    assert_eq!(default(&eds, 0x1800, 1), "$NODEID+0x180");
    assert_eq!(default(&eds, 0x1A00, 1), "0x60410010");
}

#[test]
fn application_objects_keep_type_access_mapping_and_defaults() {
    let eds = eds();
    let controlword = &find(&eds, 0x6040).subs[&0];
    assert_eq!(controlword.data_type, DataTypeCode::UInt16);
    assert_eq!(controlword.pdo_mapping, Some(true));
    assert_eq!(controlword.default_value.as_deref(), Some("0x0"));

    let statusword = &find(&eds, 0x6041).subs[&0];
    assert_eq!(statusword.pdo_mapping, Some(true));
    assert_eq!(statusword.default_value, None);

    assert_eq!(default(&eds, 0x607A, 0), "-5");
    assert_eq!(find(&eds, 0x607A).subs[&0].data_type, DataTypeCode::Int32);

    assert_eq!(default(&eds, 0x2000, 0), "0x2");
    assert_eq!(default(&eds, 0x2000, 2), "-10");
    assert_eq!(default(&eds, 0x2001, 0), "0x2");
    assert_eq!(default(&eds, 0x2001, 1), "1.5");
    assert_eq!(find(&eds, 0x2001).subs[&1].pdo_mapping, Some(false));
}

#[test]
fn single_direction_rw_objects_say_which() {
    let config = DeviceConfig::load_from_str(DEVCFG).unwrap();
    let text = device_config_to_eds(&config, "test.eds").unwrap();
    let access = |section: &str| {
        let start = text.find(&format!("[{section}]\n")).unwrap();
        let line = text[start..]
            .lines()
            .find(|l| l.starts_with("AccessType="))
            .unwrap();
        line.trim_start_matches("AccessType=").to_string()
    };
    assert_eq!(access("6040"), "rww");
    assert_eq!(access("607A"), "rw");
    assert_eq!(access("6041"), "ro");
    assert_eq!(access("1017"), "const");
}

#[test]
fn output_is_deterministic() {
    let config = DeviceConfig::load_from_str(DEVCFG).unwrap();
    assert_eq!(
        device_config_to_eds(&config, "a.eds").unwrap(),
        device_config_to_eds(&config, "a.eds").unwrap()
    );
}

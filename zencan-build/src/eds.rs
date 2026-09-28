//! Generate an electronic data sheet (EDS, CiA 306) from a device config
//!
//! The EDS is built from the same elaborated object list as the generated object dictionary, so
//! it lists exactly the objects the node has, with their defaults, including the system objects
//! zencan adds and the default PDO configuration.
//!
//! The output is deterministic: it carries no creation or modification dates, so a checked-in EDS
//! only changes when the device config does.

use std::fmt::Write as _;

use zencan_common::object_model::{AccessType, DataType, DefaultValue, PdoMappable, SubObjectSpec};

use crate::{
    device_config::{DeviceConfig, PdoDefaultConfig},
    errors::CompileError,
    object_build_spec::{
        ObjectBuildShape, ObjectBuildSpec, ObjectImplementation, SystemObjectKind,
    },
};

/// Mapping entries per PDO in zencan-node (`zencan_node::pdo::N_MAPPING_PARAMS`)
const PDO_MAPPING_ENTRIES: u8 = 8;

/// The bit rates an EDS can declare, in kbit/s
const EDS_BIT_RATES: [u16; 8] = [10, 20, 50, 125, 250, 500, 800, 1000];

/// Generate the EDS for a device config
///
/// `file_name` is written to the `FileName` entry of `[FileInfo]`.
pub fn device_config_to_eds(
    config: &DeviceConfig,
    file_name: &str,
) -> Result<String, CompileError> {
    let mut specs = config.elaborate()?;
    specs.sort_by_key(|s| s.index);

    let mut eds = String::new();
    write_file_info(&mut eds, config, file_name);
    write_device_info(&mut eds, config);
    write_fixed_sections(&mut eds);

    let (mandatory, rest): (Vec<_>, Vec<_>) = specs
        .iter()
        .partition(|s| matches!(s.index, 0x1000 | 0x1001 | 0x1018));
    let (manufacturer, optional): (Vec<_>, Vec<_>) = rest
        .into_iter()
        .partition(|s| (0x2000..=0x5FFF).contains(&s.index));

    for (name, list) in [
        ("MandatoryObjects", &mandatory),
        ("OptionalObjects", &optional),
        ("ManufacturerObjects", &manufacturer),
    ] {
        section(&mut eds, name);
        entry(&mut eds, "SupportedObjects", list.len());
        for (i, spec) in list.iter().enumerate() {
            entry(
                &mut eds,
                &(i + 1).to_string(),
                format!("0x{:04X}", spec.index),
            );
        }
        for spec in list {
            write_object(&mut eds, config, spec);
        }
    }

    Ok(eds)
}

/// Generate the EDS for a device config file and write it to `out_path`
///
/// The `FileName` entry is taken from `out_path`.
pub fn generate_eds(
    config_path: impl AsRef<std::path::Path>,
    out_path: impl AsRef<std::path::Path>,
) -> Result<(), CompileError> {
    use snafu::ResultExt as _;

    let config =
        DeviceConfig::load(config_path.as_ref()).context(crate::errors::DeviceConfigSnafu)?;
    let file_name = out_path
        .as_ref()
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let eds = device_config_to_eds(&config, &file_name)?;
    std::fs::write(out_path.as_ref(), eds).context(crate::errors::IoSnafu)?;
    Ok(())
}

fn section(eds: &mut String, name: &str) {
    if !eds.is_empty() {
        eds.push('\n');
    }
    let _ = writeln!(eds, "[{name}]");
}

fn entry(eds: &mut String, key: &str, value: impl std::fmt::Display) {
    let _ = writeln!(eds, "{key}={value}");
}

fn write_file_info(eds: &mut String, config: &DeviceConfig, file_name: &str) {
    section(eds, "FileInfo");
    entry(eds, "FileName", file_name);
    entry(eds, "FileVersion", 1);
    entry(eds, "FileRevision", 0);
    entry(eds, "EDSVersion", "4.0");
    entry(eds, "Description", &config.device_name);
    for key in [
        "CreationTime",
        "CreationDate",
        "CreatedBy",
        "ModificationTime",
        "ModificationDate",
        "ModifiedBy",
    ] {
        entry(eds, key, "");
    }
}

fn write_device_info(eds: &mut String, config: &DeviceConfig) {
    section(eds, "DeviceInfo");
    entry(eds, "VendorName", &config.vendor_name);
    entry(
        eds,
        "VendorNumber",
        format!("0x{:08X}", config.identity.vendor_id),
    );
    entry(eds, "ProductName", &config.device_name);
    entry(
        eds,
        "ProductNumber",
        format!("0x{:08X}", config.identity.product_code),
    );
    entry(
        eds,
        "RevisionNumber",
        format!("0x{:08X}", config.identity.revision_number),
    );
    entry(eds, "OrderCode", "");
    for rate in EDS_BIT_RATES {
        entry(
            eds,
            &format!("BaudRate_{rate}"),
            u8::from(config.bit_rates.contains(&rate)),
        );
    }
    entry(eds, "SimpleBootUpMaster", 0);
    // The node sends its boot-up message and is an NMT slave
    entry(eds, "SimpleBootUpSlave", 1);
    // PDO mapping is by whole bytes
    entry(eds, "Granularity", 8);
    entry(eds, "DynamicChannelsSupported", 0);
    entry(eds, "CompactPDO", 0);
    entry(eds, "GroupMessaging", 0);
    entry(eds, "NrOfRXPDO", config.pdos.num_rpdo);
    entry(eds, "NrOfTXPDO", config.pdos.num_tpdo);
    entry(eds, "LSS_Supported", 1);
}

fn write_fixed_sections(eds: &mut String) {
    // Dummy mapping (mapping a data type index to pad a PDO) is not supported
    section(eds, "DummyUsage");
    for code in 1..=7 {
        entry(eds, &format!("Dummy{code:04X}"), 0);
    }
    section(eds, "Comments");
    entry(eds, "Lines", 0);
}

fn write_object(eds: &mut String, config: &DeviceConfig, spec: &ObjectBuildSpec) {
    if spec.implementation == ObjectImplementation::System(SystemObjectKind::Pdo) {
        write_pdo_object(eds, config, spec);
        return;
    }
    let name = format!("{:04X}", spec.index);
    match &spec.shape {
        ObjectBuildShape::Var(var) => {
            section(eds, &name);
            entry(eds, "ParameterName", &spec.parameter_name);
            entry(eds, "ObjectType", "0x7");
            write_sub_fields(eds, &var.value);
        }
        ObjectBuildShape::Array(array) => {
            write_container_header(
                eds,
                &name,
                &spec.parameter_name,
                0x8,
                array.elements().len() + 1,
            );
            write_sub(eds, &name, 0, array.sub0_spec());
            for (i, element) in array.elements().iter().enumerate() {
                write_sub(eds, &name, i + 1, element);
            }
        }
        ObjectBuildShape::Record(record) => {
            let max_sub = record.subs.keys().max().copied().unwrap_or(0);
            write_container_header(eds, &name, &spec.parameter_name, 0x9, record.subs.len() + 1);
            write_sub(eds, &name, 0, &sub0(max_sub, AccessType::Const));
            for (sub, build_spec) in &record.subs {
                write_sub(eds, &name, *sub as usize, &build_spec.spec);
            }
        }
    }
}

/// The PDO communication and mapping objects, with the device config's defaults
///
/// These are written from the PDO config rather than the elaborated spec, which leaves their
/// defaults to the node at runtime and lists more mapping entries than a PDO holds.
fn write_pdo_object(eds: &mut String, config: &DeviceConfig, spec: &ObjectBuildSpec) {
    let (base, transmit) = match spec.index {
        0x1400..=0x15FF => (0x1400, false),
        0x1600..=0x17FF => (0x1600, false),
        0x1800..=0x19FF => (0x1800, true),
        _ => (0x1A00, true),
    };
    let slot = (spec.index - base) as usize;
    let defaults = if transmit {
        config.pdos.tpdo_defaults.get(&slot)
    } else {
        config.pdos.rpdo_defaults.get(&slot)
    };
    let name = format!("{:04X}", spec.index);
    let u32_rw = |parameter_name: String, default: String| SubObjectSpec {
        parameter_name,
        data_type: DataType::UInt32,
        access_type: AccessType::Rw,
        default_value: Some(DefaultValue::String(default)),
        pdo_mapping: PdoMappable::None,
        persist: true,
    };

    if base == 0x1400 || base == 0x1800 {
        write_container_header(eds, &name, &spec.parameter_name, 0x9, 3);
        write_sub(eds, &name, 0, &sub0(2, AccessType::Const));
        write_sub(
            eds,
            &name,
            1,
            &u32_rw("COB-ID".into(), cob_id_default(defaults)),
        );
        write_sub(
            eds,
            &name,
            2,
            &SubObjectSpec {
                parameter_name: "Transmission Type".into(),
                data_type: DataType::UInt8,
                access_type: AccessType::Rw,
                default_value: Some(DefaultValue::Integer(
                    defaults.map_or(0, |d| d.transmission_type).into(),
                )),
                pdo_mapping: PdoMappable::None,
                persist: true,
            },
        );
    } else {
        let mappings = defaults.map_or(&[][..], |d| &d.mappings[..]);
        write_container_header(
            eds,
            &name,
            &spec.parameter_name,
            0x9,
            PDO_MAPPING_ENTRIES as usize + 1,
        );
        let mut count = sub0(mappings.len() as u8, AccessType::Rw);
        count.parameter_name = "Number of Mapped Objects".into();
        write_sub(eds, &name, 0, &count);
        for i in 0..PDO_MAPPING_ENTRIES as usize {
            let value = mappings.get(i).map_or(0, |m| m.to_object_value());
            write_sub(
                eds,
                &name,
                i + 1,
                &u32_rw(format!("Mapped Object {}", i + 1), format!("0x{value:08X}")),
            );
        }
    }
}

/// The COB-ID a PDO starts with, including the invalid, no-RTR and extended flags
fn cob_id_default(defaults: Option<&PdoDefaultConfig>) -> String {
    let Some(d) = defaults else {
        // zencan's defaults for an unconfigured PDO: COB-ID 0, not valid
        return "0x80000000".into();
    };
    let value = (u32::from(!d.enabled) << 31)
        | (u32::from(d.rtr_disabled) << 30)
        | (u32::from(d.extended) << 29)
        | d.cob_id;
    if d.add_node_id {
        format!("$NODEID+0x{value:X}")
    } else {
        format!("0x{value:X}")
    }
}

fn sub0(max_sub: u8, access_type: AccessType) -> SubObjectSpec {
    SubObjectSpec {
        parameter_name: "Highest Sub-index Supported".into(),
        data_type: DataType::UInt8,
        access_type,
        default_value: Some(DefaultValue::Integer(max_sub.into())),
        pdo_mapping: PdoMappable::None,
        persist: false,
    }
}

fn write_container_header(
    eds: &mut String,
    name: &str,
    parameter_name: &str,
    object_type: u8,
    sub_number: usize,
) {
    section(eds, name);
    entry(eds, "ParameterName", parameter_name);
    entry(eds, "ObjectType", format!("0x{object_type:X}"));
    entry(eds, "SubNumber", format!("0x{sub_number:X}"));
}

fn write_sub(eds: &mut String, object: &str, sub: usize, spec: &SubObjectSpec) {
    section(eds, &format!("{object}sub{sub:X}"));
    entry(eds, "ParameterName", &spec.parameter_name);
    entry(eds, "ObjectType", "0x7");
    write_sub_fields(eds, spec);
}

fn write_sub_fields(eds: &mut String, spec: &SubObjectSpec) {
    entry(
        eds,
        "DataType",
        format!("0x{:04X}", data_type_code(spec.data_type)),
    );
    entry(eds, "AccessType", access_type(spec));
    if let Some(default) = &spec.default_value {
        entry(eds, "DefaultValue", default_value(spec.data_type, default));
    }
    entry(
        eds,
        "PDOMapping",
        u8::from(spec.pdo_mapping != PdoMappable::None),
    );
}

/// The EDS access type
///
/// A read-write object that maps into only one PDO direction says which with `rww` (written by
/// an RPDO) or `rwr` (read by a TPDO), so a configuration tool offers it on the right side.
fn access_type(spec: &SubObjectSpec) -> &'static str {
    match (spec.access_type, spec.pdo_mapping) {
        (AccessType::Rw, PdoMappable::Rpdo) => "rww",
        (AccessType::Rw, PdoMappable::Tpdo) => "rwr",
        (AccessType::Rw, _) => "rw",
        (AccessType::Ro, _) => "ro",
        (AccessType::Wo, _) => "wo",
        (AccessType::Const, _) => "const",
    }
}

fn default_value(data_type: DataType, value: &DefaultValue) -> String {
    match value {
        DefaultValue::Integer(i) if is_unsigned(data_type) => format!("0x{i:X}"),
        DefaultValue::Integer(i) => i.to_string(),
        DefaultValue::Float(f) => f.to_string(),
        DefaultValue::String(s) => s.clone(),
    }
}

fn is_unsigned(data_type: DataType) -> bool {
    matches!(
        data_type,
        DataType::Boolean
            | DataType::UInt8
            | DataType::UInt16
            | DataType::UInt24
            | DataType::UInt32
            | DataType::UInt64
    )
}

/// The CiA 301 data type index
fn data_type_code(data_type: DataType) -> u16 {
    match data_type {
        DataType::Boolean => 0x01,
        DataType::Int8 => 0x02,
        DataType::Int16 => 0x03,
        DataType::Int32 => 0x04,
        DataType::UInt8 => 0x05,
        DataType::UInt16 => 0x06,
        DataType::UInt32 => 0x07,
        DataType::Real32 => 0x08,
        DataType::VisibleString(_) => 0x09,
        DataType::OctetString(_) => 0x0A,
        DataType::UnicodeString(_) => 0x0B,
        DataType::TimeOfDay => 0x0C,
        DataType::TimeDifference => 0x0D,
        DataType::Domain => 0x0F,
        DataType::Int24 => 0x10,
        DataType::Real64 => 0x11,
        DataType::Int64 => 0x15,
        DataType::UInt24 => 0x16,
        DataType::UInt64 => 0x1B,
    }
}

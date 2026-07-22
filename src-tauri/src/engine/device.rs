// P1.3: device enumeration. cpal devices have no stable id, so we use the
// (usually unique) device name as the id and match by name when selecting.

use cpal::traits::{DeviceTrait, HostTrait};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub channels: u16,
    pub default_sample_rate: u32,
}

pub fn list_input_devices() -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    host.input_devices()
        .map(|iter| iter.filter_map(dev_info_in).collect())
        .unwrap_or_default()
}

pub fn list_output_devices() -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    host.output_devices()
        .map(|iter| iter.filter_map(dev_info_out).collect())
        .unwrap_or_default()
}

fn dev_info_in(d: cpal::Device) -> Option<DeviceInfo> {
    let name = d.name().ok()?;
    let cfg = d.default_input_config().ok()?;
    Some(DeviceInfo {
        id: name.clone(),
        name,
        channels: cfg.channels(),
        default_sample_rate: cfg.sample_rate().0,
    })
}

fn dev_info_out(d: cpal::Device) -> Option<DeviceInfo> {
    let name = d.name().ok()?;
    let cfg = d.default_output_config().ok()?;
    Some(DeviceInfo {
        id: name.clone(),
        name,
        channels: cfg.channels(),
        default_sample_rate: cfg.sample_rate().0,
    })
}

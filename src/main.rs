use anyhow::{anyhow, Context};
use evdev::{
    raw_stream::{self, RawDevice},
    uinput::{VirtualDevice, VirtualDeviceBuilder},
    AttributeSet, InputEvent, InputEventKind, Key, RelativeAxisType,
};
use serde::Deserialize;
use serde_yaml;
use std::{ops::RangeBounds, str::FromStr};

#[derive(Debug, PartialEq, Deserialize, Copy, Clone)]
enum Mode {
    Mouse,
    Manipulator,
}

#[derive(Debug, PartialEq, Deserialize)]
struct Config {
    target_name: String,
    virtual_manipulator_prefix: String,
    virtual_mouse_prefix: String,
    virtual_mouse_keys: Vec<String>,
    virtual_mouse_axes: Vec<String>,
    toggle_sequence: Vec<(String, u8)>,
    default_mode: Mode,
}

struct VirtualDeviceConfig {
    name: String,
    keys: AttributeSet<Key>,
    axes: AttributeSet<RelativeAxisType>,
}

impl VirtualDeviceConfig {
    fn new(name: String, keys: &Vec<String>, axes: &Vec<String>) -> Result<Self, anyhow::Error> {
        let keys = VirtualDeviceConfig::prepare_keys(keys)?;
        let axes = VirtualDeviceConfig::prepare_axes(axes)?;

        Ok(Self { name, keys, axes })
    }

    // TODO: open an issue on evdev_rs repo::
    // evdev::attribute_set::ArrayedEvdevEnum is private so I can't use it in
    // a trait bound and make these functions generic
    fn prepare_keys(list: &Vec<String>) -> Result<AttributeSet<Key>, anyhow::Error> {
        let mut result = AttributeSet::<Key>::new();
        for item in list.iter() {
            if let Ok(converted) = Key::from_str(item) {
                result.insert(converted);
            } else {
                return Err(anyhow!("event \"{}\" doesn't exist", item));
            }
        }
        Ok(result)
    }
    fn prepare_axes(list: &Vec<String>) -> Result<AttributeSet<RelativeAxisType>, anyhow::Error> {
        let mut result = AttributeSet::<RelativeAxisType>::new();
        for item in list.iter() {
            if let Ok(converted) = RelativeAxisType::from_str(item) {
                result.insert(converted);
            } else {
                return Err(anyhow!("event \"{}\" doesn't exist", item));
            }
        }
        Ok(result)
    }
}

struct VirtualDeviceWrapper {
    device: VirtualDevice,
    config: VirtualDeviceConfig,
}

fn find_device(target_name: &str) -> Result<raw_stream::RawDevice, anyhow::Error> {
    for device in raw_stream::enumerate() {
        if let Some(name) = device.name() {
            if name == target_name {
                return Ok(device);
            }
        }
    }
    Err(anyhow!("failed to find device \"{}\"", target_name))
}

fn prefix_device_name<'a>(prefix: &'a str, name: &'a str) -> String {
    format!("{} {}", prefix, name)
}

fn create_virtual_device(
    device_config: &VirtualDeviceConfig,
) -> Result<VirtualDevice, anyhow::Error> {
    let VirtualDeviceConfig { name, keys, axes } = device_config;

    let device = VirtualDeviceBuilder::new()?
        .name(&name)
        .with_keys(&keys)?
        .with_relative_axes(&axes)?
        .build()?;
    Ok(device)
}

fn setup(
    config: &Config,
) -> Result<(RawDevice, VirtualDeviceWrapper, VirtualDeviceWrapper), anyhow::Error> {
    let mut target_device = find_device(&config.target_name)?;
    let target_device_name = String::from(target_device.name().unwrap());
    target_device.grab().unwrap();

    let mut target_device_keys = AttributeSet::<Key>::new();
    for key in target_device.supported_keys().unwrap().iter() {
        target_device_keys.insert(key);
    }
    let mut target_device_axes = AttributeSet::<RelativeAxisType>::new();
    for axis in target_device.supported_relative_axes().unwrap().iter() {
        target_device_axes.insert(axis);
    }
    let virtual_manipulator_config = VirtualDeviceConfig {
        name: prefix_device_name(&config.virtual_manipulator_prefix, &target_device_name),
        keys: target_device_keys,
        axes: target_device_axes,
    };
    let virtual_manipulator_device = create_virtual_device(&virtual_manipulator_config)?;

    let virtual_mouse_config = VirtualDeviceConfig::new(
        prefix_device_name(&config.virtual_mouse_prefix, &target_device_name),
        &config.virtual_mouse_keys,
        &config.virtual_mouse_axes,
    )?;
    let virtual_mouse_device = create_virtual_device(&virtual_mouse_config)?;

    Ok((
        target_device,
        VirtualDeviceWrapper {
            device: virtual_manipulator_device,
            config: virtual_manipulator_config,
        },
        VirtualDeviceWrapper {
            device: virtual_mouse_device,
            config: virtual_mouse_config,
        },
    ))
}

fn should_emit(device: &VirtualDeviceWrapper, event: &InputEvent, mode: Mode) -> bool {
    if mode == Mode::Manipulator {
        return true;
    }
    match event.kind() {
        InputEventKind::Key(key) => device.config.keys.contains(key),
        InputEventKind::RelAxis(axis) => device.config.axes.contains(axis),
        _ => true,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config: Config =
        serde_yaml::from_str(include_str!("../config.yml")).context("config.yml is malformed")?;
    let device = find_device(&config.target_name)?;
    println!("{:#?}", device);

    let (mut target_device, mut virtual_manipulator_device, mut virtual_mouse_device) =
        setup(&config).context("failed to create virtual devices")?;

    let mut mode = config.default_mode;

    loop {
        let output_device = match mode {
            Mode::Manipulator => &virtual_manipulator_device,
            Mode::Mouse => &virtual_mouse_device,
        };
        let events = target_device
            .fetch_events()
            .context("failed to fetch events")?;
        for event in events {
            if should_emit(output_device, &event, mode) {
                println!("{:#?}", event);
            }
        }
    }
}

use anyhow::{anyhow, Context};
use evdev::{
    raw_stream::{self, RawDevice},
    uinput::{VirtualDevice, VirtualDeviceBuilder},
    AttributeSet, InputEvent, InputEventKind, Key, RelativeAxisType,
};
use serde::Deserialize;
use std::collections::VecDeque;
use std::str::FromStr;

#[derive(Debug, PartialEq)]
struct Keystroke {
    key: u16,
    value: i32,
}

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
    toggle_sequence: Vec<(String, u16)>,
    default_mode: Mode,
}

impl Config {
    fn toggle_sequence_to_keystrokes(&self) -> Vec<Keystroke> {
        self.toggle_sequence
            .iter()
            .map(|(k, v)| Keystroke {
                key: Key::from_str(k).unwrap().0,
                value: *v as i32,
            })
            .collect()
    }
}

struct VirtualDeviceConfig {
    name: String,
    keys: AttributeSet<Key>,
    axes: AttributeSet<RelativeAxisType>,
}

impl VirtualDeviceConfig {
    fn new(name: String, keys: &[String], axes: &[String]) -> Result<Self, anyhow::Error> {
        let keys = VirtualDeviceConfig::prepare_keys(keys)?;
        let axes = VirtualDeviceConfig::prepare_axes(axes)?;

        Ok(Self { name, keys, axes })
    }

    // TODO: open an issue on evdev_rs repo::
    // evdev::attribute_set::ArrayedEvdevEnum is private so I can't use it in
    // a trait bound and make these functions generic
    fn prepare_keys(list: &[String]) -> Result<AttributeSet<Key>, anyhow::Error> {
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
    fn prepare_axes(list: &[String]) -> Result<AttributeSet<RelativeAxisType>, anyhow::Error> {
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

// We need this because evdev library has no trait From<Key>/trait From<RelativeAxisType> for String,
// so we have to store AttributeSet representation of it along with the  device.
// Another reason to do that is because for some reason virtual devices
// don't provide device.supported_*() methods.
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
        .with_keys(keys)?
        .with_relative_axes(axes)?
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

fn should_emit(device: &VirtualDeviceWrapper, event: &InputEvent, mode: &Mode) -> bool {
    if *mode == Mode::Manipulator {
        return true;
    }
    match event.kind() {
        InputEventKind::Key(key) => device.config.keys.contains(key),
        InputEventKind::RelAxis(axis) => device.config.axes.contains(axis),
        _ => true,
    }
}

fn should_toggle(history: &VecDeque<Keystroke>, sequence: &[Keystroke]) -> bool {
    if history.len() != sequence.len() {
        return false;
    }
    for (a, b) in history.iter().zip(sequence.iter()) {
        if a != b {
            return false;
        }
    }
    true
}

fn save_stroke(history: &mut VecDeque<Keystroke>, event: &InputEvent, max_len: usize) -> bool {
    if history.len() == max_len {
        // we don't really need that value and just want to get a free slot
        let _ = history.pop_front();
    }
    match event.kind() {
        InputEventKind::Key(_) => {
            history.push_back(Keystroke {
                key: event.code(),
                value: event.value(),
            });
            true
        }
        _ => false,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config: Config =
        serde_yaml::from_str(include_str!("../config.yml")).context("config.yml is malformed")?;
    let device = find_device(&config.target_name)?;
    println!("target device configuration: {:#?}", device);

    let (mut target_device, mut virtual_manipulator_device, mut virtual_mouse_device) =
        setup(&config).context("failed to create virtual devices")?;

    let mut mode = config.default_mode;

    let toggle_sequence = config.toggle_sequence_to_keystrokes();
    let history_max_len: usize = toggle_sequence.len();
    let mut history: VecDeque<Keystroke> = VecDeque::new();

    //    let history = ringbuf::RingBuffer::<Keystroke>::new(toggle_sequence.len());
    //    let (mut history_producer, mut history_consumer) = history.split();

    loop {
        let output_device = match mode {
            Mode::Manipulator => &mut virtual_manipulator_device,
            Mode::Mouse => &mut virtual_mouse_device,
        };
        let events = target_device
            .fetch_events()
            .context("failed to fetch events")?;
        for event in events {
            let stroke_saved = save_stroke(&mut history, &event, history_max_len);
            if should_emit(output_device, &event, &mode) {
                println!("emitting event: {:#?}", event);
                let _ = output_device
                    .device
                    .emit(&[event])
                    .context("failed to emit an event")?;
            }
            if stroke_saved && should_toggle(&history, &toggle_sequence) {
                mode = match mode {
                    Mode::Mouse => {
                        println!("mouse mode is switching to manipulator");
                        Mode::Manipulator
                    }
                    Mode::Manipulator => {
                        println!("manipulator mode is switching to mouse");
                        Mode::Mouse
                    }
                };
            }
        }
    }
}

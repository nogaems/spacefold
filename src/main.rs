use evdev::{
    raw_stream,
    uinput::{VirtualDevice, VirtualDeviceBuilder},
    AttributeSet, AttributeSetRef, EventType, InputEvent, Key, RelativeAxisType,
};

use std::str::FromStr;
use std::{io, vec};

struct VirtualDeviceConfig {
    name: String,
    keys: AttributeSet<Key>,
    axes: AttributeSet<RelativeAxisType>,
}

impl VirtualDeviceConfig {
    fn new<K: Iterator<Item = Key>, A: Iterator<Item = RelativeAxisType>>(
        name: &str,
        keys: K,
        axes: A,
    ) -> Self {
        let name = String::from_str(name).unwrap();
        let keys = keys.into_iter();
        let mut _keys = AttributeSet::<Key>::new();
        for k in keys {
            _keys.insert(k);
        }
        let axes = axes.into_iter();
        let mut _axes = AttributeSet::<RelativeAxisType>::new();
        for a in axes {
            _axes.insert(a);
        }

        Self {
            name,
            keys: _keys,
            axes: _axes,
        }
    }
}

struct Config {
    target_device_name: String,
    virtual_mouse_keys: Vec<Key>,
    virtual_mouse_axes: Vec<RelativeAxisType>,
}

fn find_device(target_name: &str) -> Option<raw_stream::RawDevice> {
    for device in raw_stream::enumerate() {
        if let Some(name) = device.name() {
            if name == target_name {
                return Some(device);
            }
        }
    }
    None
}

fn prefix_device_name<'a>(prefix: &'a str, name: &'a str) -> String {
    format!("{} {}", prefix, name)
}

fn create_virtual_device(config: &VirtualDeviceConfig) -> Result<VirtualDevice, io::Error> {
    let VirtualDeviceConfig { name, keys, axes } = config;

    println!("{:#?}", &name);
    println!("{:#?}", &keys);
    println!("{:#?}", &axes);

    let device = VirtualDeviceBuilder::new()?
        .name(&name)
        .with_keys(&keys)?
        .with_relative_axes(&axes)?
        .build()?;
    println!("created");
    Ok(device)
}

fn setup() -> Option<(VirtualDevice, VirtualDevice)> {
    // TODO: these values should be filled up dynamically
    let config = Config {
        target_device_name: String::from_str("USB OPTICAL MOUSE ").unwrap(),
        virtual_mouse_keys: vec![Key::BTN_LEFT],
        virtual_mouse_axes: vec![RelativeAxisType::REL_X],
    };

    let target_device = find_device(&config.target_device_name)?;

    let virtual_manipulator_config = VirtualDeviceConfig::new(
        &prefix_device_name("Virtual Manipulator", target_device.name()?),
        (*target_device.supported_keys()?).iter(),
        (*target_device.supported_relative_axes()?).iter(),
    );
    let virtual_manipulator_device = create_virtual_device(&virtual_manipulator_config).unwrap();

    let virtual_mouse_config = VirtualDeviceConfig::new(
        &prefix_device_name("Virtual Mouse", target_device.name()?),
        config.virtual_mouse_keys.into_iter(),
        config.virtual_mouse_axes.into_iter(),
    );
    let virtual_mouse_device = create_virtual_device(&virtual_mouse_config).unwrap();
    Some((virtual_manipulator_device, virtual_mouse_device))
}

fn main() {
    let (mut manipulator, mut mouse) = setup().unwrap();
    let type_ = EventType::KEY;
    let code = Key::BTN_LEFT.code();
    let event0 = InputEvent::new(type_, code, 0);
    let event1 = InputEvent::new(type_, code, 1);
    let events = vec![
        event1.clone(),
        event0.clone(),
        event1.clone(),
        event0.clone(),
    ];
    mouse.emit(&events).unwrap();

    let mut buffer = String::new();
    io::stdin().read_line(&mut buffer).unwrap();
}

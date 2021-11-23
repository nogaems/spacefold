use evdev::raw_stream::enumerate;

fn main() {
    println!("The list of devices:");
    for device in enumerate() {
        println!("{:#?}", device);
    }
}

use elk_led_controller::*;
use std::{env, io};

#[tokio::main]
async fn main() -> Result<()> {
    // Get a target id/mac address from command line arguments.
    // If not provided, exit.
    let usage = "Usage: elkd <id/mac address>";
    let args: Vec<_> = env::args().collect();
    if args.len() < 2 {
        eprintln!("{usage}");
        std::process::exit(1);
    }
    if args[1] == "-h" || args[1] == "--help" {
        eprintln!("{usage}");
        std::process::exit(0);
    }

    // Initialize the device with the provided address
    let mut device = BleLedDevice::new_with_addr(&args[1]).await?;
    device.command_delay = 0; // Set a small delay for command processing

    // Inform about successful initialization
    println!("OK");

    // Mainloop: wait for user input, line by line
    loop {
        // Read a command from stdin
        let mut input: String = String::new();
        io::stdin().read_line(&mut input).expect("!!");

        // Read command and execute it
        let mut cmd = input.trim().split(":");
        match cmd.next() {
            Some("power_on") => {
                device.power_on().await?;
                // Respond with OK message
                println!("OK");
            }
            Some("power_off") => {
                device.power_off().await?;
                // Respond with OK message
                println!("OK");
            }
            Some("set_color") => {
                let rgb: Vec<u8> = cmd
                    .next()
                    .expect("no color given")
                    .split(",")
                    .map(|s| s.trim().parse().expect("invalid color"))
                    .collect();
                if rgb.len() != 3 {
                    eprintln!("ERR Invalid color format. Use R,G,B (e.g., 255,0,0 for red)");
                    continue;
                }
                device.set_color(rgb[0], rgb[1], rgb[2]).await?;
                // Respond with OK message
                println!("OK");
            }
            Some("set_brightness") => {
                let brightness: u8 = cmd
                    .next()
                    .expect("no brightness given")
                    .trim()
                    .parse()
                    .expect("invalid brightness");
                if brightness > 100 {
                    eprintln!("ERR Brightness must be between 0 and 100");
                    continue;
                }
                device.set_brightness(brightness).await?;
                // Respond with OK message
                println!("OK");
            }
            Some(other) => {
                eprintln!("ERR Unknown command: {other}");
            }
            None => {
                eprintln!("ERR No command given");
            }
        }
    }
}

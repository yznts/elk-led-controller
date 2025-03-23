# ELK-BLEDOM Bluetooth LED Strip Controller

A Rust library for controlling ELK-BLEDOM and similar Bluetooth LED strips. Works with multiple device types including ELK-BLE, LEDBLE, MELK, ELK-BULB, and ELK-LAMPL.

[![Crates.io](https://img.shields.io/crates/v/elk_ble_led_controller)](https://crates.io/crates/elk_ble_led_controller)
[![Documentation](https://docs.rs/elk_ble_led_controller/badge.svg)](https://docs.rs/elk_ble_led_controller)

## Features

* Power on/off control
* RGB color control
* Color temperature control (2700K-6500K)
* Brightness adjustment
* Various effect modes (fade, jump, blink)
* Effect speed control
* Schedule on/off functionality
* Support for multiple compatible device types

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
elk_ble_led_controller = "0.1.0"
```

## Usage

```rust
use elk_ble_led_controller::*;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initialize and connect to the device
    info!("Scanning for compatible BLE LED devices...");
    let mut device = BleLedDevice::new().await?;
    info!("Connected to {} device", device.get_device_type_name());

    // Basic operations
    device.power_on().await?;
    device.set_color(255, 0, 0).await?; // Set to red
    device.set_brightness(80).await?;   // 80% brightness

    // Set an effect
    device.set_effect(EFFECTS.crossfade_red_green_blue).await?;
    device.set_effect_speed(50).await?; // Medium speed

    // Turn off
    device.power_off().await?;

    Ok(())
}
```

## Command-Line Tool

This package includes a command-line utility to control your LED device. After installing, you can run:

```bash
# Build and install the CLI tool
cargo install --path .

# Run commands (after installing)
elk-led-control demo       # Run full demo of all features
elk-led-control on         # Turn the device on
elk-led-control off        # Turn the device off
elk-led-control red        # Set color to red
elk-led-control green      # Set color to green
elk-led-control blue       # Set color to blue
elk-led-control white      # Set color to white
elk-led-control rainbow    # Set rainbow effect

# Set custom RGB color
elk-led-control color -r 255 -g 100 -b 50

# Set brightness
elk-led-control brightness -l 75

# Set color temperature
elk-led-control color-temp -k 3500

# Set effect with custom speed
elk-led-control effect -e crossfade_rgb -s 80

# Schedule to turn on at 8:30 AM on weekdays
elk-led-control schedule-on -h 8 -m 30 -d weekdays

# Schedule to turn off at 11:45 PM on weekdays
elk-led-control schedule-off -h 23 -m 45 -d weekdays
```

For development, you can also use cargo run:

```bash
cargo run -- on      # Turn on using cargo run
cargo run -- green   # Set to green using cargo run
```

## Device Compatibility

The library supports the following device types:
- ELK-BLE (Original ELK-BLEDOM devices)
- LEDBLE
- MELK
- ELK-BULB
- ELK-LAMPL

Device detection is automatic - the library will scan for and connect to the first compatible device it finds.

## API Reference

### Initialize the device
```rust
let mut device = BleLedDevice::new().await?;
```

### Power options
```rust
device.power_on().await?;   // Power ON
device.power_off().await?;  // Power OFF
```

### Time and Schedule options
```rust
// Set schedule for powering the leds on at 8:30 on Monday and Thursday
device.set_schedule_on(WEEK_DAYS.monday + WEEK_DAYS.thursday, 8, 30, true).await?;

// Set schedule for powering the leds off at 23:45 on all weekdays
device.set_schedule_off(WEEK_DAYS.week_days, 23, 45, true).await?;

// Set custom time (Hour, Minute, Second, Day_of_week(1-7))
device.set_custom_time(17, 0, 0, 3).await?;
```

The time of the device syncs automatically with the system time when initializing a device, so generally speaking, you don't need to use `set_custom_time()`.

### Control modes
```rust
// Set static color (R,G,B)
device.set_color(255, 150, 100).await?;

// Set led brightness (0-100)
device.set_brightness(100).await?;

// Set color temperature (2700K-6500K)
device.set_color_temp_kelvin(3500).await?;

// Set an effect
device.set_effect(EFFECTS.crossfade_red_green_blue_yellow_cyan_magenta_white).await?;

// Set effect speed (0-100)
device.set_effect_speed(50).await?;
```

### Available Effects

The library provides many pre-defined effects:

```rust
// Jump effects
EFFECTS.jump_red_green_blue
EFFECTS.jump_red_green_blue_yellow_cyan_magenta_white

// Crossfade effects
EFFECTS.crossfade_red
EFFECTS.crossfade_green
EFFECTS.crossfade_blue
EFFECTS.crossfade_yellow
EFFECTS.crossfade_cyan
EFFECTS.crossfade_magenta
EFFECTS.crossfade_white
EFFECTS.crossfade_red_green
EFFECTS.crossfade_red_blue
EFFECTS.crossfade_green_blue
EFFECTS.crossfade_red_green_blue
EFFECTS.crossfade_red_green_blue_yellow_cyan_magenta_white

// Blink effects
EFFECTS.blink_red
EFFECTS.blink_green
EFFECTS.blink_blue
EFFECTS.blink_yellow
EFFECTS.blink_cyan
EFFECTS.blink_magenta
EFFECTS.blink_white
EFFECTS.blink_red_green_blue_yellow_cyan_magenta_white
```

### Schedule Day Options

Days of the week can be specified using the following constants:

```rust
WEEK_DAYS.monday
WEEK_DAYS.tuesday
WEEK_DAYS.wednesday
WEEK_DAYS.thursday
WEEK_DAYS.friday
WEEK_DAYS.saturday
WEEK_DAYS.sunday
WEEK_DAYS.all           // All days
WEEK_DAYS.week_days     // Monday-Friday
WEEK_DAYS.weekend_days  // Saturday-Sunday
WEEK_DAYS.none          // No days
```

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Acknowledgments

* Original code by TheSylex
* Based on reverse engineering of the ELK-BLEDOM Bluetooth protocol

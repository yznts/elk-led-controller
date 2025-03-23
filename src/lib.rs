/*!
 # ELK-BLEDOM Bluetooth LED Strip Controller Library

 A Rust library for controlling ELK-BLEDOM and similar Bluetooth LED strips.
 Supports multiple device types including ELK-BLE, LEDBLE, MELK, ELK-BULB, and ELK-LAMPL.

 ## Features

 * Power on/off control
 * RGB color control
 * Color temperature control
 * Brightness adjustment
 * Effect modes (fade, jump, blink)
 * Effect speed control
 * Scheduling

 ## Example

 ```rust
 use elk_led_controller::*;

 #[tokio::main]
 async fn main() -> Result<(), Error> {
     // Initialize tracing for logs
     tracing_subscriber::fmt::init();

     // Initialize error handling
     color_eyre::install()?;

     // Initialize and connect to the device
     let mut device = BleLedDevice::new_without_power().await?;

     // Basic operations
     device.power_on().await?;
     device.set_color(255, 0, 0).await?; // Set to red
     device.set_brightness(80).await?;   // 80% brightness

     Ok(())
 }
 ```
*/

use thiserror::Error;

/// Custom error types for the ELK LED controller library
#[derive(Error, Debug)]
pub enum Error {
    /// No Bluetooth adapters found
    #[error("No Bluetooth adapters found")]
    NoBluetoothAdapters,

    /// No compatible LED device found
    #[error("No compatible LED device found")]
    NoCompatibleDevice,

    /// Failed to find required BLE characteristic
    #[error("Could not find required BLE characteristic: {0}")]
    CharacteristicNotFound(String),

    /// BLE communication error
    #[error("BLE communication error: {0}")]
    BleError(String),

    /// Command timeout
    #[error("Command timed out after {0} retries")]
    CommandTimeout(u8),

    /// Value out of range
    #[error("Value {0} out of range ({1}..{2})")]
    ValueOutOfRange(u32, u32, u32),

    /// General error
    #[error("Error: {0}")]
    General(String),

    /// Error from btleplug
    #[error(transparent)]
    BtlePlugError(#[from] btleplug::Error),

    /// Other errors
    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}

// Import needed for Result type extension
pub type Result<T> = std::result::Result<T, Error>;

// Re-export modules
pub mod device;
pub mod effects;
pub mod schedule;

// Re-export key types
pub use device::{BleLedDevice, Days, DeviceConfig, DeviceType, Effects, EFFECTS, WEEK_DAYS};

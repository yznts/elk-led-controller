use btleplug::api::{
    Central, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use chrono::{self, Datelike, Timelike};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, Semaphore};
use tokio::time;
use tracing::{debug, error, info, instrument, trace, warn};
use uuid::Uuid;

// Import our custom error type
use crate::{Error, Result};

// Re-export schedule and effects modules
pub use crate::effects::{Effects, EFFECTS};
pub use crate::schedule::{Days, WEEK_DAYS};

/// Gets the default Bluetooth adapter
#[instrument(skip(manager))]
async fn get_central(manager: &Manager) -> Result<Adapter> {
    debug!("Getting default Bluetooth adapter");
    let adapters = manager.adapters().await?;
    if adapters.is_empty() {
        error!("No Bluetooth adapters found");
        return Err(Error::NoBluetoothAdapters);
    }

    let adapter = adapters.into_iter().next().unwrap();
    debug!("Using Bluetooth adapter");
    Ok(adapter)
}

/// Supported device types for LED control
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    /// ELK-BLE device type
    ElkBle,
    /// LEDBLE device type
    LedBle,
    /// MELK device type
    Melk,
    /// ELK-BULB device type
    ElkBulb,
    /// ELK-LAMPL device type
    ElkLampl,
    /// Unknown device type
    Unknown,
}

/// Configuration for different device types
#[derive(Debug, Clone)]
pub struct DeviceConfig {
    /// UUID for write characteristic
    pub write_uuid: Uuid,
    /// UUID for read characteristic
    pub read_uuid: Uuid,
    /// Command to turn the device on
    pub turn_on_cmd: [u8; 9],
    /// Command to turn the device off
    pub turn_off_cmd: [u8; 9],
    /// Minimum supported color temperature in Kelvin
    pub min_color_temp_k: u32,
    /// Maximum supported color temperature in Kelvin
    pub max_color_temp_k: u32,
    /// Command processing time in milliseconds
    pub command_delay: u64,
}

/// Command queue to manage Bluetooth commands with rate limiting
struct CommandQueue {
    /// Semaphore to limit command concurrency
    semaphore: Semaphore,
    /// Minimum delay between commands
    min_delay: Duration,
    /// Last command timestamp
    last_command: Mutex<std::time::Instant>,
}

impl CommandQueue {
    fn new(min_delay_ms: u64) -> Self {
        Self {
            semaphore: Semaphore::new(1), // Only allow one command at a time
            min_delay: Duration::from_millis(min_delay_ms),
            last_command: Mutex::new(std::time::Instant::now() - Duration::from_secs(1)),
        }
    }

    async fn execute<T, F>(&self, future: F) -> T
    where
        F: std::future::Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        // Acquire permit to ensure only one command executes at a time
        let _permit = self.semaphore.acquire().await.unwrap();

        // Check if we need to wait before executing
        let mut last_cmd = self.last_command.lock().await;
        let elapsed = last_cmd.elapsed();
        if elapsed < self.min_delay {
            let wait_time = self.min_delay - elapsed;
            trace!("Rate limiting: waiting {:?} before next command", wait_time);
            tokio::time::sleep(wait_time).await;
        }

        // Execute the command
        let result = future.await;

        // Update last command time
        *last_cmd = std::time::Instant::now();

        result
    }
}

/// Main struct for controlling an LED strip via Bluetooth LE
pub struct BleLedDevice {
    /// The connected Bluetooth peripheral
    peripheral: Peripheral,
    /// Characteristic used for sending commands
    write_characteristic: Characteristic,
    /// Optional characteristic for reading device state
    /// This is currently stored for future implementation of device status reading,
    /// but not yet used in the current version.
    #[allow(dead_code)]
    read_characteristic: Option<Characteristic>,
    /// Type of the connected device
    device_type: DeviceType,
    /// Device-specific configuration
    config: DeviceConfig,
    /// Command queue for rate limiting
    command_queue: Arc<CommandQueue>,
    /// Current power state
    pub is_on: bool,
    /// Current RGB color (red, green, blue)
    pub rgb_color: (u8, u8, u8),
    /// Current brightness (0-100)
    pub brightness: u8,
    /// Current effect mode if active
    pub effect: Option<u8>,
    /// Current effect speed if an effect is active
    pub effect_speed: Option<u8>,
    /// Current color temperature in Kelvin if using white mode
    pub color_temp_kelvin: Option<u32>,
    /// Delay configuration for command processing (in milliseconds)
    pub command_delay: u64,
}

impl BleLedDevice {
    /// Creates a new instance by scanning for and connecting to a compatible LED strip
    /// and automatically powers it on
    #[instrument]
    pub async fn new() -> Result<BleLedDevice> {
        let mut device = Self::new_without_power().await?;

        // Power on by default
        info!("Powering on device");
        device.power_on().await?;

        info!(
            "Successfully connected to {} device",
            device.get_device_type_name()
        );

        Ok(device)
    }

    /// Creates a new instance by scanning for and connecting to a compatible LED strip
    /// without automatically powering it on
    #[instrument]
    pub async fn new_without_power() -> Result<BleLedDevice> {
        info!("Initializing BLE LED controller");
        let manager = Manager::new().await?;
        let central = get_central(&manager).await?;

        info!("Scanning for compatible BLE devices...");
        central.start_scan(ScanFilter::default()).await?;

        // Maximum time to wait for device discovery (10 seconds)
        let max_discovery_time = Duration::from_secs(10);
        let start_time = std::time::Instant::now();
        let mut found_device = false;
        let mut device: Option<(Peripheral, DeviceType)> = None;

        // Poll for devices until we find a compatible one or timeout
        while start_time.elapsed() < max_discovery_time && !found_device {
            // Poll for new devices
            let peripherals = central.peripherals().await?;
            debug!("Found {} BLE peripherals so far", peripherals.len());

            if !peripherals.is_empty() {
                info!(
                    "Checking {} BLE devices for compatibility...",
                    peripherals.len()
                );

                // Check each peripheral for compatibility
                for p in peripherals {
                    if let Ok(Some(props)) = p.properties().await {
                        if let Some(name) = props.local_name {
                            debug!("Found device: {}", name);
                            let device_type = if name.starts_with("ELK-BLE") {
                                DeviceType::ElkBle
                            } else if name.starts_with("LEDBLE") {
                                DeviceType::LedBle
                            } else if name.starts_with("MELK") {
                                DeviceType::Melk
                            } else if name.starts_with("ELK-BULB") {
                                DeviceType::ElkBulb
                            } else if name.starts_with("ELK-LAMPL") {
                                DeviceType::ElkLampl
                            } else {
                                DeviceType::Unknown
                            };

                            if device_type != DeviceType::Unknown {
                                info!(
                                    "Found compatible device: {} (type: {:?})",
                                    name, device_type
                                );
                                device = Some((p, device_type));
                                found_device = true;
                                break;
                            }
                        }
                    }
                }
            }

            if !found_device {
                // Report scanning progress
                let elapsed = start_time.elapsed().as_secs();
                let remaining = max_discovery_time.as_secs() - elapsed;
                info!(
                    "Still scanning for compatible devices... ({} seconds remaining)",
                    remaining
                );
                // Wait a moment before polling again
                time::sleep(Duration::from_millis(500)).await;
            }
        }

        // If we've timed out without finding a device, report and error
        if !found_device {
            central.stop_scan().await?;
            error!(
                "No compatible LED device found within {} seconds",
                max_discovery_time.as_secs()
            );
            return Err(Error::NoCompatibleDevice);
        }

        if let Some((peripheral, device_type)) = device {
            // Connection and fetching of characteristics
            info!("Connecting to device...");
            if !peripheral.is_connected().await? {
                peripheral.connect().await?;
            }

            central.stop_scan().await?;
            debug!("Discovering services...");
            peripheral.discover_services().await?;

            // Get configuration for this device type
            let config = Self::get_device_config(device_type);
            debug!("Using config for device type: {:?}", device_type);

            // Create command queue with device-specific delay
            let command_queue = Arc::new(CommandQueue::new(config.command_delay));

            // Find write characteristic
            let write_char = peripheral
                .characteristics()
                .into_iter()
                .find(|c| c.uuid == config.write_uuid)
                .ok_or(Error::CharacteristicNotFound(config.write_uuid.to_string()))?;

            debug!("Found write characteristic: {}", write_char.uuid);

            // Find read characteristic (may not be needed for all devices)
            let read_char = peripheral
                .characteristics()
                .into_iter()
                .find(|c| c.uuid == config.read_uuid);

            if let Some(ref char) = read_char {
                debug!("Found read characteristic: {}", char.uuid);
            } else {
                debug!("Read characteristic not found, but this is optional");
            }

            let device = BleLedDevice {
                peripheral,
                write_characteristic: write_char,
                read_characteristic: read_char,
                device_type,
                config,
                command_queue,
                is_on: false,
                rgb_color: (255, 255, 255),
                brightness: 100,
                effect: None,
                effect_speed: None,
                color_temp_kelvin: Some(5000),
                command_delay: 200,
            };

            // Sync time for devices that support it
            if device_type == DeviceType::ElkBle
                || device_type == DeviceType::ElkBulb
                || device_type == DeviceType::ElkLampl
            {
                debug!("Synchronizing device time");
                device.sync_time().await?;
            }

            info!(
                "Successfully connected to {} device (without powering on)",
                device.get_device_type_name()
            );
            Ok(device)
        } else {
            error!("No compatible LED device found");
            Err(Error::NoCompatibleDevice)
        }
    }

    /// Creates a new instance by scanning for and connecting to a LED strip with a specific MAC address or ID
    /// without automatically powering it on
    #[instrument]
    pub async fn new_with_addr(addr: &str) -> Result<BleLedDevice> {
        info!("Initializing BLE LED controller");
        let manager = Manager::new().await?;
        let central = get_central(&manager).await?;

        info!("Scanning for compatible BLE devices...");
        central.start_scan(ScanFilter::default()).await?;

        // Maximum time to wait for device discovery (10 seconds)
        let max_discovery_time = Duration::from_secs(10);
        let start_time = std::time::Instant::now();
        let mut found_device = false;
        let mut device: Option<(Peripheral, DeviceType)> = None;

        // Poll for devices until we find a compatible one or timeout
        while start_time.elapsed() < max_discovery_time && !found_device {
            // Poll for new devices
            let peripherals = central.peripherals().await?;
            debug!("Found {} BLE peripherals so far", peripherals.len());

            if !peripherals.is_empty() {
                info!(
                    "Checking {} BLE devices for compatibility...",
                    peripherals.len()
                );

                // Check each peripheral
                for p in peripherals {
                    if let Ok(Some(props)) = p.properties().await {
                        if let Some(name) = props.local_name {
                            println!("Found device: {} {}", p.id().to_string().to_lowercase(), name);
                            // Skip if the address does not match
                            if p.address().to_string().to_lowercase() != addr.to_lowercase()
                                && p.id().to_string().to_lowercase() != addr.to_lowercase()
                            {
                                continue;
                            }

                            debug!("Found device: {}", name);
                            let device_type = if name.starts_with("ELK-BLE") {
                                DeviceType::ElkBle
                            } else if name.starts_with("LEDBLE") {
                                DeviceType::LedBle
                            } else if name.starts_with("MELK") {
                                DeviceType::Melk
                            } else if name.starts_with("ELK-BULB") {
                                DeviceType::ElkBulb
                            } else if name.starts_with("ELK-LAMPL") {
                                DeviceType::ElkLampl
                            } else {
                                DeviceType::Unknown
                            };

                            if device_type == DeviceType::Unknown {
                                error!(
                                    "Device with a given address {} is not compatible: {}",
                                    addr, name,
                                );
                            }

                            device = Some((p, device_type));
                            found_device = true;
                            break;
                        }
                    }
                }
            }

            if !found_device {
                // Report scanning progress
                let elapsed = start_time.elapsed().as_secs();
                let remaining = max_discovery_time.as_secs() - elapsed;
                info!(
                    "Still scanning for a device... ({} seconds remaining)",
                    remaining
                );
                // Wait a moment before polling again
                time::sleep(Duration::from_millis(500)).await;
            }
        }

        // If we've timed out without finding a device, report and error
        if !found_device {
            central.stop_scan().await?;
            error!(
                "No compatible LED device found within {} seconds",
                max_discovery_time.as_secs()
            );
            return Err(Error::NoCompatibleDevice);
        }

        if let Some((peripheral, device_type)) = device {
            // Connection and fetching of characteristics
            info!("Connecting to device...");
            if !peripheral.is_connected().await? {
                peripheral.connect().await?;
            }

            central.stop_scan().await?;
            debug!("Discovering services...");
            peripheral.discover_services().await?;

            // Get configuration for this device type
            let config = Self::get_device_config(device_type);
            debug!("Using config for device type: {:?}", device_type);

            // Create command queue with device-specific delay
            let command_queue = Arc::new(CommandQueue::new(config.command_delay));

            // Find write characteristic
            let write_char = peripheral
                .characteristics()
                .into_iter()
                .find(|c| c.uuid == config.write_uuid)
                .ok_or(Error::CharacteristicNotFound(config.write_uuid.to_string()))?;

            debug!("Found write characteristic: {}", write_char.uuid);

            // Find read characteristic (may not be needed for all devices)
            let read_char = peripheral
                .characteristics()
                .into_iter()
                .find(|c| c.uuid == config.read_uuid);

            if let Some(ref char) = read_char {
                debug!("Found read characteristic: {}", char.uuid);
            } else {
                debug!("Read characteristic not found, but this is optional");
            }

            let device = BleLedDevice {
                peripheral,
                write_characteristic: write_char,
                read_characteristic: read_char,
                device_type,
                config,
                command_queue,
                is_on: false,
                rgb_color: (255, 255, 255),
                brightness: 100,
                effect: None,
                effect_speed: None,
                color_temp_kelvin: Some(5000),
                command_delay: 200,
            };

            // Sync time for devices that support it
            if device_type == DeviceType::ElkBle
                || device_type == DeviceType::ElkBulb
                || device_type == DeviceType::ElkLampl
            {
                debug!("Synchronizing device time");
                device.sync_time().await?;
            }

            info!(
                "Successfully connected to {} device (without powering on)",
                device.get_device_type_name()
            );
            Ok(device)
        } else {
            error!("No compatible LED device found");
            Err(Error::NoCompatibleDevice)
        }
    }

    /// Get configuration based on device type
    fn get_device_config(device_type: DeviceType) -> DeviceConfig {
        match device_type {
            DeviceType::ElkBle => DeviceConfig {
                write_uuid: Uuid::parse_str("0000fff3-0000-1000-8000-00805f9b34fb").unwrap(),
                read_uuid: Uuid::parse_str("0000fff4-0000-1000-8000-00805f9b34fb").unwrap(),
                turn_on_cmd: [0x7e, 0x00, 0x04, 0xf0, 0x00, 0x01, 0xff, 0x00, 0xef],
                turn_off_cmd: [0x7e, 0x00, 0x04, 0x00, 0x00, 0x00, 0xff, 0x00, 0xef],
                min_color_temp_k: 2700,
                max_color_temp_k: 6500,
                command_delay: 15, // 15 seems to be the lowest value supported
            },
            DeviceType::LedBle => DeviceConfig {
                write_uuid: Uuid::parse_str("0000ffe1-0000-1000-8000-00805f9b34fb").unwrap(),
                read_uuid: Uuid::parse_str("0000ffe2-0000-1000-8000-00805f9b34fb").unwrap(),
                turn_on_cmd: [0x7e, 0x00, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0xef],
                turn_off_cmd: [0x7e, 0x00, 0x04, 0x00, 0x00, 0x00, 0xff, 0x00, 0xef],
                min_color_temp_k: 2700,
                max_color_temp_k: 6500,
                command_delay: 15,
            },
            DeviceType::Melk => DeviceConfig {
                write_uuid: Uuid::parse_str("0000fff3-0000-1000-8000-00805f9b34fb").unwrap(),
                read_uuid: Uuid::parse_str("0000fff4-0000-1000-8000-00805f9b34fb").unwrap(),
                turn_on_cmd: [0x7e, 0x00, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0xef],
                turn_off_cmd: [0x7e, 0x00, 0x04, 0x00, 0x00, 0x00, 0xff, 0x00, 0xef],
                min_color_temp_k: 2700,
                max_color_temp_k: 6500,
                command_delay: 15,
            },
            DeviceType::ElkBulb | DeviceType::ElkLampl | DeviceType::Unknown => DeviceConfig {
                write_uuid: Uuid::parse_str("0000fff3-0000-1000-8000-00805f9b34fb").unwrap(),
                read_uuid: Uuid::parse_str("0000fff4-0000-1000-8000-00805f9b34fb").unwrap(),
                turn_on_cmd: [0x7e, 0x00, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0xef],
                turn_off_cmd: [0x7e, 0x00, 0x04, 0x00, 0x00, 0x00, 0xff, 0x00, 0xef],
                min_color_temp_k: 2700,
                max_color_temp_k: 6500,
                command_delay: 15,
            },
        }
    }

    /// Get the device type name as string
    pub fn get_device_type_name(&self) -> &'static str {
        match self.device_type {
            DeviceType::ElkBle => "ELK-BLE",
            DeviceType::LedBle => "LEDBLE",
            DeviceType::Melk => "MELK",
            DeviceType::ElkBulb => "ELK-BULB",
            DeviceType::ElkLampl => "ELK-LAMPL",
            DeviceType::Unknown => "Unknown",
        }
    }

    /// Synchronizes the device's internal clock with the system time
    #[instrument(skip(self))]
    async fn sync_time(&self) -> Result<()> {
        let system_time = chrono::Local::now();
        debug!(
            "Syncing device time to {}:{}:{} day:{}",
            system_time.hour(),
            system_time.minute(),
            system_time.second(),
            system_time.weekday().number_from_monday()
        );

        self.send_command(&[
            0x7e,
            0x00,
            0x83,
            system_time.hour() as u8,
            system_time.minute() as u8,
            system_time.second() as u8,
            system_time.weekday().number_from_monday() as u8,
            0x00,
            0xef,
        ])
        .await?;

        debug!("Time synchronization complete");
        Ok(())
    }

    /// Sets a custom time on the device
    ///
    /// # Arguments
    ///
    /// * `hour` - Hour (0-23)
    /// * `minute` - Minute (0-59)
    /// * `second` - Second (0-59)
    /// * `day_of_week` - Day of week (1-7, where 1 is Monday)
    #[instrument(skip(self))]
    pub async fn set_custom_time(
        &self,
        hour: u8,
        minute: u8,
        second: u8,
        day_of_week: u8,
    ) -> Result<()> {
        let hour = hour.min(23);
        let minute = minute.min(59);
        let second = second.min(59);
        let day_of_week = day_of_week.clamp(1, 7);

        debug!(
            "Setting custom time to {}:{}:{} day:{}",
            hour, minute, second, day_of_week
        );

        self.send_command(&[
            0x7e,
            0x00,
            0x83,
            hour,
            minute,
            second,
            day_of_week,
            0x00,
            0xef,
        ])
        .await?;

        debug!("Custom time set successfully");
        Ok(())
    }

    /// Turns the LED strip on
    #[instrument(skip(self))]
    pub async fn power_on(&mut self) -> Result<()> {
        debug!("Turning LED strip on");
        self.send_command(&self.config.turn_on_cmd).await?;
        self.is_on = true;

        // Add a small delay to ensure the command has been processed
        time::sleep(Duration::from_millis(self.command_delay)).await;
        info!("LED strip powered on");
        Ok(())
    }

    /// Turns the LED strip off
    #[instrument(skip(self))]
    pub async fn power_off(&mut self) -> Result<()> {
        debug!("Turning LED strip off");
        self.send_command(&self.config.turn_off_cmd).await?;
        self.is_on = false;

        // Add a small delay to ensure the command has been processed
        time::sleep(Duration::from_millis(self.command_delay)).await;
        info!("LED strip powered off");
        Ok(())
    }

    /// Sets the RGB color of the LED strip
    ///
    /// # Arguments
    ///
    /// * `red_value` - Red component (0-255)
    /// * `green_value` - Green component (0-255)
    /// * `blue_value` - Blue component (0-255)
    #[instrument(skip(self))]
    pub async fn set_color(
        &mut self,
        red_value: u8,
        green_value: u8,
        blue_value: u8,
    ) -> Result<()> {
        debug!(
            "Setting color to RGB({}, {}, {})",
            red_value, green_value, blue_value
        );

        // First, ensure we're in RGB mode (not an effect)
        if self.effect.is_some() {
            debug!("Disabling active effect before setting color");
            // Send a pre-command to disable effects mode
            self.send_command(&[0x7e, 0x00, 0x05, 0x01, 0x00, 0x00, 0x00, 0x00, 0xef])
                .await?;
            // Add a small delay after disabling effect
            time::sleep(Duration::from_millis(self.command_delay)).await;
        }

        // Now set the RGB color
        trace!("Sending RGB color command");
        self.send_command(&[
            0x7e,
            0x00,
            0x05,
            0x03,
            red_value,
            green_value,
            blue_value,
            0x00,
            0xef,
        ])
        .await?;

        // Update the state
        self.rgb_color = (red_value, green_value, blue_value);
        self.effect = None; // Setting a static color disables any active effect

        // Add a small delay to ensure the command has been processed
        time::sleep(Duration::from_millis(self.command_delay)).await;
        info!(
            "Color set to RGB({}, {}, {})",
            red_value, green_value, blue_value
        );
        Ok(())
    }

    /// Sets the brightness level
    ///
    /// # Arguments
    ///
    /// * `value` - Brightness level (0-100)
    #[instrument(skip(self))]
    pub async fn set_brightness(&mut self, value: u8) -> Result<()> {
        let limited_value = value.min(100);
        if value > 100 {
            warn!(
                "Brightness value {} out of range (0-100), limiting to 100",
                value
            );
        }

        debug!("Setting brightness to {}%", limited_value);
        self.send_command(&[
            0x7e,
            0x00,
            0x01,
            limited_value,
            0x00,
            0x00,
            0x00,
            0x00,
            0xef,
        ])
        .await?;

        self.brightness = limited_value;

        info!("Brightness set to {}%", limited_value);
        Ok(())
    }

    /// Sets a light effect mode
    ///
    /// # Arguments
    ///
    /// * `value` - Effect code (use the EFFECTS constant)
    #[instrument(skip(self))]
    pub async fn set_effect(&mut self, value: u8) -> Result<()> {
        debug!("Setting effect mode to code: {:#04x}", value);

        // Send the effect command with retries
        self.send_command(&[0x7e, 0x00, 0x03, value, 0x03, 0x00, 0x00, 0x00, 0xef])
            .await?;

        self.effect = Some(value);

        // Add a small delay to ensure the command has been processed
        time::sleep(Duration::from_millis(self.command_delay)).await;
        info!("Effect mode set successfully");
        Ok(())
    }

    /// Sets the speed of the current effect
    ///
    /// # Arguments
    ///
    /// * `value` - Effect speed (0-100)
    #[instrument(skip(self))]
    pub async fn set_effect_speed(&mut self, value: u8) -> Result<()> {
        let limited_value = value.min(100);
        if value > 100 {
            warn!(
                "Effect speed {} out of range (0-100), limiting to 100",
                value
            );
        }

        if self.effect.is_none() {
            warn!("Setting effect speed without an active effect. This may not have any effect.");
        }

        debug!("Setting effect speed to {}", limited_value);
        // Send the effect speed command with retries
        self.send_command(&[
            0x7e,
            0x00,
            0x02,
            limited_value,
            0x00,
            0x00,
            0x00,
            0x00,
            0xef,
        ])
        .await?;

        self.effect_speed = Some(limited_value);

        // Add a small delay to ensure the command has been processed
        time::sleep(Duration::from_millis(self.command_delay)).await;
        info!("Effect speed set to {}", limited_value);
        Ok(())
    }

    /// Sets the color temperature in Kelvin for white light
    ///
    /// # Arguments
    ///
    /// * `value` - Color temperature in Kelvin (typically 2700-6500)
    #[instrument(skip(self))]
    pub async fn set_color_temp_kelvin(&mut self, value: u32) -> Result<()> {
        // Ensure value is within range
        let temp = value
            .max(self.config.min_color_temp_k)
            .min(self.config.max_color_temp_k);

        if value < self.config.min_color_temp_k || value > self.config.max_color_temp_k {
            warn!(
                "Color temperature {} out of range ({}-{}), adjusting to {}",
                value, self.config.min_color_temp_k, self.config.max_color_temp_k, temp
            );
        }

        debug!("Setting color temperature to {}K", temp);

        // Calculate color temp percent (0-100) from kelvin value
        let color_temp_percent = ((temp - self.config.min_color_temp_k) * 100
            / (self.config.max_color_temp_k - self.config.min_color_temp_k))
            as u8;

        // Set warm/cold values
        let warm = color_temp_percent;
        let cold = 100 - color_temp_percent;

        // First, ensure we're in white mode (not an effect)
        if self.effect.is_some() {
            debug!("Disabling active effect before setting color temperature");
            // Send a pre-command to disable effects mode
            self.send_command(&[0x7e, 0x00, 0x05, 0x01, 0x00, 0x00, 0x00, 0x00, 0xef])
                .await?;
            // Add a small delay after disabling effect
            time::sleep(Duration::from_millis(self.command_delay)).await;
        }

        // Now set the color temperature
        trace!(
            "Sending color temperature command: warm={}, cold={}",
            warm,
            cold
        );
        self.send_command(&[0x7e, 0x00, 0x05, 0x02, warm, cold, 0x00, 0x00, 0xef])
            .await?;

        self.color_temp_kelvin = Some(temp);
        self.effect = None; // Setting color temp disables any active effect

        // Add a small delay to ensure the command has been processed
        time::sleep(Duration::from_millis(self.command_delay)).await;
        info!("Color temperature set to {}K", temp);
        Ok(())
    }

    /// Sets a schedule to turn on the device
    ///
    /// # Arguments
    ///
    /// * `days` - Bitmask of days (use the WEEK_DAYS constants)
    /// * `hours` - Hour to turn on (0-23)
    /// * `minutes` - Minute to turn on (0-59)
    /// * `enabled` - Whether to enable or disable this schedule
    #[instrument(skip(self))]
    pub async fn set_schedule_on(
        &self,
        days: u8,
        hours: u8,
        minutes: u8,
        enabled: bool,
    ) -> Result<()> {
        let hours = hours.min(23);
        let minutes = minutes.min(59);
        let value = if enabled { days + 0x80 } else { days };

        debug!(
            "Setting schedule to turn on at {}:{:02} on days: {:#04x}, enabled: {}",
            hours, minutes, days, enabled
        );

        self.send_command(&[0x7e, 0x00, 0x82, hours, minutes, 0x00, 0x00, value, 0xef])
            .await?;

        // Add a small delay to ensure the command has been processed
        time::sleep(Duration::from_millis(self.command_delay)).await;
        info!("Schedule set to turn on at {}:{:02}", hours, minutes);
        Ok(())
    }

    /// Sets a schedule to turn off the device
    ///
    /// # Arguments
    ///
    /// * `days` - Bitmask of days (use the WEEK_DAYS constants)
    /// * `hours` - Hour to turn off (0-23)
    /// * `minutes` - Minute to turn off (0-59)
    /// * `enabled` - Whether to enable or disable this schedule
    #[instrument(skip(self))]
    pub async fn set_schedule_off(
        &self,
        days: u8,
        hours: u8,
        minutes: u8,
        enabled: bool,
    ) -> Result<()> {
        let hours = hours.min(23);
        let minutes = minutes.min(59);
        let value = if enabled { days + 0x80 } else { days };

        debug!(
            "Setting schedule to turn off at {}:{:02} on days: {:#04x}, enabled: {}",
            hours, minutes, days, enabled
        );

        self.send_command(&[0x7e, 0x00, 0x82, hours, minutes, 0x00, 0x01, value, 0xef])
            .await?;

        // Add a small delay to ensure the command has been processed
        time::sleep(Duration::from_millis(self.command_delay)).await;
        info!("Schedule set to turn off at {}:{:02}", hours, minutes);
        Ok(())
    }

    /// Sends a generic command to the device with retries
    ///
    /// # Arguments
    ///
    /// * `id` - Command ID
    /// * `sub_id` - Sub command ID
    /// * `arg1` - First argument
    /// * `arg2` - Second argument
    /// * `arg3` - Third argument
    #[instrument(skip(self))]
    pub async fn generic_command(
        &self,
        id: u8,
        sub_id: u8,
        arg1: u8,
        arg2: u8,
        arg3: u8,
    ) -> Result<()> {
        debug!(
            "Sending generic command: id={:#04x}, sub_id={:#04x}, args=[{:#04x}, {:#04x}, {:#04x}]",
            id, sub_id, arg1, arg2, arg3
        );

        self.send_command(&[0x7e, 0x00, id, sub_id, arg1, arg2, arg3, 0x00, 0xef])
            .await?;
        debug!("Generic command sent successfully");
        Ok(())
    }

    /// Helper function to ensure commands are sent reliably with rate limiting
    #[instrument(skip(self, command), fields(cmd_length = command.len()))]
    async fn send_command(&self, command: &[u8]) -> Result<()> {
        // Create a clone of the command for the async block
        let cmd = command.to_vec();
        let peripheral = self.peripheral.clone();
        let write_characteristic = self.write_characteristic.clone();

        // Use the command queue to handle rate limiting
        self.command_queue
            .execute(async move {
                // TODO: Fix this as delay is not working
                // BLE can be unreliable, so we implement retries
                let max_retries = 3;
                let mut attempt = 0;

                // Determine write type - prefer WriteWithResponse when supported
                let write_type = if write_characteristic
                    .properties
                    .contains(btleplug::api::CharPropFlags::WRITE)
                {
                    WriteType::WithResponse
                } else {
                    WriteType::WithoutResponse
                };

                while attempt < max_retries {
                    trace!(
                        "Sending BLE command (attempt {}/{})",
                        attempt + 1,
                        max_retries
                    );

                    match peripheral
                        .write(&write_characteristic, &cmd, write_type)
                        .await
                    {
                        Ok(_) => {
                            trace!("Command sent successfully");
                            return Ok(());
                        }
                        Err(e) => {
                            attempt += 1;
                            warn!(
                                "Command failed (attempt {}/{}): {}",
                                attempt, max_retries, e
                            );

                            if attempt < max_retries {
                                // Wait a bit before retrying
                                trace!("Waiting before retry...");
                                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                            } else {
                                // Log the last error
                                error!("Command failed permanently: {}", e);
                                return Err(Error::BleError(e.to_string()));
                            }
                        }
                    }
                }

                // Should never get here, but just in case
                error!("Command failed after {} attempts", max_retries);
                Err(Error::CommandTimeout(max_retries))
            })
            .await
    }
}

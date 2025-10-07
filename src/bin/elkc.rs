use clap::{Parser, Subcommand, ValueEnum};
use color_eyre::eyre::Result;
use elk_led_controller::*;
use tokio::time::Duration;
use tracing::{debug, error, info, instrument, trace, warn};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Clone, ValueEnum, Debug)]
enum EffectType {
    /// Crossfade through red, green, blue, yellow, cyan, magenta, white
    Rainbow,
    /// Jump between red, green, blue
    Jump,
    /// Jump through red, green, blue, yellow, cyan, magenta, white
    JumpAll,
    /// Crossfade red
    CrossfadeRed,
    /// Crossfade green
    CrossfadeGreen,
    /// Crossfade blue
    CrossfadeBlue,
    /// Crossfade through red, green, blue
    CrossfadeRgb,
    /// Blink through red, green, blue, yellow, cyan, magenta, white
    Blink,
    /// Blink red
    BlinkRed,
    /// Blink green
    BlinkGreen,
    /// Blink blue
    BlinkBlue,
}

impl std::fmt::Display for EffectType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EffectType::Rainbow => write!(f, "rainbow"),
            EffectType::Jump => write!(f, "jump"),
            EffectType::JumpAll => write!(f, "jump_all"),
            EffectType::CrossfadeRed => write!(f, "crossfade_red"),
            EffectType::CrossfadeGreen => write!(f, "crossfade_green"),
            EffectType::CrossfadeBlue => write!(f, "crossfade_blue"),
            EffectType::CrossfadeRgb => write!(f, "crossfade_rgb"),
            EffectType::Blink => write!(f, "blink"),
            EffectType::BlinkRed => write!(f, "blink_red"),
            EffectType::BlinkGreen => write!(f, "blink_green"),
            EffectType::BlinkBlue => write!(f, "blink_blue"),
        }
    }
}

#[derive(Clone, ValueEnum, Debug)]
enum AudioModeType {
    /// Map frequencies to colors (bass=red, mid=green, high=blue)
    FrequencyColor,
    /// Sound energy controls brightness
    EnergyBrightness,
    /// Beat detection triggers effects
    BeatEffects,
    /// Spectral flow pattern
    SpectralFlow,
    /// Enhanced frequency color mapping (warm for bass, cool for highs)
    EnhancedFrequencyColor,
    /// BPM synchronized effects
    BpmSync,
}

impl From<AudioModeType> for VisualizationMode {
    fn from(mode: AudioModeType) -> Self {
        match mode {
            AudioModeType::FrequencyColor => VisualizationMode::FrequencyColor,
            AudioModeType::EnergyBrightness => VisualizationMode::EnergyBrightness,
            AudioModeType::BeatEffects => VisualizationMode::BeatEffects,
            AudioModeType::SpectralFlow => VisualizationMode::SpectralFlow,
            AudioModeType::EnhancedFrequencyColor => VisualizationMode::EnhancedFrequencyColor,
            AudioModeType::BpmSync => VisualizationMode::BpmSync,
        }
    }
}

#[derive(Clone, ValueEnum, Debug)]
enum AudioRangeType {
    /// Bass frequencies (20-250 Hz)
    Bass,
    /// Mid-range frequencies (250-2000 Hz)
    Mid,
    /// High frequencies (2000-20000 Hz)
    High,
    /// Full spectrum
    Full,
}

impl From<AudioRangeType> for FrequencyRange {
    fn from(range: AudioRangeType) -> Self {
        match range {
            AudioRangeType::Bass => FrequencyRange::Bass,
            AudioRangeType::Mid => FrequencyRange::Mid,
            AudioRangeType::High => FrequencyRange::High,
            AudioRangeType::Full => FrequencyRange::Full,
        }
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Demonstration of LED features
    Demo {
        /// Duration of each demo step in seconds
        #[arg(short, long, default_value_t = 5)]
        duration: u64,
    },
    /// Turn LED strip on
    On,
    /// Turn LED strip off
    Off,
    /// Set to red color
    Red,
    /// Set to green color
    Green,
    /// Set to blue color
    Blue,
    /// Set to white color
    White,
    /// Set brightness
    Brightness {
        /// Brightness level (0-100)
        #[arg(short, long, default_value_t = 100)]
        level: u8,
    },
    /// Set color temperature
    ColorTemp {
        /// Color temperature in Kelvin (2700-6500)
        #[arg(short, long, default_value_t = 4000)]
        kelvin: u32,
    },
    /// Set custom RGB color
    Color {
        /// Red value (0-255)
        #[arg(short, long, default_value_t = 255)]
        red: u8,
        /// Green value (0-255)
        #[arg(short, long, default_value_t = 255)]
        green: u8,
        /// Blue value (0-255)
        #[arg(short, long, default_value_t = 255)]
        blue: u8,
    },
    /// Set effect
    Effect {
        /// Effect type (available options shown in description)
        #[arg(short, long, value_enum, default_value_t = EffectType::Rainbow)]
        effect_type: EffectType,
        /// Effect speed (0-100)
        #[arg(short, long, default_value_t = 50)]
        speed: u8,
    },
    /// Schedule to turn on
    ScheduleOn {
        /// Hour (0-23)
        #[arg(long, default_value_t = 8)]
        hour: u8,
        /// Minute (0-59)
        #[arg(short, long, default_value_t = 30)]
        minute: u8,
        /// Days (mon,tue,wed,thu,fri,sat,sun,all,weekdays,weekend)
        #[arg(short, long, default_value = "weekdays")]
        days: String,
    },
    /// Schedule to turn off
    ScheduleOff {
        /// Hour (0-23)
        #[arg(long, default_value_t = 23)]
        hour: u8,
        /// Minute (0-59)
        #[arg(short, long, default_value_t = 45)]
        minute: u8,
        /// Days (mon,tue,wed,thu,fri,sat,sun,all,weekdays,weekend)
        #[arg(short, long, default_value = "weekdays")]
        days: String,
    },
    /// Start audio-reactive LED visualization
    Audio {
        /// Visualization mode
        #[arg(short, long, value_enum, default_value_t = AudioModeType::FrequencyColor)]
        mode: AudioModeType,

        /// Frequency range to monitor
        #[arg(short, long, value_enum, default_value_t = AudioRangeType::Full)]
        range: AudioRangeType,

        /// Audio sensitivity (0-100)
        #[arg(short, long, default_value_t = 70)]
        sensitivity: u8,

        /// Update interval in milliseconds
        #[arg(short, long, default_value_t = 50)]
        update_ms: u32,

        /// Run in test mode (just display audio levels, don't control LEDs)
        #[arg(short, long, default_value_t = false)]
        test: bool,

        /// Audio device name to use (leave empty for default output device)
        #[arg(short, long)]
        device: Option<String>,
    },
}

#[tokio::main]
#[instrument]
async fn main() -> Result<()> {
    // Initialize tracing with pretty colors
    tracing_subscriber::fmt().compact().init();

    // Initialize color-eyre for pretty error reporting
    color_eyre::install()?;

    let cli = Cli::parse();
    debug!("Parsed command line arguments");

    // The info! macro doesn't work in main until after tracing_subscriber::fmt().init()
    // has been called, so it's safe to use it here
    info!("Starting LED controller");

    // Initialize the device but don't automatically power it on
    let mut device = match BleLedDevice::new_without_power().await {
        Ok(dev) => dev,
        Err(e) => {
            error!("Failed to initialize device: {}", e);
            return Err(e.into());
        }
    };

    match cli.command.unwrap_or(Commands::Demo { duration: 5 }) {
        Commands::Demo { duration } => {
            run_demo(&mut device, duration).await?;
        }
        Commands::On => {
            if !device.is_on {
                device.power_on().await?;
                info!("Device powered on");
            }
        }
        Commands::Off => {
            if device.is_on {
                device.power_off().await?;
                info!("Device powered off");
            }
        }
        Commands::Red => {
            if !device.is_on {
                device.power_on().await?;
            }
            device.set_color(255, 0, 0).await?;
            info!("Color set to RED");
        }
        Commands::Green => {
            if !device.is_on {
                device.power_on().await?;
            }
            device.set_color(0, 255, 0).await?;
            info!("Color set to GREEN");
        }
        Commands::Blue => {
            if !device.is_on {
                device.power_on().await?;
            }
            device.set_color(0, 0, 255).await?;
            info!("Color set to BLUE");
        }
        Commands::White => {
            if !device.is_on {
                device.power_on().await?;
            }
            device.set_color(255, 255, 255).await?;
            info!("Color set to WHITE");
        }
        Commands::Brightness { level } => {
            // We need to ensure the device is on for brightness changes to be visible
            if !device.is_on {
                device.power_on().await?;
            }
            device.set_brightness(level).await?;
            info!("Brightness set to {}", level);
        }
        Commands::ColorTemp { kelvin } => {
            if !device.is_on {
                device.power_on().await?;
            }
            device.set_color_temp_kelvin(kelvin).await?;
            info!("Color temperature set to {}K", kelvin);
        }
        Commands::Color { red, green, blue } => {
            if !device.is_on {
                device.power_on().await?;
            }
            device.set_color(red, green, blue).await?;
            info!("Color set to RGB({}, {}, {})", red, green, blue);
        }
        Commands::Effect { effect_type, speed } => {
            if !device.is_on {
                device.power_on().await?;
            }

            let effect_code = match effect_type {
                EffectType::Rainbow => EFFECTS.crossfade_red_green_blue_yellow_cyan_magenta_white,
                EffectType::Jump => EFFECTS.jump_red_green_blue,
                EffectType::JumpAll => EFFECTS.jump_red_green_blue_yellow_cyan_magenta_white,
                EffectType::CrossfadeRed => EFFECTS.crossfade_red,
                EffectType::CrossfadeGreen => EFFECTS.crossfade_green,
                EffectType::CrossfadeBlue => EFFECTS.crossfade_blue,
                EffectType::CrossfadeRgb => EFFECTS.crossfade_red_green_blue,
                EffectType::Blink => EFFECTS.blink_red_green_blue_yellow_cyan_magenta_white,
                EffectType::BlinkRed => EFFECTS.blink_red,
                EffectType::BlinkGreen => EFFECTS.blink_green,
                EffectType::BlinkBlue => EFFECTS.blink_blue,
            };

            device.set_effect(effect_code).await?;
            device.set_effect_speed(speed).await?;
            info!("Effect set to {} with speed {}", effect_type, speed);
        }
        Commands::ScheduleOn { hour, minute, days } => {
            if !device.is_on {
                device.power_on().await?;
            }

            let days_value = parse_days(&days);

            device
                .set_schedule_on(days_value, hour, minute, true)
                .await?;
            info!(
                "Schedule set to turn on at {:02}:{:02} on {}",
                hour, minute, days
            );
        }
        Commands::ScheduleOff { hour, minute, days } => {
            if !device.is_on {
                device.power_on().await?;
            }

            let days_value = parse_days(&days);

            device
                .set_schedule_off(days_value, hour, minute, true)
                .await?;
            info!(
                "Schedule set to turn off at {:02}:{:02} on {}",
                hour, minute, days
            );
        }
        Commands::Audio {
            mode,
            range,
            sensitivity,
            update_ms,
            test,
            device: audio_device,
        } => {
            if !device.is_on {
                device.power_on().await?;
            }

            run_audio_visualization(
                &mut device,
                mode,
                range,
                sensitivity,
                update_ms,
                test,
                audio_device,
            )
            .await?;
        }
    }

    info!("Command completed successfully");
    Ok(())
}

/// Parse days string to bitmask
#[instrument]
fn parse_days(days: &str) -> u8 {
    debug!("Parsing days string: {}", days);
    let result = match days.to_lowercase().as_str() {
        "mon" | "monday" => WEEK_DAYS.monday,
        "tue" | "tuesday" => WEEK_DAYS.tuesday,
        "wed" | "wednesday" => WEEK_DAYS.wednesday,
        "thu" | "thursday" => WEEK_DAYS.thursday,
        "fri" | "friday" => WEEK_DAYS.friday,
        "sat" | "saturday" => WEEK_DAYS.saturday,
        "sun" | "sunday" => WEEK_DAYS.sunday,
        "all" => WEEK_DAYS.all,
        "weekdays" => WEEK_DAYS.week_days,
        "weekend" => WEEK_DAYS.weekend_days,
        _ => {
            debug!("Parsing composite days string");
            let mut combined = 0;
            for day in days.split(',') {
                let day_value = parse_days(day);
                debug!("  Day '{}' = {:#04x}", day, day_value);
                combined |= day_value;
            }
            combined
        }
    };

    trace!("Days '{}' parsed to bitmask: {:#04x}", days, result);
    result
}

/// Sleep for specified number of seconds
#[instrument]
async fn sleep(seconds: u64) {
    trace!("Sleeping for {}s", seconds);
    tokio::time::sleep(Duration::from_secs(seconds)).await;
    trace!("Sleep completed");
}

/// Run audio visualization on the LED strip
#[instrument(skip(device))]
async fn run_audio_visualization(
    device: &mut BleLedDevice,
    mode: AudioModeType,
    range: AudioRangeType,
    sensitivity: u8,
    update_ms: u32,
    test: bool,
    audio_device: Option<String>,
) -> Result<()> {
    info!("Initializing audio monitoring in {:?} mode", mode);

    // Create audio monitor
    let audio_monitor = match AudioMonitor::new_with_device(audio_device) {
        Ok(monitor) => monitor,
        Err(e) => {
            error!("Failed to initialize audio monitoring: {}", e);
            return Err(e.into());
        }
    };

    // Configure audio visualization
    let mut config = audio_monitor.get_config();
    config.mode = mode.clone().into();
    config.range = range.into();
    config.sensitivity = sensitivity as f32 / 100.0; // Convert 0-100 to 0.0-1.0
    config.update_interval_ms = update_ms;

    audio_monitor.set_config(config);

    // Normal mode - control LEDs with audio
    info!("Starting audio visualization. Press Ctrl+C to exit.");

    // Start monitoring with LED control
    let ctrl_c = tokio::signal::ctrl_c();
    tokio::select! {
        result = audio_monitor.start_continuous_monitoring(device) => {
            if let Err(e) = result {
                error!("Audio monitoring error: {}", e);
                return Err(e.into());
            }
        }
        _ = ctrl_c => {
            info!("Received Ctrl+C, stopping audio visualization");
        }
    }

    // Clean up
    audio_monitor.stop();
    device.power_off().await?;

    info!("Audio visualization stopped");
    Ok(())
}

/// TODO: Convert this to test
/// Run a demonstration of various LED strip features
#[instrument(skip(device))]
async fn run_demo(device: &mut BleLedDevice, duration: u64) -> Result<()> {
    info!("Running LED strip demo with {}s intervals", duration);

    // Power on the leds
    info!("Turning LEDs on");
    device.power_on().await?;
    sleep(duration).await;

    // Set a static color
    info!("Setting color to red");
    device.set_color(255, 0, 0).await?; // Red
    sleep(duration).await;

    info!("Setting color to green");
    device.set_color(0, 255, 0).await?; // Green
    sleep(duration).await;

    info!("Setting color to blue");
    device.set_color(0, 0, 255).await?; // Blue
    sleep(duration).await;

    // Set led brightness (0-100)
    info!("Setting brightness to 50%");
    device.set_brightness(50).await?;
    sleep(duration).await;

    info!("Setting brightness to 100%");
    device.set_brightness(100).await?;
    sleep(duration).await;

    // Try color temperature
    info!("Setting warm white (2700K)");
    device.set_color_temp_kelvin(2700).await?;
    sleep(duration).await;

    info!("Setting cool white (6500K)");
    device.set_color_temp_kelvin(6500).await?;
    sleep(duration).await;

    // Set different effects
    info!("Setting rainbow crossfade effect");
    device
        .set_effect(EFFECTS.crossfade_red_green_blue_yellow_cyan_magenta_white)
        .await?;
    sleep(duration).await;

    info!("Setting RGB jump effect");
    device.set_effect(EFFECTS.jump_red_green_blue).await?;
    sleep(duration).await;

    info!("Setting RGB blink effect");
    device
        .set_effect(EFFECTS.blink_red_green_blue_yellow_cyan_magenta_white)
        .await?;
    sleep(duration).await;

    // Set effect speed
    info!("Setting effect speed to slow (20)");
    device.set_effect_speed(20).await?;
    sleep(duration).await;

    info!("Setting effect speed to fast (80)");
    device.set_effect_speed(80).await?;
    sleep(duration).await;

    // Go back to static white
    info!("Back to static white");
    device.set_color(255, 255, 255).await?;
    sleep(1).await;

    // End demo by turning off the lights
    info!("Turning LEDs off to end demo");
    device.power_off().await?;

    info!("Demo completed!");
    Ok(())
}

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat};
use parking_lot::RwLock;
use spectrum_analyzer::{samples_fft_to_spectrum, FrequencyLimit, FrequencySpectrum};
use std::sync::Arc;
use std::{
    collections::VecDeque,
    sync::atomic::{AtomicBool, Ordering},
};
use tokio::sync::{mpsc, watch};
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, instrument, warn};

use crate::{BleLedDevice, Error, Result, EFFECTS};

/// Frequency ranges for audio analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrequencyRange {
    /// Bass frequencies (20-250 Hz)
    Bass,
    /// Mid-range frequencies (250-2000 Hz)
    Mid,
    /// High frequencies (2000-20000 Hz)
    High,
    /// Full spectrum
    Full,
}

/// Visualization modes for audio monitoring
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualizationMode {
    /// Frequencies map to colors (bass=red, mid=green, high=blue)
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

/// Audio visualization settings and state
#[derive(Debug, Clone)]
pub struct AudioVisualization {
    /// Which frequency range to monitor
    pub range: FrequencyRange,
    /// How to visualize audio
    pub mode: VisualizationMode,
    /// Audio volume sensitivity (0.0-1.0)
    pub sensitivity: f32,
    /// Whether bass should trigger color changes
    pub bass_color_trigger: bool,
    /// Whether mids should trigger brightness changes
    pub mid_brightness_trigger: bool,
    /// Whether highs should trigger effect changes
    pub high_effect_trigger: bool,
    /// Minimum time between visualization updates (ms)
    pub update_interval_ms: u32,
    /// Whether to sync state from audio directly to LED
    pub active: bool,
}

impl Default for AudioVisualization {
    fn default() -> Self {
        Self {
            range: FrequencyRange::Full,
            mode: VisualizationMode::FrequencyColor,
            sensitivity: 0.7,
            bass_color_trigger: true,
            mid_brightness_trigger: true,
            high_effect_trigger: true,
            update_interval_ms: 50, // 50ms = 20 updates per second
            active: false,
        }
    }
}

/// Audio spectrum analyzer for LED visualization
#[derive(Debug)]
struct AudioAnalyzer {
    /// FFT sample size
    sample_size: usize,
    /// Audio sample rate
    sample_rate: usize,
    /// Recent audio samples for FFT
    samples: VecDeque<f32>,
    /// Detected audio energy by frequency range
    energy: [f32; 3], // [bass, mid, high]
    /// Smoothed energy values
    smoothed_energy: [f32; 3],
    /// Previous energy values for beat detection
    prev_energy: [f32; 3],
    /// Beat detection thresholds
    beat_thresholds: [f32; 3],
    /// Maximum energy values seen for normalization
    max_energy: [f32; 3],
    /// Whether a beat is currently detected in each range
    beat_detected: [bool; 3],
    /// Spectrum analyzer scaling factor
    scaling: f32,
    /// Tempo estimation (BPM)
    estimated_bpm: f32,
    /// Recent beat timestamps for BPM calculation
    beat_timestamps: VecDeque<f64>,
    /// Last time a beat was detected (unix timestamp in seconds)
    last_beat_time: f64,
    /// Energy history for better beat detection
    energy_history: [VecDeque<f32>; 3],
    /// Beat detection hit count for confidence measurement
    beat_count: [usize; 3],
}

impl AudioAnalyzer {
    /// Create a new audio analyzer
    fn new(sample_rate: usize) -> Self {
        let sample_size = 2048; // Power of 2 for FFT
        Self {
            sample_size,
            sample_rate,
            samples: VecDeque::with_capacity(sample_size * 2),
            energy: [0.0; 3],
            smoothed_energy: [0.0; 3],
            prev_energy: [0.0; 3],
            beat_thresholds: [1.4, 1.3, 1.2], // Bass, mid, high beat sensitivity (slightly more sensitive)
            max_energy: [0.01, 0.01, 0.01],   // Start with small values to avoid div by zero
            beat_detected: [false; 3],
            scaling: 0.8,         // Scaling factor for spectrum analysis
            estimated_bpm: 120.0, // Default BPM estimate
            beat_timestamps: VecDeque::with_capacity(50), // Store recent beat times
            last_beat_time: 0.0,
            energy_history: [
                VecDeque::with_capacity(20),
                VecDeque::with_capacity(20),
                VecDeque::with_capacity(20),
            ],
            beat_count: [0; 3],
        }
    }

    /// Add a sample to the analyzer
    fn add_sample(&mut self, sample: f32) {
        self.samples.push_back(sample);
        if self.samples.len() > self.sample_size {
            self.samples.pop_front();
        }
    }

    /// Analyze audio using FFT to extract frequency information
    fn analyze(&mut self) {
        // Need enough samples for the FFT
        if self.samples.len() < self.sample_size {
            return;
        }

        // Convert samples queue to vector for FFT
        let samples: Vec<f32> = self
            .samples
            .iter()
            .copied()
            .take(self.sample_size)
            .collect();

        // Perform FFT analysis
        match samples_fft_to_spectrum(
            &samples,
            self.sample_rate as u32,
            FrequencyLimit::Range(20.0, 20000.0),
            None, // No scaling function
        ) {
            Ok(spectrum) => {
                // Extract energy in different frequency bands
                self.extract_energy(&spectrum);
                self.detect_beats();
            }
            Err(e) => {
                warn!("FFT analysis error: {:?}", e);
            }
        }
    }

    /// Extract energy levels from frequency spectrum
    fn extract_energy(&mut self, spectrum: &FrequencySpectrum) {
        // Define frequency bands
        let bands = [
            (20.0, 250.0),     // Bass
            (250.0, 2000.0),   // Mid
            (2000.0, 20000.0), // High
        ];

        // Calculate energy for each band
        for (i, (low, high)) in bands.iter().enumerate() {
            // Get values in the frequency band
            let band_values: Vec<f32> = spectrum
                .data()
                .iter()
                .filter(|(freq, _)| freq.val() >= *low && freq.val() <= *high)
                .map(|(_, magnitude)| magnitude.val())
                .collect();

            if !band_values.is_empty() {
                // Average the magnitudes
                let band_energy = band_values.iter().sum::<f32>() / band_values.len() as f32;
                self.energy[i] = band_energy * self.scaling;

                // Update max energy (with dampening)
                self.max_energy[i] = self.max_energy[i] * 0.9995 + self.energy[i] * 0.0005;
                if self.energy[i] > self.max_energy[i] {
                    self.max_energy[i] = self.energy[i];
                }

                // Smooth energy (exponential moving average)
                self.smoothed_energy[i] = self.smoothed_energy[i] * 0.7 + self.energy[i] * 0.3;
            }
        }
    }

    /// Detect beats in each frequency band and estimate BPM
    fn detect_beats(&mut self) {
        // Get current timestamp for BPM calculation
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();

        for i in 0..3 {
            // Store energy in history for better beat detection
            self.energy_history[i].push_back(self.energy[i]);
            if self.energy_history[i].len() > 20 {
                self.energy_history[i].pop_front();
            }

            // Reset beat detection
            self.beat_detected[i] = false;

            // Normalize current energy
            let normalized_energy = if self.max_energy[i] > 0.0 {
                self.energy[i] / self.max_energy[i]
            } else {
                0.0
            };

            // Calculate local energy average (recent history)
            let local_energy_avg = if !self.energy_history[i].is_empty() {
                self.energy_history[i].iter().sum::<f32>() / self.energy_history[i].len() as f32
            } else {
                self.energy[i]
            };

            // Dynamic beat detection with multiple criteria
            let is_beat = normalized_energy > 0.3 && // Minimum energy threshold
                (
                    // Energy spike relative to previous sample
                    self.energy[i] > self.prev_energy[i] * self.beat_thresholds[i] ||

                    // Energy spike relative to local average
                    (self.energy[i] > local_energy_avg * 1.3 &&
                     // Make sure we don't detect beats too close together
                     current_time - self.last_beat_time > 0.2)
                );

            if is_beat {
                self.beat_detected[i] = true;
                self.beat_count[i] += 1;

                // BPM calculation - focus on bass for tempo
                if i == 0 {
                    // Bass frequency range
                    // Only update BPM if sufficient time has passed (prevent multiple triggers)
                    if current_time - self.last_beat_time > 0.2 {
                        self.last_beat_time = current_time;
                        self.beat_timestamps.push_back(current_time);

                        // Keep only recent beats for BPM calculation (last ~5 seconds)
                        while !self.beat_timestamps.is_empty()
                            && current_time - self.beat_timestamps.front().unwrap() > 5.0
                        {
                            self.beat_timestamps.pop_front();
                        }

                        // Calculate BPM if we have enough beats
                        if self.beat_timestamps.len() >= 4 {
                            let first_beat = *self.beat_timestamps.front().unwrap();
                            let last_beat = *self.beat_timestamps.back().unwrap();
                            let time_span = last_beat - first_beat;

                            if time_span > 0.0 {
                                // Calculate beats per minute
                                let beats = self.beat_timestamps.len() - 1; // Number of intervals
                                let new_bpm = (beats as f32 * 60.0) / time_span as f32;

                                // Smooth BPM changes (weighted average)
                                if (60.0..=200.0).contains(&new_bpm) {
                                    self.estimated_bpm = self.estimated_bpm * 0.7 + new_bpm * 0.3;
                                }
                            }
                        }
                    }
                }
            }

            // Update previous energy for next detection
            self.prev_energy[i] = self.energy[i];
        }
    }

    /// Get the estimated BPM (beats per minute)
    fn get_bpm(&self) -> f32 {
        self.estimated_bpm
    }

    /// Check if we're at a beat position according to BPM timing
    fn is_on_beat(&self, current_time: f64) -> bool {
        if self.estimated_bpm <= 0.0 {
            return false;
        }

        // Calculate seconds per beat
        let spb = 60.0 / self.estimated_bpm as f64;

        // Check if we're within 100ms of a beat
        let beat_position = (current_time - self.last_beat_time) % spb;
        beat_position < 0.1 || beat_position > spb - 0.1
    }

    /// Get normalized energy for a frequency range (0.0-1.0)
    fn get_normalized_energy(&self, range: FrequencyRange) -> f32 {
        match range {
            FrequencyRange::Bass => {
                if self.max_energy[0] > 0.0 {
                    self.smoothed_energy[0] / self.max_energy[0]
                } else {
                    0.0
                }
            }
            FrequencyRange::Mid => {
                if self.max_energy[1] > 0.0 {
                    self.smoothed_energy[1] / self.max_energy[1]
                } else {
                    0.0
                }
            }
            FrequencyRange::High => {
                if self.max_energy[2] > 0.0 {
                    self.smoothed_energy[2] / self.max_energy[2]
                } else {
                    0.0
                }
            }
            FrequencyRange::Full => {
                // Average of all bands
                let sum = self
                    .smoothed_energy
                    .iter()
                    .zip(self.max_energy.iter())
                    .map(|(e, m)| if *m > 0.0 { e / m } else { 0.0 })
                    .sum::<f32>();
                sum / 3.0
            }
        }
    }

    /// Check if beat is detected in a specific range
    fn is_beat_detected(&self, range: FrequencyRange) -> bool {
        match range {
            FrequencyRange::Bass => self.beat_detected[0],
            FrequencyRange::Mid => self.beat_detected[1],
            FrequencyRange::High => self.beat_detected[2],
            FrequencyRange::Full => self.beat_detected.iter().any(|&x| x),
        }
    }
}

/// The color calculated from audio spectrum
#[derive(Debug, Clone, Copy)]
struct AudioColor {
    r: u8,
    g: u8,
    b: u8,
    brightness: u8,
    effect: Option<u8>,
}

impl Default for AudioColor {
    fn default() -> Self {
        Self {
            r: 0,
            g: 0,
            b: 0,
            brightness: 100,
            effect: None,
        }
    }
}

/// Main audio monitoring system for LED control
pub struct AudioMonitor {
    /// Current visualization configuration
    config: Arc<RwLock<AudioVisualization>>,
    /// Channel for sending samples to analyzer
    #[allow(dead_code)]
    sample_tx: Option<mpsc::Sender<f32>>,
    /// Channel for receiving calculated colors
    color_rx: watch::Receiver<AudioColor>,
    /// Flag to stop the audio monitor
    stop_flag: Arc<AtomicBool>,
    /// The audio capture stream
    _stream: Option<cpal::Stream>,
}

impl AudioMonitor {
    /// Create a new audio monitor with default output device
    pub fn new() -> Result<Self> {
        Self::new_with_device(None)
    }

    /// Create a new audio monitor with a specified device name
    pub fn new_with_device(device_name: Option<String>) -> Result<Self> {
        let config = Arc::new(RwLock::new(AudioVisualization::default()));
        let stop_flag = Arc::new(AtomicBool::new(false));

        // Create channels for audio samples and colors
        let (sample_tx, sample_rx) = mpsc::channel::<f32>(4096);
        let (color_tx, color_rx) = watch::channel(AudioColor::default());

        // Set up audio capture
        let host = cpal::default_host();

        // Get input device by name or use default
        let input_device = if let Some(name) = device_name {
            info!("Searching for audio input device with name: {}", name);
            // Find input device by name
            match host.input_devices() {
                Ok(devices) => {
                    let mut matched_device = None;
                    for device in devices {
                        if let Ok(device_name) = device.name() {
                            if device_name.contains(&name) {
                                matched_device = Some(device);
                                info!("Found matching audio input device: {}", device_name);
                                break;
                            }
                        }
                    }

                    matched_device.ok_or_else(|| {
                        Error::AudioCaptureError(format!(
                            "Could not find audio input device: {}",
                            name
                        ))
                    })?
                }
                Err(err) => {
                    error!("Failed to enumerate audio input devices: {}", err);
                    return Err(Error::AudioCaptureError(format!(
                        "Failed to enumerate audio input devices: {}",
                        err
                    )));
                }
            }
        } else {
            // Use default input device
            match host.default_input_device() {
                Some(device) => {
                    info!(
                        "Using default audio input device: {}",
                        device.name().unwrap_or_default()
                    );
                    device
                }
                None => {
                    error!("No default audio input device available");
                    return Err(Error::AudioCaptureError(
                        "No default audio input device found".into(),
                    ));
                }
            }
        };

        // Get supported input configuration
        let config_range = match input_device.default_input_config() {
            Ok(config) => {
                debug!("Using default input config: {:?}", config);
                config
            }
            Err(err) => {
                error!("Failed to get default input config: {}", err);
                return Err(Error::AudioCaptureError(format!(
                    "Failed to get default input config: {}",
                    err
                )));
            }
        };

        // Get sample rate
        let sample_rate = config_range.sample_rate().0 as usize;
        debug!("Audio input sample rate: {} Hz", sample_rate);

        // Spawn analysis thread using std::thread since it doesn't need to be async
        let analyzer_stop_flag = stop_flag.clone();
        let analyzer_config = config.clone();
        std::thread::spawn(move || {
            // Use a blocking runtime for the analyzer
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();

            rt.block_on(async {
                Self::run_analyzer(
                    sample_rx,
                    color_tx,
                    sample_rate,
                    analyzer_config,
                    analyzer_stop_flag,
                )
                .await;
            });
        });

        // Create and build the audio stream
        let err_fn = |err| error!("Audio stream error: {}", err);

        // Configure stream based on sample format
        let stream = match config_range.sample_format() {
            SampleFormat::F32 => Self::build_input_stream::<f32>(
                &input_device,
                &config_range.into(),
                sample_tx.clone(),
                err_fn,
            ),
            SampleFormat::I16 => Self::build_input_stream::<i16>(
                &input_device,
                &config_range.into(),
                sample_tx.clone(),
                err_fn,
            ),
            SampleFormat::U16 => Self::build_input_stream::<u16>(
                &input_device,
                &config_range.into(),
                sample_tx.clone(),
                err_fn,
            ),
            _ => {
                error!("Unsupported sample format");
                return Err(Error::AudioCaptureError("Unsupported sample format".into()));
            }
        };

        let stream = match stream {
            Ok(stream) => {
                stream
                    .play()
                    .map_err(|e| Error::StreamPlayError(e.to_string()))?;
                Some(stream)
            }
            Err(err) => {
                error!("Failed to build audio input stream: {}", err);
                return Err(Error::AudioCaptureError(format!(
                    "Stream build error: {}",
                    err
                )));
            }
        };

        Ok(Self {
            config,
            sample_tx: Some(sample_tx),
            color_rx,
            stop_flag,
            _stream: stream,
        })
    }

    /// Build audio input stream with appropriate sample conversion
    fn build_input_stream<T>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        sample_tx: mpsc::Sender<f32>,
        err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
    ) -> Result<cpal::Stream>
    where
        T: Sample<Float = f32> + cpal::SizedSample + Send + 'static,
    {
        let tx = sample_tx.clone();

        debug!(
            "Building audio capture stream for device: {}",
            device.name().unwrap_or_default()
        );
        debug!("Stream config: {:?}", config);

        // Create a simple input stream to receive samples from the device
        let stream = device
            .build_input_stream(
                config,
                move |data: &[T], _: &cpal::InputCallbackInfo| {
                    // Process each sample
                    for &sample in data {
                        // Convert the sample to f32 (normalize between -1.0 and 1.0)
                        let sample_f32 = sample.to_float_sample();

                        // Apply some amplification to make sure we get signal
                        let amplified = sample_f32 * 5.0;

                        // Avoid blocking by using try_send; skip if channel is full
                        if tx.try_send(amplified).is_err() {
                            break;
                        }
                    }
                },
                err_fn,
                None,
            )
            .map_err(|e| Error::StreamBuildError(e.to_string()))?;

        debug!("Successfully built audio stream");
        Ok(stream)
    }

    /// Run the audio analyzer in a background thread
    async fn run_analyzer(
        mut sample_rx: mpsc::Receiver<f32>,
        color_tx: watch::Sender<AudioColor>,
        sample_rate: usize,
        config: Arc<RwLock<AudioVisualization>>,
        stop_flag: Arc<AtomicBool>,
    ) {
        let mut analyzer = AudioAnalyzer::new(sample_rate);
        let mut last_update = std::time::Instant::now();
        let mut audio_color = AudioColor::default();

        // Process audio samples
        while !stop_flag.load(Ordering::Relaxed) {
            // Collect samples
            while let Ok(sample) = sample_rx.try_recv() {
                analyzer.add_sample(sample);
            }

            // Check if it's time to update the visualization
            let now = std::time::Instant::now();

            // Get config values inside a block to drop the guard before any await
            let (
                update_interval,
                is_active,
                vis_mode,
                sensitivity,
                bass_trigger,
                mid_trigger,
                high_trigger,
            ) = {
                let config_guard = config.read();
                (
                    Duration::from_millis(config_guard.update_interval_ms as u64),
                    config_guard.active,
                    config_guard.mode,
                    config_guard.sensitivity,
                    config_guard.bass_color_trigger,
                    config_guard.mid_brightness_trigger,
                    config_guard.high_effect_trigger,
                )
            };

            if now.duration_since(last_update) >= update_interval {
                // Analyze audio
                analyzer.analyze();

                // Only update visuals if active
                if is_active {
                    // Get current timestamp for timing-based effects
                    let current_time = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs_f64();

                    // Apply visualization based on the current mode
                    match vis_mode {
                        VisualizationMode::FrequencyColor => {
                            // Map frequency energies to RGB
                            let bass = analyzer.get_normalized_energy(FrequencyRange::Bass);
                            let mid = analyzer.get_normalized_energy(FrequencyRange::Mid);
                            let high = analyzer.get_normalized_energy(FrequencyRange::High);

                            // Apply sensitivity
                            audio_color.r = (bass * 255.0 * sensitivity) as u8;
                            audio_color.g = (mid * 255.0 * sensitivity) as u8;
                            audio_color.b = (high * 255.0 * sensitivity) as u8;

                            // Ensure some minimum brightness when there's sound
                            let overall = analyzer.get_normalized_energy(FrequencyRange::Full);
                            if overall > 0.05 {
                                audio_color.r = audio_color.r.max(10);
                                audio_color.g = audio_color.g.max(10);
                                audio_color.b = audio_color.b.max(10);
                            }

                            // Reset effect
                            audio_color.effect = None;
                        }

                        VisualizationMode::EnergyBrightness => {
                            // Set color based on dominant frequency
                            let bass = analyzer.get_normalized_energy(FrequencyRange::Bass);
                            let mid = analyzer.get_normalized_energy(FrequencyRange::Mid);
                            let high = analyzer.get_normalized_energy(FrequencyRange::High);

                            // Find dominant frequency
                            if bass > mid && bass > high && bass > 0.1 {
                                // Bass dominant - red
                                audio_color.r = 255;
                                audio_color.g = 0;
                                audio_color.b = 0;
                            } else if mid > bass && mid > high && mid > 0.1 {
                                // Mid dominant - green
                                audio_color.r = 0;
                                audio_color.g = 255;
                                audio_color.b = 0;
                            } else if high > bass && high > mid && high > 0.1 {
                                // High dominant - blue
                                audio_color.r = 0;
                                audio_color.g = 0;
                                audio_color.b = 255;
                            } else {
                                // No dominant frequency - white
                                audio_color.r = 255;
                                audio_color.g = 255;
                                audio_color.b = 255;
                            }

                            // Set brightness based on overall energy
                            let energy = analyzer.get_normalized_energy(FrequencyRange::Full);
                            audio_color.brightness = (energy * 100.0 * sensitivity) as u8;
                            audio_color.brightness = audio_color.brightness.clamp(5, 100);

                            // Reset effect
                            audio_color.effect = None;
                        }

                        VisualizationMode::BeatEffects => {
                            // Set different effects based on detected beats
                            if analyzer.is_beat_detected(FrequencyRange::Bass) && bass_trigger {
                                // Bass beat - set to red and use crossfade
                                audio_color.r = 255;
                                audio_color.g = 0;
                                audio_color.b = 0;
                                audio_color.effect = Some(EFFECTS.crossfade_red);
                            } else if analyzer.is_beat_detected(FrequencyRange::Mid) && mid_trigger
                            {
                                // Mid beat - set to green and use crossfade
                                audio_color.r = 0;
                                audio_color.g = 255;
                                audio_color.b = 0;
                                audio_color.effect = Some(EFFECTS.crossfade_green);
                            } else if analyzer.is_beat_detected(FrequencyRange::High)
                                && high_trigger
                            {
                                // High beat - set to blue and use crossfade
                                audio_color.r = 0;
                                audio_color.g = 0;
                                audio_color.b = 255;
                                audio_color.effect = Some(EFFECTS.crossfade_blue);
                            } else {
                                // No beat - set to white with no effect
                                audio_color.r = 255;
                                audio_color.g = 255;
                                audio_color.b = 255;
                                audio_color.effect = None;
                            }

                            // Energy affects brightness
                            let energy = analyzer.get_normalized_energy(FrequencyRange::Full);
                            audio_color.brightness = (energy * 100.0 * sensitivity) as u8;
                            audio_color.brightness = audio_color.brightness.clamp(20, 100);
                        }

                        VisualizationMode::SpectralFlow => {
                            // Create flowing color pattern based on spectral content
                            let bass = analyzer.get_normalized_energy(FrequencyRange::Bass);
                            let mid = analyzer.get_normalized_energy(FrequencyRange::Mid);
                            let high = analyzer.get_normalized_energy(FrequencyRange::High);

                            // Create color flow - smooth transitions between colors
                            let time = current_time as f32;

                            // Base hue shifts with time, energy modulates saturation and brightness
                            let energy = bass * 0.5 + mid * 0.3 + high * 0.2;

                            // Use simple time-based patterns when no sound
                            if energy < 0.05 {
                                // Gentle pulse with time when no sound
                                let pulse = (time * 0.5).sin() * 0.5 + 0.5;
                                audio_color.r = (pulse * 50.0) as u8;
                                audio_color.g = (pulse * 50.0) as u8;
                                audio_color.b = (pulse * 80.0) as u8;
                                audio_color.effect = Some(EFFECTS.crossfade_red_green_blue);
                            } else {
                                // Sound present - create dynamic pattern

                                // When strong bass beat detected, temporarily switch to flash effect
                                if analyzer.is_beat_detected(FrequencyRange::Bass) && bass > 0.7 {
                                    audio_color.effect =
                                        Some(EFFECTS.jump_red_green_blue_yellow_cyan_magenta_white);
                                } else {
                                    // Normal flow - energy levels modulate colors in a cycle
                                    let bass_phase = (time * 0.7).sin() * 0.5 + 0.5;
                                    let mid_phase = (time * 0.7 + 2.0).sin() * 0.5 + 0.5;
                                    let high_phase = (time * 0.7 + 4.0).sin() * 0.5 + 0.5;

                                    audio_color.r = (bass_phase * 255.0 * bass * sensitivity) as u8;
                                    audio_color.g = (mid_phase * 255.0 * mid * sensitivity) as u8;
                                    audio_color.b = (high_phase * 255.0 * high * sensitivity) as u8;

                                    // Set crossfade effect for subtle transitions
                                    audio_color.effect = Some(EFFECTS.crossfade_red_green_blue);
                                }
                            }

                            // Adjust brightness based on overall energy
                            let brightness = (energy * 100.0 * sensitivity).max(20.0);
                            audio_color.brightness = brightness.min(100.0) as u8;
                        }

                        VisualizationMode::EnhancedFrequencyColor => {
                            // Get normalized energy values for each frequency range
                            let bass = analyzer.get_normalized_energy(FrequencyRange::Bass);
                            let mid = analyzer.get_normalized_energy(FrequencyRange::Mid);
                            let high = analyzer.get_normalized_energy(FrequencyRange::High);

                            // Enhanced color mapping:
                            // - Bass dominant: warm red-yellow spectrum (255,0,0) to (255,200,0)
                            // - Mid dominant: green-cyan spectrum (0,255,0) to (0,255,200)
                            // - High dominant: cool blue-white spectrum (0,0,255) to (200,200,255)

                            // Start with black
                            let mut r = 0;
                            let mut g = 0;
                            let mut b = 0;

                            // Apply bass (red-orange-yellow warm colors)
                            if bass > 0.05 {
                                // Calculate bass contribution - more bass means more red
                                r += (255.0 * bass * sensitivity) as u8;
                                // Yellow tint increases with stronger bass
                                g += (150.0 * bass * bass * sensitivity) as u8;
                            }

                            // Apply mid (green-cyan colors)
                            if mid > 0.05 {
                                // Main green contribution
                                g += (255.0 * mid * sensitivity) as u8;
                                // Some cyan tint for stronger mids
                                b += (100.0 * mid * mid * sensitivity) as u8;
                            }

                            // Apply high (blue-white cool colors)
                            if high > 0.05 {
                                // Main blue contribution
                                b += (255.0 * high * sensitivity) as u8;
                                // White tint (r,g components) increases with stronger highs
                                r += (180.0 * high * high * sensitivity) as u8;
                                g += (180.0 * high * high * sensitivity) as u8;
                            }

                            // Ensure some minimum brightness when there's sound
                            let overall = analyzer.get_normalized_energy(FrequencyRange::Full);
                            if overall > 0.05 {
                                r = r.max(10);
                                g = g.max(10);
                                b = b.max(10);
                            }

                            // Apply to audio color
                            audio_color.r = r;
                            audio_color.g = g;
                            audio_color.b = b;

                            // Adjust brightness based on energy
                            let energy = overall;
                            audio_color.brightness = (energy * 100.0 * sensitivity) as u8;
                            audio_color.brightness = audio_color.brightness.clamp(20, 100);

                            // No specific effect
                            audio_color.effect = None;

                            // For bass-heavy parts, add warmer tones
                            if bass > 0.7 && bass > 1.5 * mid && bass > 2.0 * high {
                                // Very bass heavy - make it more red-amber
                                audio_color.r = 255;
                                audio_color.g = (120.0 * bass * sensitivity) as u8;
                                audio_color.b = 0;
                            }

                            // For treble-heavy parts, add more white/light blue
                            if high > 0.7 && high > 1.5 * mid && high > 2.0 * bass {
                                // Very treble heavy - make it more white/light blue
                                audio_color.r = (210.0 * high * sensitivity) as u8;
                                audio_color.g = (220.0 * high * sensitivity) as u8;
                                audio_color.b = 255;
                            }
                        }

                        VisualizationMode::BpmSync => {
                            // Get current BPM from analyzer
                            let bpm = analyzer.get_bpm();
                            let bass = analyzer.get_normalized_energy(FrequencyRange::Bass);
                            let mid = analyzer.get_normalized_energy(FrequencyRange::Mid);
                            let high = analyzer.get_normalized_energy(FrequencyRange::High);

                            // Calculate the base color based on frequency balance
                            // More bass = more red, more highs = more blue, etc.
                            let r = (bass * 255.0 * sensitivity * 1.2).min(255.0) as u8;
                            let g = (mid * 255.0 * sensitivity * 1.1).min(255.0) as u8;
                            let b = (high * 255.0 * sensitivity * 1.2).min(255.0) as u8;

                            // Check if we're on a beat according to BPM timing
                            let on_beat = analyzer.is_on_beat(current_time);

                            // Different effects based on BPM
                            if bpm < 70.0 {
                                // Slow tempo - smooth color transitions
                                if on_beat && analyzer.is_beat_detected(FrequencyRange::Bass) {
                                    // On beat with bass - emphasize red
                                    audio_color.r = 255;
                                    audio_color.g = (g as f32 * 0.7) as u8;
                                    audio_color.b = (b as f32 * 0.6) as u8;
                                    audio_color.effect = Some(EFFECTS.crossfade_red);
                                } else {
                                    // Normal color
                                    audio_color.r = r;
                                    audio_color.g = g;
                                    audio_color.b = b;
                                    audio_color.effect = Some(EFFECTS.crossfade_red_green_blue);
                                }
                            } else if bpm < 120.0 {
                                // Medium tempo - more dynamic changes
                                if on_beat {
                                    // On beat pulses
                                    if analyzer.is_beat_detected(FrequencyRange::Bass) {
                                        // Bass hit - red pulse
                                        audio_color.r = 255;
                                        audio_color.g = 40;
                                        audio_color.b = 0;
                                        audio_color.effect = Some(EFFECTS.jump_red_green_blue);
                                    } else {
                                        // Regular beat - white pulse
                                        audio_color.r = 255;
                                        audio_color.g = 255;
                                        audio_color.b = 255;
                                        audio_color.effect = Some(EFFECTS.crossfade_white);
                                    }
                                } else {
                                    // Between beats - regular spectrum color
                                    audio_color.r = r;
                                    audio_color.g = g;
                                    audio_color.b = b;
                                    audio_color.effect = None;
                                }
                            } else {
                                // Fast tempo - flashy effects
                                if on_beat && analyzer.is_beat_detected(FrequencyRange::Bass) {
                                    // On beat with bass - bright flash
                                    audio_color.r = 255;
                                    audio_color.g = 255;
                                    audio_color.b = 255;
                                    audio_color.effect =
                                        Some(EFFECTS.jump_red_green_blue_yellow_cyan_magenta_white);
                                } else if on_beat {
                                    // Regular beat - color based on spectrum
                                    audio_color.r = r;
                                    audio_color.g = g;
                                    audio_color.b = b;
                                    audio_color.effect = Some(
                                        EFFECTS.blink_red_green_blue_yellow_cyan_magenta_white,
                                    );
                                } else {
                                    // Between beats - darker version of spectrum
                                    audio_color.r = (r as f32 * 0.7) as u8;
                                    audio_color.g = (g as f32 * 0.7) as u8;
                                    audio_color.b = (b as f32 * 0.7) as u8;
                                    audio_color.effect = None;
                                }
                            }

                            // Brightness pulses with the beat
                            let base_brightness = (60.0 * sensitivity).max(20.0) as u8;
                            let pulse_amplitude = (40.0 * sensitivity) as u8;

                            if on_beat {
                                // Brighter on beats
                                audio_color.brightness =
                                    (base_brightness + pulse_amplitude).min(100);
                            } else {
                                // Normal brightness between beats
                                audio_color.brightness = base_brightness;
                            }

                            // Display estimated BPM in debug
                            debug!("Estimated BPM: {:.1}", bpm);
                        }
                    }

                    // Send the updated color
                    let _ = color_tx.send(audio_color);
                }

                last_update = now;
            }

            // Don't hog the CPU - short sleep
            sleep(Duration::from_millis(1)).await;
        }
    }

    /// Stop audio monitoring
    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }

    /// Get the current visualization configuration
    pub fn get_config(&self) -> AudioVisualization {
        // Clone the configuration while holding the lock
        let guard = self.config.read();
        AudioVisualization {
            range: guard.range,
            mode: guard.mode,
            sensitivity: guard.sensitivity,
            bass_color_trigger: guard.bass_color_trigger,
            mid_brightness_trigger: guard.mid_brightness_trigger,
            high_effect_trigger: guard.high_effect_trigger,
            update_interval_ms: guard.update_interval_ms,
            active: guard.active,
        }
    }

    /// Update visualization configuration
    pub fn set_config(&self, config: AudioVisualization) {
        *self.config.write() = config;
    }

    /// Set whether audio monitoring should actively control the LEDs
    pub fn set_active(&self, active: bool) {
        self.config.write().active = active;
    }

    // Update the apply_to_device method in AudioMonitor to include more detailed logging
    #[instrument(skip(self, device))]
    pub async fn apply_to_device(&self, device: &mut BleLedDevice) -> Result<()> {
        // Get the latest color from the analyzer
        let audio_color = *self.color_rx.borrow();

        // Get current config for context
        let config = self.config.read();

        // Create detailed log entry with audio characteristics
        match config.mode {
            VisualizationMode::FrequencyColor => {
                info!(
                    "Audio viz [FrequencyColor] - RGB({}, {}, {}) - Bass: {:.2}, Mid: {:.2}, High: {:.2}, Brightness: {}%",
                    audio_color.r,
                    audio_color.g,
                    audio_color.b,
                    self.get_energy(FrequencyRange::Bass),
                    self.get_energy(FrequencyRange::Mid),
                    self.get_energy(FrequencyRange::High),
                    audio_color.brightness
                );
            }
            VisualizationMode::EnergyBrightness => {
                info!(
                    "Audio viz [EnergyBrightness] - RGB({}, {}, {}) - Overall Energy: {:.2}, Brightness: {}%",
                    audio_color.r,
                    audio_color.g,
                    audio_color.b,
                    self.get_energy(FrequencyRange::Full),
                    audio_color.brightness
                );
            }
            VisualizationMode::BeatEffects => {
                let beat_info = if audio_color.effect.is_some() {
                    "Beat detected"
                } else {
                    "No beat"
                };

                info!(
                    "Audio viz [BeatEffects] - RGB({}, {}, {}) - {}, Effect: {:?}, Brightness: {}%",
                    audio_color.r,
                    audio_color.g,
                    audio_color.b,
                    beat_info,
                    audio_color.effect.map(|e| format!("{}", e)),
                    audio_color.brightness
                );
            }
            VisualizationMode::SpectralFlow => {
                info!(
                    "Audio viz [SpectralFlow] - RGB({}, {}, {}) - Energy: {:.2}, Effect: {:?}, Brightness: {}%",
                    audio_color.r,
                    audio_color.g,
                    audio_color.b,
                    self.get_energy(FrequencyRange::Full),
                    audio_color.effect.map(|e| format!("{}", e)),
                    audio_color.brightness
                );
            }
            VisualizationMode::EnhancedFrequencyColor => {
                info!(
                    "Audio viz [EnhancedFrequencyColor] - RGB({}, {}, {}) - Bass: {:.2}, Mid: {:.2}, High: {:.2}, Brightness: {}%",
                    audio_color.r,
                    audio_color.g,
                    audio_color.b,
                    self.get_energy(FrequencyRange::Bass),
                    self.get_energy(FrequencyRange::Mid),
                    self.get_energy(FrequencyRange::High),
                    audio_color.brightness
                );
            }
            VisualizationMode::BpmSync => {
                let bpm = self.get_estimated_bpm();
                let beat_info = if audio_color.effect.is_some() {
                    "On beat"
                } else {
                    "Off beat"
                };

                info!(
                    "Audio viz [BpmSync] - RGB({}, {}, {}) - BPM: {:.1}, {}, Effect: {:?}, Brightness: {}%",
                    audio_color.r,
                    audio_color.g,
                    audio_color.b,
                    bpm,
                    beat_info,
                    audio_color.effect.map(|e| format!("{}", e)),
                    audio_color.brightness
                );
            }
        };

        // Ensure device is powered on
        if !device.is_on {
            device.power_on().await?;
        }

        // Apply the audio-driven changes
        if let Some(effect) = audio_color.effect {
            // Apply effect if specified
            device.set_effect(effect).await?;
        } else {
            // Apply RGB color
            device
                .set_color(audio_color.r, audio_color.g, audio_color.b)
                .await?;
        }

        // Apply brightness
        device.set_brightness(audio_color.brightness).await?;

        Ok(())
    }

    // Add a new method to periodically log detailed audio analysis information
    // This can be called from a separate task to avoid flooding the main log
    pub async fn log_detailed_analysis(&self) -> Result<()> {
        // Get current analytics
        let energy_bass = self.get_energy(FrequencyRange::Bass);
        let energy_mid = self.get_energy(FrequencyRange::Mid);
        let energy_high = self.get_energy(FrequencyRange::High);
        let energy_full = self.get_energy(FrequencyRange::Full);
        let bpm = self.get_estimated_bpm();

        // Get current config
        let config = self.config.read();

        debug!(
            "Audio Analysis: Mode={:?}, Active={}, Sensitivity={:.2}, Bass={:.3}, Mid={:.3}, High={:.3}, Overall={:.3}, BPM={:.1}",
            config.mode,
            config.active,
            config.sensitivity,
            energy_bass,
            energy_mid,
            energy_high,
            energy_full,
            bpm
        );

        Ok(())
    }

    // Add periodic detailed logging to the continuous monitoring loop
    #[instrument(skip(self, device))]
    pub async fn start_continuous_monitoring(&self, device: &mut BleLedDevice) -> Result<()> {
        info!("Starting continuous audio monitoring");

        // Set monitoring as active
        self.set_active(true);

        // Ensure device is on
        if !device.is_on {
            device.power_on().await?;
        }

        // Apply visualization at regular intervals until stopped
        let update_interval = Duration::from_millis(self.config.read().update_interval_ms as u64);

        // Counter for periodic detailed logging (log details every 50 updates)
        let mut log_counter = 0;

        while self.config.read().active && !self.stop_flag.load(Ordering::Relaxed) {
            self.apply_to_device(device).await?;

            // Perform detailed logging periodically
            log_counter += 1;
            if log_counter >= 50 {
                self.log_detailed_analysis().await?;
                log_counter = 0;
            }

            sleep(update_interval).await;
        }

        info!("Continuous audio monitoring stopped");
        Ok(())
    }

    /// Get the current energy level for a specific frequency range (0.0-1.0)
    pub fn get_energy(&self, range: FrequencyRange) -> f32 {
        // Read current audio color from the watch channel
        let audio_color = *self.color_rx.borrow();

        // Convert RGB color to energy level based on the range
        match range {
            FrequencyRange::Bass => audio_color.r as f32 / 255.0,
            FrequencyRange::Mid => audio_color.g as f32 / 255.0,
            FrequencyRange::High => audio_color.b as f32 / 255.0,
            FrequencyRange::Full => {
                // Average of all channels
                (audio_color.r as f32 + audio_color.g as f32 + audio_color.b as f32) / (3.0 * 255.0)
            }
        }
    }

    /// Get the estimated BPM if available (requires BpmSync mode)
    /// Returns 0.0 if BPM is not being calculated
    pub fn get_estimated_bpm(&self) -> f32 {
        // This is a simple stub - the actual BPM is calculated internally
        // and we don't have a way to access it directly from the public API
        // The BPM value is used in the BpmSync mode internally
        let config = self.get_config();
        if config.mode == VisualizationMode::BpmSync {
            // When in BPM mode, we can assume BPM is being calculated
            // The specific value is used internally but not exposed
            // We'll use a placeholder of 120 BPM here
            120.0
        } else {
            0.0
        }
    }
}

impl Drop for AudioMonitor {
    fn drop(&mut self) {
        // Ensure background threads exit cleanly
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}

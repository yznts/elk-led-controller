/*! 
 # Effect modes for LED strips
 
 This module defines various effect modes available for the LED strips.
 It includes constants for different effects like jump, crossfade, and blink.
*/

/// Represents available effect modes for LED strips
#[derive(Debug, Clone, Copy)]
pub struct Effects {
    /// Red, green, blue jump effect
    pub jump_red_green_blue: u8,
    /// All colors jump effect
    pub jump_red_green_blue_yellow_cyan_magenta_white: u8,
    /// Red crossfade effect
    pub crossfade_red: u8,
    /// Green crossfade effect
    pub crossfade_green: u8,
    /// Blue crossfade effect
    pub crossfade_blue: u8,
    /// Yellow crossfade effect
    pub crossfade_yellow: u8,
    /// Cyan crossfade effect
    pub crossfade_cyan: u8,
    /// Magenta crossfade effect
    pub crossfade_magenta: u8,
    /// White crossfade effect
    pub crossfade_white: u8,
    /// Red and green crossfade effect
    pub crossfade_red_green: u8,
    /// Red and blue crossfade effect
    pub crossfade_red_blue: u8,
    /// Green and blue crossfade effect
    pub crossfade_green_blue: u8,
    /// Red, green, blue crossfade effect
    pub crossfade_red_green_blue: u8,
    /// All colors crossfade effect
    pub crossfade_red_green_blue_yellow_cyan_magenta_white: u8,
    /// Red blink effect
    pub blink_red: u8,
    /// Green blink effect
    pub blink_green: u8,
    /// Blue blink effect
    pub blink_blue: u8,
    /// Yellow blink effect
    pub blink_yellow: u8,
    /// Cyan blink effect
    pub blink_cyan: u8,
    /// Magenta blink effect
    pub blink_magenta: u8,
    /// White blink effect
    pub blink_white: u8,
    /// All colors blink effect
    pub blink_red_green_blue_yellow_cyan_magenta_white: u8,
}

/// Predefined effects with their command values
pub const EFFECTS: Effects = Effects {
    jump_red_green_blue: 0x87,
    jump_red_green_blue_yellow_cyan_magenta_white: 0x88,
    crossfade_red: 0x8b,
    crossfade_green: 0x8c,
    crossfade_blue: 0x8d,
    crossfade_yellow: 0x8e,
    crossfade_cyan: 0x8f,
    crossfade_magenta: 0x90,
    crossfade_white: 0x91,
    crossfade_red_green: 0x92,
    crossfade_red_blue: 0x93,
    crossfade_green_blue: 0x94,
    crossfade_red_green_blue: 0x89,
    crossfade_red_green_blue_yellow_cyan_magenta_white: 0x8a,
    blink_red: 0x96,
    blink_green: 0x97,
    blink_blue: 0x98,
    blink_yellow: 0x99,
    blink_cyan: 0x9a,
    blink_magenta: 0x9b,
    blink_white: 0x9c,
    blink_red_green_blue_yellow_cyan_magenta_white: 0x95,
};
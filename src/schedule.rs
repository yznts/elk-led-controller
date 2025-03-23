/*! 
 # Scheduling functionality for LED strips
 
 This module provides scheduling capabilities for the LED strips,
 allowing them to be turned on or off at specific days and times.
*/

/// Represents days of the week for scheduling
#[derive(Debug, Clone, Copy)]
pub struct Days {
    /// Monday (0x01)
    pub monday: u8,
    /// Tuesday (0x02)
    pub tuesday: u8,
    /// Wednesday (0x04)
    pub wednesday: u8,
    /// Thursday (0x08)
    pub thursday: u8,
    /// Friday (0x10)
    pub friday: u8,
    /// Saturday (0x20)
    pub saturday: u8,
    /// Sunday (0x40)
    pub sunday: u8,
    /// All days (0x7F)
    pub all: u8,
    /// Week days (Monday-Friday, 0x1F)
    pub week_days: u8,
    /// Weekend days (Saturday-Sunday, 0x60)
    pub weekend_days: u8,
    /// No days (0x00)
    pub none: u8,
}

/// Predefined day constants for scheduling
pub const WEEK_DAYS: Days = Days {
    monday: 0x01,
    tuesday: 0x02,
    wednesday: 0x04,
    thursday: 0x08,
    friday: 0x10,
    saturday: 0x20,
    sunday: 0x40,
    all: 0x01 + 0x02 + 0x04 + 0x08 + 0x10 + 0x20 + 0x40,
    week_days: 0x01 + 0x02 + 0x04 + 0x08 + 0x10,
    weekend_days: 0x20 + 0x40,
    none: 0x00,
};
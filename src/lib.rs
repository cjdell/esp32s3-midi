#![no_std]
#![feature(asm_experimental_arch, allocator_api, impl_trait_in_assoc_type)]
#![recursion_limit = "256"]

extern crate alloc;

pub mod http;
pub mod types;
pub mod utils;
pub mod wifi;

use embassy_usb::driver::EndpointError;
use embassy_usb_logger::ReceiverHandler;

// --- Helper macro: Make static with type safety ---
/// A safe replacement for `static_cell::make_static!` that works with known types.
#[macro_export]
macro_rules! make_static {
    ($t:ty, $val:expr) => {
        make_static!($t, $val,)
    };
    ($t:ty, $val:expr, $(#[$m:meta])*) => {{
        $(#[$m])*
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        STATIC_CELL.init($val)
    }};
}

// --- USB Logger Handler: Restart on 'r' ---
pub struct RestartHandler;

impl ReceiverHandler for RestartHandler {
    /// Handle incoming data from CDC-ACM serial port.
    /// If the first byte is 'r' (ASCII 114), trigger a software reset.
    async fn handle_data(&self, data: &[u8]) {
        if let Some(&first_byte) = data.first() {
            if first_byte == b'r' || first_byte == b'R' {
                log::info!("Received 'r' - Restarting device...");
                esp_hal::system::software_reset();
            }
        }
    }

    /// Required by trait — create a new instance.
    fn new() -> Self {
        RestartHandler
    }
}

// --- Custom Error for MIDI Disconnection ---
#[derive(Debug)]
pub struct Disconnected;

impl From<EndpointError> for Disconnected {
    fn from(err: EndpointError) -> Self {
        match err {
            EndpointError::BufferOverflow => panic!("MIDI buffer overflow"),
            EndpointError::Disabled => Disconnected, // Graceful disconnect
        }
    }
}

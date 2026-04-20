//! CDC-ACM Serial + MIDI over USB example using Embassy and ESP32-S3.
//!
//! This example creates a USB device that:
//! - Acts as a CDC-ACM serial port (for logging via USB)
//! - Acts as a USB MIDI device (to send Note On/Off messages)
//! - Uses a physical button (GPIO0) to trigger MIDI notes
//! - Supports restarting the device via serial command 'r'
//!
//! Wiring:
//! - DP (USB D+) => GPIO20
//! - DM (USB D-) => GPIO19
//! - Button => GPIO0 (pull-up)
//!
//! Build in release mode: `cargo esp flash --release`

#![no_std]
#![no_main]
#![feature(asm_experimental_arch)]

extern crate alloc;

use embassy_executor::Spawner;
use embassy_futures::join::join3;
use embassy_usb::{
    class::{
        cdc_acm::{CdcAcmClass, State},
        midi::MidiClass,
    },
    driver::EndpointError,
    Builder,
};
use embassy_usb_logger::{ReceiverHandler, UsbLogger};
use esp_backtrace as _;
use esp_hal::{
    gpio::{Input, InputConfig, Pull},
    interrupt::software::SoftwareInterruptControl,
    otg_fs::{
        asynch::{Config, Driver},
        Usb,
    },
    peripherals::Peripherals,
    timer::timg::TimerGroup,
};
use midi_convert::{
    midi_types::{Channel, MidiMessage, Note, Value7},
    render_slice::MidiRenderSlice,
};
use usbd_midi::{CableNumber, UsbMidiEventPacket};

// --- ESP-IDF Bootloader Descriptor ---
esp_bootloader_esp_idf::esp_app_desc!();

// --- MIDI Constants ---
const MIDI_NOTE: Note = Note::C3;
const MIDI_CHANNEL: Channel = Channel::C1;
const MIDI_VELOCITY_ON: u8 = 100;
const MIDI_VELOCITY_OFF: u8 = 0;

// --- USB Configuration ---
const USB_VENDOR_ID: u16 = 0x303A; // Espressif
const USB_PRODUCT_ID: u16 = 0x3001; // Custom device
const USB_MAX_PACKET_SIZE: u16 = 64;

// --- USB Logger Handler: Restart on 'r' ---
struct RestartHandler;

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
struct Disconnected;

impl From<EndpointError> for Disconnected {
    fn from(err: EndpointError) -> Self {
        match err {
            EndpointError::BufferOverflow => panic!("MIDI buffer overflow"),
            EndpointError::Disabled => Disconnected, // Graceful disconnect
        }
    }
}

// --- Main MIDI Task ---
/// Continuously waits for button press and sends MIDI Note On/Off.
async fn midi_task<'a>(midi_class: &mut MidiClass<'a, Driver<'a>>) -> Result<(), Disconnected> {
    // Borrow peripherals for button
    let p = unsafe { Peripherals::steal() };

    // Configure button on GPIO0 with pull-up
    let mut button = Input::new(p.GPIO0, InputConfig::default().with_pull(Pull::Up));

    log::info!("MIDI task started. Waiting for button...");

    loop {
        // Wait for button press (low = pressed)
        button.wait_for_low().await;
        log::info!("Button pressed → Note On");

        // Send Note On
        send_midi_note(midi_class, true).await?;

        // Wait for release (high = released)
        button.wait_for_high().await;
        log::info!("Button released → Note Off");

        // Send Note Off
        send_midi_note(midi_class, false).await?;
    }
}

/// Send a MIDI Note On/Off message using the USB MIDI class.
async fn send_midi_note<'a>(
    midi_class: &mut MidiClass<'a, Driver<'a>>,
    is_on: bool,
) -> Result<(), Disconnected> {
    // Construct MIDI message
    let message = if is_on {
        MidiMessage::NoteOn(MIDI_CHANNEL, MIDI_NOTE, Value7::from(MIDI_VELOCITY_ON))
    } else {
        MidiMessage::NoteOff(MIDI_CHANNEL, MIDI_NOTE, Value7::from(MIDI_VELOCITY_OFF))
    };

    // Render MIDI message into 3-byte payload
    let mut payload = [0u8; 3];
    message.render_slice(&mut payload);

    // Wrap in USB MIDI packet (Cable 0, 4-byte packet format)
    let packet = UsbMidiEventPacket::try_from_payload_bytes(CableNumber::Cable0, &payload)
        .expect("Invalid MIDI payload");

    // Send over USB MIDI
    midi_class.write_packet(packet.as_raw_bytes()).await?;

    log::debug!("MIDI sent: {:?}", packet);
    Ok(())
}

// --- Main Entry Point ---
#[esp_rtos::main]
async fn main(spawner: Spawner) {
    // Initialize hardware
    let peripherals = esp_hal::init(esp_hal::Config::default());

    // Allocate heap memory for dynamic allocations
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 72 * 1024);

    // Initialize timer and software interrupt for RTOS
    let sw_int = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);

    // Initialize USB hardware
    let usb = Usb::new(
        peripherals.USB0,
        peripherals.GPIO20, // DP
        peripherals.GPIO19, // DM
    );

    // USB endpoint buffer for incoming data
    let mut ep_out_buffer = [0u8; 1024];
    let usb_config = Config::default();

    // Create USB driver (low-level)
    let driver = Driver::new(usb, &mut ep_out_buffer, usb_config);

    // Configure USB device descriptor
    let mut usb_config = embassy_usb::Config::new(USB_VENDOR_ID, USB_PRODUCT_ID);
    usb_config.manufacturer = Some("Espressif");
    usb_config.product = Some("USB-CDC+MIDI");
    usb_config.serial_number = Some("12345678");

    // Required for Windows compatibility (Composite device with IAD)
    usb_config.device_class = 0xEF; // Miscellanous Device
    usb_config.device_sub_class = 0x02; // Common Class
    usb_config.device_protocol = 0x01; // Interface Association Descriptor
    usb_config.composite_with_iads = true;

    // Buffers for USB descriptor generation
    let mut config_descriptor = [0; 256];
    let mut bos_descriptor = [0; 256];
    let mut control_buf = [0; 64];

    // State for CDC-ACM (serial) class
    let mut cdc_state = State::new();

    // Create USB builder
    let mut builder = Builder::new(
        driver,
        usb_config,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut [], // No MSOS descriptors
        &mut control_buf,
    );

    // Create USB classes
    let mut midi_class = MidiClass::new(&mut builder, 1, 1, USB_MAX_PACKET_SIZE); // 1 IN, 1 OUT, 64-byte max
    let cdc_class = CdcAcmClass::new(&mut builder, &mut cdc_state, USB_MAX_PACKET_SIZE);

    // Build USB device
    let mut usb_device = builder.build();

    // --- Start USB Device ---
    let usb_fut = usb_device.run();

    // --- Start MIDI Task ---
    let midi_fut = async {
        loop {
            midi_class.wait_connection().await;
            log::info!("MIDI device connected.");
            match midi_task(&mut midi_class).await {
                Ok(()) => log::info!("MIDI task ended gracefully."),
                Err(_) => log::warn!("MIDI connection lost."),
            }
            log::info!("MIDI device disconnected.");
        }
    };

    // --- Start USB Logger (CDC-ACM) ---
    // Create a logger that writes USB serial data to `log!` macros
    let logger = make_static!(
        UsbLogger<1024, RestartHandler>,
        UsbLogger::<1024, RestartHandler>::new()
    );

    // Assign the restart handler to handle 'r' commands
    logger.with_handler(RestartHandler::new());

    // Set global log target to USB logger
    log::set_logger(logger).ok();
    log::set_max_level(log::LevelFilter::Info);

    // Start the logger: this will block until USB is connected
    let logger_fut = async {
        log::info!("Waiting for USB CDC connection to enable logging...");
        logger.create_future_from_class(cdc_class).await;
        log::info!("USB CDC logging active.");
    };

    // Run all tasks concurrently
    join3(usb_fut, midi_fut, logger_fut).await;
}

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

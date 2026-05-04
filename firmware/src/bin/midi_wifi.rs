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
//! Build in release mode: `cargo run -r --bin midi_async`

#![no_std]
#![no_main]
#![feature(asm_experimental_arch, allocator_api, impl_trait_in_assoc_type)]
#![recursion_limit = "256"]

extern crate alloc;

use core::{net::Ipv4Addr, str::FromStr};
use embassy_executor::Spawner;
use embassy_futures::join::join4;
use embassy_futures::{join::join3, select::select};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embassy_usb::class::*;
use embassy_usb_logger::{ReceiverHandler, UsbLogger};
use firmware::*;
use firmware::{types::*, utils::*};
use esp_backtrace as _;
use esp_hal::{
    gpio::{AnyPin, Input, InputConfig, Pull},
    interrupt::software::SoftwareInterruptControl,
    otg_fs,
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
const MIDI_CHANNEL: Channel = Channel::C1;
const MIDI_VELOCITY_ON: u8 = 100;
const MIDI_VELOCITY_OFF: u8 = 0;

// --- USB Configuration ---
const USB_VENDOR_ID: u16 = 0x303A; // Espressif
const USB_PRODUCT_ID: u16 = 0x3001; // Custom device
const USB_MAX_PACKET_SIZE: u16 = 64;

// --- Main MIDI Task ---
/// Continuously waits for button press and sends MIDI Note On/Off.
async fn midi_task<'a>(
    midi_class: &mut midi::MidiClass<'a, otg_fs::asynch::Driver<'a>>,
    button: AnyPin<'a>,
    web_socket_incoming_receiver: WebSocketIncomingReceiver,
) -> Result<(), Disconnected> {
    let midi_mutex = Mutex::<CriticalSectionRawMutex, _>::new(midi_class);

    // Configure button on GPIO0 with pull-up
    let mut button = Input::new(button, InputConfig::default().with_pull(Pull::Up));

    log::info!("MIDI task started. Waiting for button...");

    loop {
        either_into_result::<_, Disconnected>(
            select(
                // Button task
                async {
                    button.wait_for_low().await;
                    log::info!("Button pressed → Note On");
                    send_midi_note(*midi_mutex.lock().await, Note::C3, true).await?;

                    button.wait_for_high().await;
                    log::info!("Button released → Note Off");
                    send_midi_note(*midi_mutex.lock().await, Note::C3, false).await?;

                    Ok(())
                },
                // WebSocket task
                async {
                    match web_socket_incoming_receiver.receive().await {
                        WebSocketIncomingMessage::NoteOn(n) => {
                            send_midi_note(*midi_mutex.lock().await, Note::new(n), true).await?;
                        }
                        WebSocketIncomingMessage::NoteOff(n) => {
                            send_midi_note(*midi_mutex.lock().await, Note::new(n), false).await?;
                        }
                    }

                    // send_midi_note(*midi_mutex.lock().await, Note::C4, true).await?;

                    // utils::sleep(1_000).await;

                    Ok(())
                },
            )
            .await,
        )?;
    }
}

/// Send a MIDI Note On/Off message using the USB MIDI class.
async fn send_midi_note<'a>(
    midi_class: &mut midi::MidiClass<'a, otg_fs::asynch::Driver<'a>>,
    node: Note,
    is_on: bool,
) -> Result<(), Disconnected> {
    // Construct MIDI message
    let message = if is_on {
        MidiMessage::NoteOn(MIDI_CHANNEL, node, Value7::from(MIDI_VELOCITY_ON))
    } else {
        MidiMessage::NoteOff(MIDI_CHANNEL, node, Value7::from(MIDI_VELOCITY_OFF))
    };

    // Render MIDI message into 3-byte payload
    let mut payload = [0u8; 3];
    message.render_slice(&mut payload);

    // Wrap in USB MIDI packet (Cable 0, 4-byte packet format)
    let packet =
        UsbMidiEventPacket::try_from_payload_bytes(CableNumber::Cable0, &payload).expect("Invalid MIDI payload");

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
    esp_alloc::heap_allocator!(size: 128 * 1024);
    esp_alloc::psram_allocator!(peripherals.PSRAM, esp_hal::psram);

    // Initialize timer and software interrupt for RTOS
    let sw_int = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);

    let web_socket_incoming_channel = make_static!(WebSocketIncomingChannel, WebSocketIncomingChannel::new());

    // Initialize USB hardware
    let usb = otg_fs::Usb::new(
        peripherals.USB0,
        peripherals.GPIO20, // DP
        peripherals.GPIO19, // DM
    );

    // USB endpoint buffer for incoming data
    let mut ep_out_buffer = [0u8; 1024];
    let usb_config = otg_fs::asynch::Config::default();

    // Create USB driver (low-level)
    let driver = otg_fs::asynch::Driver::new(usb, &mut ep_out_buffer, usb_config);

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
    let mut cdc_state = cdc_acm::State::new();

    // Create USB builder
    let mut builder = embassy_usb::Builder::new(
        driver,
        usb_config,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut [], // No MSOS descriptors
        &mut control_buf,
    );

    // Create USB classes
    let mut midi_class = midi::MidiClass::new(&mut builder, 1, 1, USB_MAX_PACKET_SIZE); // 1 IN, 1 OUT, 64-byte max
    let cdc_class = cdc_acm::CdcAcmClass::new(&mut builder, &mut cdc_state, USB_MAX_PACKET_SIZE);

    // Build USB device
    let mut usb_device = builder.build();

    // --- Start USB Device ---
    let usb_fut = usb_device.run();

    // --- Start MIDI Task ---
    let midi_fut = async {
        loop {
            midi_class.wait_connection().await;
            log::info!("MIDI device connected.");

            let button = unsafe { peripherals.GPIO0.clone_unchecked() };

            match midi_task(&mut midi_class, button.into(), web_socket_incoming_channel.receiver()).await {
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
        logger.create_future_from_class(cdc_class).await; // Never returns
    };

    let wifi_fut = async {
        sleep(2_000).await;

        log::info!("Starting...");

        let rng = esp_hal::rng::Rng::new();
        let seed = (rng.random() as u64) << 32 | rng.random() as u64;

        let (controller, interfaces) = esp_radio::wifi::new(peripherals.WIFI, Default::default()).unwrap();

        // Init network stack
        let (stack, runner) = embassy_net::new(
            interfaces.access_point,
            embassy_net::Config::dhcpv4(Default::default()),
            make_static!(embassy_net::StackResources<8>, embassy_net::StackResources::<8>::new()),
            seed,
        );

        let ap_ip = Ipv4Addr::from_str("192.168.1.1").expect("Failed to parse AP IP!");

        spawner.spawn(wifi::connection_task(controller, stack, ap_ip).unwrap());
        spawner.spawn(wifi::net_task(runner).unwrap());
        spawner.spawn(wifi::dhcp_task(stack, ap_ip).unwrap());
        spawner.spawn(wifi::captive_task(stack, ap_ip).unwrap());

        http::start_http(spawner, stack, web_socket_incoming_channel.sender());
    };

    // Run all tasks concurrently
    join4(usb_fut, midi_fut, logger_fut, wifi_fut).await;
}

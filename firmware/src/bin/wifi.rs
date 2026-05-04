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
use esp_hal::clock::CpuClock;
use esp_hal::{
    gpio::{AnyPin, Input, InputConfig, Pull},
    interrupt::software::SoftwareInterruptControl,
    otg_fs,
    timer::timg::TimerGroup,
};
use esp_println as _;
use midi_convert::{
    midi_types::{Channel, MidiMessage, Note, Value7},
    render_slice::MidiRenderSlice,
};
use usbd_midi::{CableNumber, UsbMidiEventPacket};

// --- ESP-IDF Bootloader Descriptor ---
esp_bootloader_esp_idf::esp_app_desc!();

// --- Main Entry Point ---
#[esp_rtos::main]
async fn main(spawner: Spawner) {
    esp_println::logger::init_logger(log::LevelFilter::Info);

    // Initialize hardware
    // Some basic setup to run the MCU at maximum clock speed.
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::_240MHz);
    let peripherals = esp_hal::init(config);

    log::info!("Init!");
    esp_println::println!("Init!!!");

    // Allocate heap memory for dynamic allocations
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 64 * 1024);
    esp_alloc::heap_allocator!(size: 128 * 1024);
    esp_alloc::psram_allocator!(peripherals.PSRAM, esp_hal::psram);

    // Initialize timer and software interrupt for RTOS
    let sw_int = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);

    sleep(1_000).await;

    log::info!("Timer working!");

    let web_socket_incoming_channel = make_static!(WebSocketIncomingChannel, WebSocketIncomingChannel::new());

    log::info!("Waiting...");

    sleep(5_000).await;

    log::info!("Starting...");

    let rng = esp_hal::rng::Rng::new();
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;

    let access_point_config =
        esp_radio::wifi::Config::AccessPoint(esp_radio::wifi::ap::AccessPointConfig::default().with_ssid("esp-radio"));

    let (controller, interfaces) = esp_radio::wifi::new(
        peripherals.WIFI,
        esp_radio::wifi::ControllerConfig::default().with_initial_config(access_point_config),
    )
    .unwrap();

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
    // spawner.spawn(wifi::dhcp_task(stack, ap_ip).unwrap());
    // spawner.spawn(wifi::captive_task(stack, ap_ip).unwrap());

    // http::start_http(spawner, stack, web_socket_incoming_channel.sender());
}

# 🎹 MIDI over USB on ESP32-S3 with Embassy (Async Rust)

> Send MIDI Note On/Off events via USB from your ESP32-S3 using async Rust and Embassy — no OS required.

This example demonstrates how to build a **USB MIDI device** on the ESP32-S3 using **async Embassy**, with **serial logging** via CDC-ACM and **button-triggered MIDI notes**. Press the onboard **BOOT button** to send a MIDI note (C3) to any host device like GarageBand, Ableton, or DAWs.

---

## ✨ Features

| Feature | Description |
|--------|-------------|
| 🎵 **USB MIDI Device** | Sends `Note On` and `Note Off` messages over USB MIDI (Cable 0) |
| 🔌 **CDC-ACM Serial Logging** | Real-time logs via USB serial port (`log!` macros) |
| 🎛️ **Button Triggered** | Uses the ESP32-S3’s built-in `BOOT` button (GPIO0) |
| ⚡ **Async-First** | Clean, readable async code with `await` — no busy loops or RTOS tasks |
| 📦 **No External Libraries** | Uses only `embassy`, `esp-hal`, and `midi-convert` |

---

## 📁 Project Structure

```bash
.
├── midi_async.rs        # ✅ Recommended: Async Embassy version (this example)
├── midi.rs              # 🚫 Legacy: Blocking (non-async) version for comparison
├── README.md            # You're here!
└── Cargo.toml           # Dependencies and build config
```

> 💡 **Use `midi_async.rs`** — it’s modern, efficient, and scales beautifully.

---

## 🤔 Why Use Async?

Async Rust lets you write **clean, sequential logic** that feels like blocking code — but runs concurrently without threads or RTOS overhead.

```rust
loop {
    button.wait_for_low().await;        // Wait for button press — no polling!
    log::info!("Button pressed → Note On");
    send_midi_note(midi_class, true).await?;

    button.wait_for_high().await;       // Wait for release — natural flow!
    log::info!("Button released → Note Off");
    send_midi_note(midi_class, false).await?;
}
```

✅ No interrupts.  
✅ No state machines.  
✅ No `while !button.is_pressed()` loops.  
✅ Just **readable, linear logic** that the compiler optimizes into efficient async code.

---

## 🛠️ First-Time Setup

Make sure you have the right tools installed:

```bash
# Install Rust toolchain and ESP tooling
sudo apt update && sudo apt install -y rustup tio

rustup install stable
cargo install espup

# Install ESP-IDF toolchain + Rust tooling
espup install
```

> 💡 If you haven't already, set up your environment variables by sourcing the ESP profile:
```bash
source ~/export-esp.sh
```

> 🔧 **Pro Tip**: Add this line to your `~/.bashrc` or `~/.zshrc` to auto-load it:
> ```bash
> source ~/export-esp.sh
> ```

---

## ▶️ Running the Example

```bash
# Build and flash in release mode (required for USB performance!)
cargo run --release --bin midi_async

# Or if you prefer to flash manually:
cargo build --release --bin midi_async
espflash --port /dev/ttyUSB0 target/esp32s3-esp-elf/release/midi_async.bin
```

### 🔌 Connect & Test

1. Plug your ESP32-S3 into your computer via USB.
2. Wait 2–3 seconds for the device to enumerate.
3. Open a **MIDI-compatible app**:
   - **macOS**: GarageBand, Logic Pro, MIDI Monitor
   - **Linux**: `aconnect -i` + `qjackctl` or `midisnoop`
   - **Windows**: MIDI-OX, VCV Rack

> ✅ You should see **MIDI events** appear when you press the **BOOT button** (labeled `BOOT` or `GPIO0` on your board).

### 📜 View Serial Logs (Debugging)

The device also creates a **virtual serial port (CDC-ACM)** for logs.

```bash
tio /dev/ttyUSB0
```

> ⚠️ If `/dev/ttyUSB0` doesn’t work, try `ttyUSB1`, `ttyACM0`, or use:
> ```bash
> ls /dev/tty* | grep -E "(USB|ACM)"
> ```

Logs will show:
```
[INFO] MIDI device connected.
[INFO] Button pressed → Note On
[DEBUG] MIDI sent: UsbMidiEventPacket { ... }
[INFO] Button released → Note Off
```

> 💡 To **restart the device**, type `r` + Enter in the serial terminal!

---

## 🧩 How It Works

| Component | Role |
|---------|------|
| **USB MIDI Class** | Implements USB MIDI protocol over bulk endpoints. |
| **CDC-ACM Class** | Provides serial logging via USB (like Arduino Serial). |
| **Button (GPIO0)** | Pulled high; goes low when pressed. |
| **`midi-convert`** | Converts Rust MIDI messages (`NoteOn`, etc.) into raw 3-byte MIDI data. |
| **`usbd-midi`** | Wraps MIDI data into USB MIDI event packets (4-byte format). |
| **`embassy`** | Async USB stack — handles all USB protocol details for you. |

> 🔍 **No driver needed on host** — it’s a standard USB MIDI device!

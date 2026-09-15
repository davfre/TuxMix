//! `tuxmix-gui` — Graphical RME interface controller.
//!
//! ```bash
//! cargo run -p tuxmix-gui              # with real hardware
//! cargo run -p tuxmix-gui -- --mock    # simulation
//! cargo run -p tuxmix-gui -- --mock --osc   # + OSC control surface
//! cargo run -p tuxmix-gui -- --backend usb  # force libusb over the kernel driver
//! ```

mod app;
mod connection;
mod monitor;
mod layouts;
mod matrix;
mod osc;
mod scenes;
mod sidebar;
mod theme;
mod widgets;

use std::net::{IpAddr, Ipv4Addr};

use iced::window;
use iced::{Font, Size};

/// Inter (SIL OFL 1.1, see assets/fonts/Inter-LICENSE) replaces whatever
/// sans-serif the host system happens to default to — a plain-looking UI is
/// the single biggest thing separating this from a "finished" app, for very
/// little effort.
const INTER_REGULAR: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");

/// Value following a `--flag <value>` pair, if present.
fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

const HELP: &str = "TuxMix — RME mixer

Usage: tuxmix-gui [OPTIONS]

Options:
  -h, --help             Show this help and exit
  --mock                 Simulate a device without accessing hardware
  --backend alsa|usb     Select a backend (default: try ALSA, then USB)
  --osc                  Enable OSC control
  --osc-port PORT        OSC receive port (default: 9000)
  --osc-send-port PORT   OSC send port (default: 9001)
  --osc-host ADDRESS     OSC destination IP address (default: 127.0.0.1)

Without --mock, a failed connection shows a disconnected screen.
";

fn main() -> iced::Result {
    if std::env::args().any(|arg| arg == "--help" || arg == "-h") {
        print!("{HELP}");
        return Ok(());
    }
    env_logger::init();
    let args: Vec<String> = std::env::args().collect();
    let mock = args.iter().any(|a| a == "--mock");

    // `None` = auto-detect (ALSA/kernel-driver first, USB fallback — see
    // `DeviceHandle::open_real`). An unrecognized value falls back to
    // auto rather than silently opening nothing.
    let backend = arg_value(&args, "--backend").and_then(|v| match v.as_str() {
        "alsa" | "usb" => Some(v),
        other => {
            eprintln!("Unknown --backend value {other:?} (use alsa|usb), auto-detecting");
            None
        }
    });

    // Opt-in, loopback-only OSC control surface — see osc.rs. Off unless
    // `--osc` is passed, so enabling it is always a deliberate choice.
    let osc_config = args.iter().any(|a| a == "--osc").then(|| osc::OscConfig {
        recv_port: arg_value(&args, "--osc-port")
            .and_then(|v| v.parse().ok())
            .unwrap_or(9000),
        send_port: arg_value(&args, "--osc-send-port")
            .and_then(|v| v.parse().ok())
            .unwrap_or(9001),
        send_host: arg_value(&args, "--osc-host")
            .and_then(|v| v.parse::<IpAddr>().ok())
            .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST)),
    });

    iced::application(
        move || connection::State::new(mock, osc_config.clone(), backend.clone()),
        connection::update,
        connection::view,
    )
    .title(connection::title)
    .subscription(connection::subscription)
    .theme(|_state: &connection::State| iced::Theme::Dark)
    .font(INTER_REGULAR)
    .default_font(Font::with_name("Inter"))
    .window(window::Settings {
        size: Size::new(1280.0, 800.0),
        // No enforced minimum — the adaptive scale (down to `SCALE_MIN`)
        // plus per-row horizontal scroll and the page's vertical scroll
        // handle small windows fine now, so a fixed 960×600 floor is just
        // an outdated restriction on shrinking.
        ..window::Settings::default()
    })
    .run()
}

//! Smoke-test the PulseAudio backend against the running sound server.
//!
//! Run with: cargo run -p deckweaver-core --example pulse_probe
//! Optionally pass an app key and a delta to exercise the control path:
//!   cargo run -p deckweaver-core --example pulse_probe -- msedge -10

use std::time::Duration;

fn main() {
    let backend = deckweaver_core::PulseBackend::new();

    // Give the worker a moment to connect and take its first snapshot.
    std::thread::sleep(Duration::from_millis(600));

    println!("available: {}", backend.is_available());
    println!("--- apps ---");
    for app in backend.apps() {
        println!(
            "  key={:<24} name={:<28} vol={:>3}% muted={:<5} streams={:?} icon={:?}",
            app.key, app.name, app.volume, app.is_muted, app.indices, app.icon_name
        );
        println!("      device_id={}", app.device_id());
    }

    let mut args = std::env::args().skip(1);
    let (Some(key), Some(delta)) = (args.next(), args.next()) else {
        return;
    };
    let delta: i16 = delta.parse().expect("delta must be an integer");

    let before = backend.get(&key).map(|a| a.volume);
    println!("\nadjusting {key} by {delta} (was {before:?})");
    backend.adjust_volume(&key, delta);

    std::thread::sleep(Duration::from_millis(500));
    println!("after (snapshot): {:?}", backend.get(&key).map(|a| a.volume));

    // Re-read from a fresh backend so we are reading the server, not our own optimistic write.
    let verify = deckweaver_core::PulseBackend::new();
    std::thread::sleep(Duration::from_millis(600));
    println!("after (server):   {:?}", verify.get(&key).map(|a| a.volume));
}

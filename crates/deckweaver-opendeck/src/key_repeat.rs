//! Press-and-hold repeat for the volume keys.
//!
//! A volume key steps once as soon as it is pressed, then keeps stepping for as long as it is
//! held, so a long press ramps the level instead of needing one press per step. The release (or
//! the key leaving the deck) stops it.

use std::collections::HashMap;
use std::time::Duration;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use tokio::task::JoinHandle;

/// How long a key has to be held before it starts repeating, so an ordinary tap is one step.
const HOLD_DELAY: Duration = Duration::from_millis(400);
/// Default time between repeated steps while held (a little over two steps a second); each key
/// can override it with its "hold repeat speed" setting, within `MIN..=MAX`.
pub const DEFAULT_REPEAT_MS: u32 = 400;
pub const MIN_REPEAT_MS: u32 = 50;
pub const MAX_REPEAT_MS: u32 = 1000;
/// Upper bound on one hold. Twenty steps of 5% cover the whole range, so this only matters if a
/// release is lost; it stops a key from stepping forever in that case.
const MAX_REPEATS: u32 = 100;

static REPEATS: Lazy<Mutex<HashMap<String, JoinHandle<()>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Step once now, then keep stepping every `interval` after [`HOLD_DELAY`] until [`stop`] is
/// called for the same instance.
pub fn start<F>(instance_id: String, interval: Duration, step: F)
where
    F: Fn() + Send + Sync + 'static,
{
    step();
    let task = tokio::spawn(async move {
        tokio::time::sleep(HOLD_DELAY).await;
        let mut interval = tokio::time::interval(interval);
        for _ in 0..MAX_REPEATS {
            interval.tick().await;
            step();
        }
    });
    if let Some(previous) = REPEATS.lock().insert(instance_id, task) {
        previous.abort();
    }
}

/// Stop a hold started by [`start`]. Safe to call when nothing is running.
pub fn stop(instance_id: &str) {
    if let Some(task) = REPEATS.lock().remove(instance_id) {
        task.abort();
    }
}

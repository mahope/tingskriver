//! When a dictation counts as started: the pill says "ready" and the start
//! sound plays at the first non-empty audio buffer, not when the stream was
//! told to play (a Bluetooth microphone can take seconds).

use std::time::Duration;

/// A microphone that has delivered nothing after this long gets a message.
pub const NO_SOUND_AFTER: Duration = Duration::from_millis(1000);

/// Message for the settings page and the log (never contains dictated text).
pub const NO_SOUND_MESSAGE: &str =
    "Mikrofonen leverer ingen lyd. Tjek, at den rigtige mikrofon er valgt i indstillingerne for diktering.";

/// Short text for the pill.
pub const NO_SOUND_PILL: &str = "Ingen lyd fra mikrofonen";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Startup {
    /// No audio yet and still within the wait.
    Waiting,
    /// The first buffer has arrived.
    Ready,
    /// Nothing arrived within [`NO_SOUND_AFTER`].
    NoSound,
}

/// `since_open`: time since the stream started; `first_audio`: whether a
/// non-empty buffer has arrived.
pub fn startup(since_open: Duration, first_audio: bool) -> Startup {
    if first_audio {
        Startup::Ready
    } else if since_open >= NO_SOUND_AFTER {
        Startup::NoSound
    } else {
        Startup::Waiting
    }
}

/// Sounds only play for a dictation that became ready, and only when the user
/// switched them on and the volume is above zero.
pub fn should_play_sound(enabled: bool, volume: u32, ready: bool) -> bool {
    enabled && volume > 0 && ready
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_only_after_the_first_audio() {
        assert_eq!(startup(Duration::from_millis(0), false), Startup::Waiting);
        assert_eq!(startup(Duration::from_millis(999), false), Startup::Waiting);
        assert_eq!(startup(Duration::from_millis(40), true), Startup::Ready);
        // Late audio still counts as ready.
        assert_eq!(startup(Duration::from_millis(3000), true), Startup::Ready);
    }

    #[test]
    fn silence_for_a_second_gives_the_message() {
        assert_eq!(startup(NO_SOUND_AFTER, false), Startup::NoSound);
        assert!(NO_SOUND_MESSAGE.contains("Mikrofonen"));
    }

    #[test]
    fn no_sound_without_audio_or_when_off() {
        assert!(!should_play_sound(true, 50, false));
        assert!(!should_play_sound(false, 50, true));
        assert!(!should_play_sound(true, 0, true));
        assert!(should_play_sound(true, 50, true));
    }
}

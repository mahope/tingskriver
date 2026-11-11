//! External controls use the existing dictation control thread.
use super::*;

static MODE: RwLock<Option<String>> = RwLock::new(None);

pub(super) fn selected_mode(settings: &DictationSettings) -> Option<Mode> {
    let mut selected = MODE.write().ok()?;
    let id = selected.as_ref()?;
    let mode = settings
        .modes
        .modes
        .iter()
        .find(|mode| &mode.id == id)
        .cloned();
    if mode.is_none() {
        *selected = None;
    }
    mode
}

impl Service {
    pub fn select_external_mode(&self, name: &str) -> Result<(), String> {
        let settings = self.settings();
        let mode = settings
            .modes
            .modes
            .iter()
            .find(|mode| mode.name == name)
            .map(|mode| mode.id.clone())
            .ok_or_else(|| "Dikteringstilstanden findes ikke".to_string())?;
        *MODE
            .write()
            .map_err(|_| "Tilstanden kunne ikke vælges".to_string())? = Some(mode);
        Ok(())
    }

    #[cfg(windows)]
    pub fn cancel_external(&self) -> Result<(), String> {
        let trigger = self.trigger.lock().ok().and_then(|trigger| trigger.clone());
        trigger
            .ok_or_else(|| "Diktering er ikke klar".to_string())?
            .send((ChordSignal::Interrupted, Instant::now(), None))
            .map_err(|_| "Diktering kører ikke".to_string())
    }

    #[cfg(not(windows))]
    pub fn cancel_external(&self) -> Result<(), String> {
        Err("Diktering findes kun på Windows".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictation::modes::Output;

    #[test]
    fn external_mode_tracks_edits_and_clears_deleted_selection() {
        let mut settings = DictationSettings::default();
        let mut mode = settings.modes.modes[0].clone();
        *MODE.write().unwrap() = Some(mode.id.clone());

        mode.name = "Renamed mode".into();
        mode.language = Some("en".into());
        mode.model = Some("updated-model".into());
        mode.output = Output::InsertAndEnter {
            trigger: "send".into(),
        };
        settings.modes.modes[0] = mode.clone();
        assert_eq!(selected_mode(&settings), Some(mode.clone()));

        settings.modes.modes.clear();
        assert_eq!(selected_mode(&settings), None);
        settings.modes.modes.push(mode);
        assert_eq!(selected_mode(&settings), None);
    }
}

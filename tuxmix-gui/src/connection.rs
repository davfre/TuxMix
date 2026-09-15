//! Keep a failed hardware connection separate from the simulated mixer.
use iced::widget::{button, column, container, text};
use iced::{Element, Fill, Subscription, Task};

use crate::{app, osc::OscConfig};

pub struct State {
    mixer: Option<app::TuxMix>,
    mock: bool,
    osc_config: Option<OscConfig>,
    backend: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Retry,
    Mixer(app::Message),
}

impl State {
    pub fn new(mock: bool, osc_config: Option<OscConfig>, backend: Option<String>) -> Self {
        Self::open_with(mock, osc_config, backend, app::new)
    }

    fn open_with(
        mock: bool,
        osc_config: Option<OscConfig>,
        backend: Option<String>,
        open: impl FnOnce(bool, Option<OscConfig>, Option<String>) -> Option<app::TuxMix>,
    ) -> Self {
        Self {
            mixer: open(mock, osc_config.clone(), backend.clone()),
            mock,
            osc_config,
            backend,
        }
    }
}

pub fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::Retry if state.mixer.is_none() => {
            state.mixer = app::new(state.mock, state.osc_config.clone(), state.backend.clone());
            Task::none()
        }
        Message::Mixer(message) => state
            .mixer
            .as_mut()
            .map(|mixer| app::update(mixer, message).map(Message::Mixer))
            .unwrap_or_else(Task::none),
        Message::Retry => Task::none(),
    }
}

pub fn title(state: &State) -> String {
    match &state.mixer {
        Some(_) if state.mock => "TuxMix - Simulation".into(),
        Some(mixer) => app::title(mixer),
        None => "TuxMix - No device connected".into(),
    }
}

pub fn subscription(state: &State) -> Subscription<Message> {
    state
        .mixer
        .as_ref()
        .map(|mixer| app::subscription(mixer).map(Message::Mixer))
        .unwrap_or_else(Subscription::none)
}

pub fn view(state: &State) -> Element<'_, Message> {
    if let Some(mixer) = &state.mixer {
        return app::view(mixer).map(Message::Mixer);
    }
    container(
        column![
            text("No device connected").size(28),
            text("TuxMix could not open an RME device. Connect it and try again."),
            text("If it is already connected, check the driver and device access."),
            button("Retry connection").on_press(Message::Retry),
        ]
        .spacing(16),
    )
    .center_x(Fill)
    .center_y(Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_open_stays_disconnected_and_ignores_mixer_messages() {
        let mut state = State::open_with(false, None, Some("alsa".into()), |mock, _, backend| {
            assert!(!mock);
            assert_eq!(backend.as_deref(), Some("alsa"));
            None
        });
        assert!(state.mixer.is_none());
        assert_eq!(title(&state), "TuxMix - No device connected");
        let _ = update(&mut state, Message::Mixer(app::Message::Tick));
        let _ = update(&mut state, Message::Mixer(app::Message::SaveNow));
        assert!(state.mixer.is_none());
    }

    #[test]
    fn explicit_mock_opens_a_labelled_simulation() {
        let state = State::new(true, None, Some("alsa".into()));
        assert!(state.mixer.as_ref().unwrap().device.is_mock());
        assert_eq!(title(&state), "TuxMix - Simulation");
    }
}

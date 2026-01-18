use super::{InputMode, Rotation};

#[derive(Debug, Copy, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    EnterMode(InputMode),
    ClearStatus,
    ToggleHelp,
    Quit,
    SelectNext,
    SelectPrevious,
    AddFilter,
    RemoveFilter,
    ToggleBypass,
    ToggleMute,
    SelectIndex(usize),
    AdjustFrequency(Adjustment),
    AdjustGain(Adjustment),
    AdjustQ(Adjustment),
    AdjustPreamp(Adjustment),
    CycleFilterType(Rotation),
    CycleViewMode(Rotation),
    ExecuteCommand,
    CommandHistoryPrevious,
    CommandHistoryNext,
    DeleteCharBackward,
    DeleteCharForward,
    MoveCursorLeft,
    MoveCursorRight,
    MoveCursorHome,
    MoveCursorEnd,
    OpenAutoEq,
    CloseAutoEq,
    ApplyAutoEq,
    EnterAutoEqFilter,
    CycleAutoEqTarget(Rotation),
}

#[derive(Debug, Copy, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Adjustment {
    Multiplier(f64),
    Delta(f64),
    Set(f64),
}

impl Adjustment {
    pub fn apply(&self, value: f64) -> f64 {
        match *self {
            Adjustment::Multiplier(factor) => value * factor,
            Adjustment::Delta(delta) => value + delta,
            Adjustment::Set(new_value) => new_value,
        }
    }
}

impl Action {
    /// Returns a short description of the action for help text
    pub fn description(&self) -> Option<&'static str> {
        match self {
            Action::ToggleHelp => Some("help"),
            Action::Quit => Some("quit"),
            Action::SelectNext => Some("next"),
            Action::SelectPrevious => Some("prev"),
            Action::AddFilter => Some("add"),
            Action::RemoveFilter => Some("delete"),
            Action::ToggleBypass => Some("bypass"),
            Action::ToggleMute => Some("mute"),
            Action::SelectIndex(_) => Some("select"),
            Action::AdjustFrequency(_) => Some("freq"),
            Action::AdjustGain(Adjustment::Set(0.0)) => Some("zero gain"),
            Action::AdjustGain(_) => Some("gain"),
            Action::AdjustQ(_) => Some("Q"),
            Action::AdjustPreamp(_) => Some("preamp"),
            Action::CycleFilterType(..) => Some("cycle type"),
            Action::CycleViewMode(..) => Some("cycle view"),
            Action::EnterMode(mode) => match mode {
                InputMode::Eq => Some("normal mode"),
                InputMode::AutoEq => Some("autoeq mode"),
                InputMode::Command => Some("command mode"),
            },
            Action::OpenAutoEq => Some("autoeq"),
            Action::CloseAutoEq => Some("close autoeq"),
            Action::ApplyAutoEq => Some("apply"),
            Action::EnterAutoEqFilter => Some("filter"),
            Action::CycleAutoEqTarget(_) => Some("cycle target"),
            Action::ExecuteCommand
            | Action::ClearStatus
            | Action::CommandHistoryPrevious
            | Action::CommandHistoryNext
            | Action::DeleteCharBackward
            | Action::DeleteCharForward
            | Action::MoveCursorLeft
            | Action::MoveCursorRight
            | Action::MoveCursorHome
            | Action::MoveCursorEnd => None,
        }
    }
}

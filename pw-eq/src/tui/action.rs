use super::Rotation;

#[derive(Debug, Copy, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EqAction {
    Quit,
    ToggleHelp,
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
    OpenAutoEq,
    EnterCommandMode,
}

#[derive(Debug, Copy, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AutoEqAction {
    Quit,
    ToggleHelp,
    SelectNext,
    SelectPrevious,
    ApplyAutoEq,
    CycleTarget(Rotation),
    EnterFilterMode,
    EnterEqMode,
    EnterCommandMode,
}

#[derive(Debug, Copy, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CommandAction {
    ExecuteCommand,
    ExitCommandMode,
    CommandHistoryPrevious,
    CommandHistoryNext,
    DeleteCharBackward,
    DeleteCharForward,
    MoveCursorLeft,
    MoveCursorRight,
    MoveCursorHome,
    MoveCursorEnd,
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

impl EqAction {
    /// Returns a short description of the action for help text
    pub fn description(&self) -> Option<&'static str> {
        match self {
            EqAction::ToggleHelp => Some("help"),
            EqAction::Quit => Some("quit"),
            EqAction::SelectNext => Some("next"),
            EqAction::SelectPrevious => Some("prev"),
            EqAction::AddFilter => Some("add"),
            EqAction::RemoveFilter => Some("delete"),
            EqAction::ToggleBypass => Some("bypass"),
            EqAction::ToggleMute => Some("mute"),
            EqAction::SelectIndex(_) => Some("select"),
            EqAction::AdjustFrequency(_) => Some("freq"),
            EqAction::AdjustGain(Adjustment::Set(0.0)) => Some("zero gain"),
            EqAction::AdjustGain(_) => Some("gain"),
            EqAction::AdjustQ(_) => Some("Q"),
            EqAction::AdjustPreamp(_) => Some("preamp"),
            EqAction::CycleFilterType(..) => Some("cycle type"),
            EqAction::CycleViewMode(..) => Some("cycle view"),
            EqAction::OpenAutoEq => Some("autoeq"),
            EqAction::EnterCommandMode => None,
        }
    }
}

impl AutoEqAction {
    /// Returns a short description of the action for help text
    pub fn description(&self) -> Option<&'static str> {
        match self {
            AutoEqAction::Quit => Some("quit"),
            AutoEqAction::ToggleHelp => Some("help"),
            AutoEqAction::SelectNext => Some("next"),
            AutoEqAction::SelectPrevious => Some("prev"),
            AutoEqAction::ApplyAutoEq => Some("apply"),
            AutoEqAction::CycleTarget(_) => Some("cycle target"),
            AutoEqAction::EnterFilterMode => Some("filter"),
            AutoEqAction::EnterEqMode => Some("close"),
            AutoEqAction::EnterCommandMode => None,
        }
    }
}

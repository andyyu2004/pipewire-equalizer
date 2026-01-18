use super::Rotation;

#[derive(Debug, Copy, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NormalAction {
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
    CloseAutoEq,
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

impl NormalAction {
    /// Returns a short description of the action for help text
    pub fn description(&self) -> Option<&'static str> {
        match self {
            NormalAction::ToggleHelp => Some("help"),
            NormalAction::Quit => Some("quit"),
            NormalAction::SelectNext => Some("next"),
            NormalAction::SelectPrevious => Some("prev"),
            NormalAction::AddFilter => Some("add"),
            NormalAction::RemoveFilter => Some("delete"),
            NormalAction::ToggleBypass => Some("bypass"),
            NormalAction::ToggleMute => Some("mute"),
            NormalAction::SelectIndex(_) => Some("select"),
            NormalAction::AdjustFrequency(_) => Some("freq"),
            NormalAction::AdjustGain(Adjustment::Set(0.0)) => Some("zero gain"),
            NormalAction::AdjustGain(_) => Some("gain"),
            NormalAction::AdjustQ(_) => Some("Q"),
            NormalAction::AdjustPreamp(_) => Some("preamp"),
            NormalAction::CycleFilterType(..) => Some("cycle type"),
            NormalAction::CycleViewMode(..) => Some("cycle view"),
            NormalAction::OpenAutoEq => Some("autoeq"),
            NormalAction::EnterCommandMode => None,
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
            AutoEqAction::CloseAutoEq => Some("close"),
            AutoEqAction::EnterCommandMode => None,
        }
    }
}

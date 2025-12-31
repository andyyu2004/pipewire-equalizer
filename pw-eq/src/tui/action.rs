use super::{InputMode, Rotation};

#[derive(Debug, Copy, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    EnterMode { mode: InputMode },
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
    CycleFilterType { rotation: Rotation },
    CycleViewMode { rotation: Rotation },
}

#[derive(Debug, Copy, Clone, serde::Serialize, serde::Deserialize)]
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

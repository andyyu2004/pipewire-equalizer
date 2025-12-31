use super::Rotation;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    ClearStatus,
    ToggleHelp,
    Quit,
    SelectNext,
    SelectPrevious,
    AddFilter,
    RemoveFilter,
    SelectIndex(usize),
    AdjustFrequency { multiplier: f64 },
    AdjustGain { delta: f64 },
    AdjustQ { delta: f64 },
    AdjustPreamp { delta: f64 },
    CycleFilterType { rotation: Rotation },
    ToggleBypass,
    ToggleMute,
    CycleViewMode { rotation: Rotation },
}

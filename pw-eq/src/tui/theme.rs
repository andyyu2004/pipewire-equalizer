use ratatui::style::Color;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub(crate) struct Theme {
    pub background: Color,
    pub text: Color,
    pub selected_row: Color,

    pub index: Color,
    pub filter_type: Color,
    pub frequency: Color,
    pub gain_positive: Color,
    pub gain_negative: Color,
    pub gain_neutral: Color,
    pub q_value: Color,
    pub coefficients: Color,

    pub dimmed: Color,
    pub bypassed: Color,

    pub header: Color,
    pub footer: Color,
    pub help: Color,
    pub status_ok: Color,
    pub status_error: Color,
    pub chart: Color,
    pub border: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self::solarized_dark()
    }
}

impl Theme {
    pub fn solarized_dark() -> Self {
        Self {
            background: Color::Rgb(0x00, 0x2b, 0x36),
            text: Color::Rgb(0x83, 0x94, 0x96),
            selected_row: Color::Rgb(0x58, 0x6e, 0x75),

            index: Color::Rgb(0x83, 0x94, 0x96),
            filter_type: Color::Rgb(0x26, 0x8b, 0xd2),
            frequency: Color::Rgb(0x2a, 0xa1, 0x98),
            gain_positive: Color::Rgb(0x85, 0x99, 0x00),
            gain_negative: Color::Rgb(0xcb, 0x4b, 0x16),
            gain_neutral: Color::Rgb(0x58, 0x6e, 0x75),
            q_value: Color::Rgb(0xb5, 0x89, 0x00),
            coefficients: Color::Rgb(0x85, 0x99, 0x00),

            dimmed: Color::Rgb(0x58, 0x6e, 0x75),
            bypassed: Color::Rgb(0xb5, 0x89, 0x00),

            header: Color::Rgb(0x83, 0x94, 0x96),
            footer: Color::Rgb(0x58, 0x6e, 0x75),
            help: Color::Rgb(0x58, 0x6e, 0x75),
            status_ok: Color::Rgb(0x83, 0x94, 0x96),
            status_error: Color::Rgb(0xdc, 0x32, 0x2f),
            chart: Color::Rgb(0x2a, 0xa1, 0x98),
            border: Color::Rgb(0x58, 0x6e, 0x75),
        }
    }
}

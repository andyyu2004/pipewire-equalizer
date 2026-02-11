use dear_imgui_rs::Context;
use dear_imgui_glow::GlowRenderer;
use dear_imgui_winit::WinitPlatform;
use dear_implot::PlotContext;

use crate::{autoeq::AutoEqWindowState, filter::FilterWindowState};

pub struct ImguiState {
    pub renderer: GlowRenderer,
    pub platform: WinitPlatform,
    pub context: Context,
    pub plot_context: PlotContext,
    pub clear_color: [f32; 4],
    pub last_frame: std::time::Instant,

    pub auto_eq: AutoEqWindowState,
    pub filter: FilterWindowState,
}
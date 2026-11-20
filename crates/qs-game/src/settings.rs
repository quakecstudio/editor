// SPDX-License-Identifier: GPL-3.0-or-later

//! The client's settings as the preview host reads them each frame (CON-7.4): what the console
//! and the options menu set, turned into what the view, the frame and the mixer use.

use qs_client::ViewSettings;
use qs_render::{LitWater, Look, ModelDepth, Play, StyleLerp};

/// `r_lerplightstyles 2`, from which every change blends (REND-5.8).
const ALWAYS_LERP: f32 = 2.0;

/// What the host does with the client's settings in one frame (CON-7.4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HostSettings {
    /// The view's settings: `viewsize` (VIEW-3.6), and `gamma` for the blend table in
    /// GLQuake's look (VIEW-4.7, REND-13.9).
    pub(crate) view: ViewSettings,
    /// The `gamma` setting for the frame: `Play` raises the whole frame by it in QuakeSpasm's
    /// look and leaves the frame alone in GLQuake's (`Look::frame_gamma`, REND-13.9).
    pub(crate) frame_gamma: f32,
    /// The mixer's master volume, `volume` (SND-5.4).
    pub(crate) volume: f32,
    /// The CD music's volume, `bgmvolume` (SND-6.2).
    pub(crate) music: f32,
    /// The later engines' drawing settings (CON-7.6).
    pub(crate) drawing: DrawingSettings,
}

/// The later engines' drawing settings as the host reads them each frame (CON-7.6): Ironwail's
/// `r_lerplightstyles` (REND-5.8) and `gl_zfix` (REND-1.16), and `r_litwater` (REND-2.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DrawingSettings {
    pub(crate) style_lerp: StyleLerp,
    pub(crate) zfix: ModelDepth,
    pub(crate) water: LitWater,
}

impl DrawingSettings {
    /// The settings `setting` gives by name, one it lacks 0 (CON-7.6).
    pub(crate) fn read(setting: impl Fn(&str) -> Option<f32>) -> Self {
        let setting = |name: &str| setting(name).unwrap_or(0.0);
        Self {
            // "`if (!r_lerplightstyles.value) f = 0.0;`" and "`if (r_lerplightstyles.value <
            // 2.f && ...) n = k;`".
            style_lerp: match setting("r_lerplightstyles") {
                0.0 => StyleLerp::Off,
                value if value < ALWAYS_LERP => StyleLerp::SkipAbrupt,
                _ => StyleLerp::Always,
            },
            // "`if (!gl_zfix.value ...) zfix = 0;`"
            zfix: if setting("gl_zfix") == 0.0 {
                ModelDepth::AsDrawn
            } else {
                ModelDepth::Behind
            },
            // "`cl.worldmodel->haslitwater && r_litwater.value`"
            water: if setting("r_litwater") == 0.0 {
                LitWater::Unlit
            } else {
                LitWater::Lit
            },
        }
    }

    /// `play` drawing the next frame as the settings say.
    pub(crate) const fn apply(self, play: &mut Play) {
        play.style_lerp = self.style_lerp;
        play.zfix = self.zfix;
        play.water = self.water;
    }
}

impl HostSettings {
    /// The settings `setting` gives by name, one it lacks 0 (CON-7.4), over the client's
    /// current `view` in the look `look`.
    pub(crate) fn read(
        setting: impl Fn(&str) -> Option<f32>,
        view: ViewSettings,
        look: Look,
    ) -> Self {
        let drawing = DrawingSettings::read(&setting);
        let setting = |name: &str| setting(name).unwrap_or(0.0);
        let gamma = setting("gamma");
        Self {
            view: ViewSettings {
                viewsize: setting("viewsize"),
                gamma: look.blend_gamma(gamma),
                ..view
            },
            frame_gamma: gamma,
            volume: setting("volume"),
            music: setting("bgmvolume"),
            drawing,
        }
    }
}

#[cfg(test)]
mod tests;

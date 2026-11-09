// SPDX-License-Identifier: GPL-3.0-or-later

//! The client's settings as the preview host reads them each frame (CON-7.4): what the console
//! and the options menu set, turned into what the view, the frame and the mixer use.

use qs_client::ViewSettings;
use qs_render::Look;

/// What the host does with the client's settings in one frame (CON-7.4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HostSettings {
    /// The view's settings: `viewsize` (VIEW-3.6), and `gamma` for the blend table in
    /// GLQuake's look (VIEW-4.7, REND-13.9).
    pub(crate) view: ViewSettings,
    /// The `gamma` the whole frame is raised to in QuakeSpasm's look (REND-13.9).
    pub(crate) frame_gamma: f32,
    /// The mixer's master volume, `volume` (SND-5.4).
    pub(crate) volume: f32,
    /// The CD music's volume, `bgmvolume` (SND-6.2).
    pub(crate) music: f32,
}

impl HostSettings {
    /// The settings `setting` names, a missing one 0, over the client's current `view` in the
    /// look `look`.
    pub(crate) fn read(setting: impl Fn(&str) -> f32, view: ViewSettings, look: Look) -> Self {
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
        }
    }
}

#[cfg(test)]
mod tests;

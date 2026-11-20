// SPDX-License-Identifier: GPL-3.0-or-later

//! The About window (P7-17): the app's icon, name and version, its copyright and license, and
//! links to its site and its source. It is the app's own on every platform, so a build run
//! outside its bundle says the same as the installed one.

use eframe::egui::{self, RichText};

use crate::strings::{Text, version_line};
use crate::theme::{Theme, Token};

/// The app's site.
pub(crate) const SITE: &str = "https://www.quakecstudio.com/";
/// The app's source, and where an issue with it is reported.
pub(crate) const SOURCE: &str = "https://github.com/quakecstudio/editor";
/// The page that takes a new issue.
pub(crate) const NEW_ISSUE: &str = "https://github.com/quakecstudio/editor/issues/new/choose";

/// The app's icon, the drawing its packages carry, at 256 pixels, as a PNG.
pub const ICON: &[u8] = include_bytes!("../assets/icon.png");
/// The side the icon is drawn at, in points.
const ICON_SIDE: f32 = 96.0;

/// Whether the About window shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum About {
    /// Not shown.
    #[default]
    Hidden,
    /// Shown, over the app.
    Shown,
}

/// The About window and the icon it draws, loaded when it first shows.
#[derive(Default)]
pub(crate) struct AboutWindow {
    shown: About,
    icon: Option<egui::TextureHandle>,
}

impl AboutWindow {
    /// Shows the window.
    pub(crate) const fn open(&mut self) {
        self.shown = About::Shown;
    }

    /// The icon as a texture, decoded once; none when the drawing does not decode.
    pub(crate) fn icon(&mut self, ctx: &egui::Context) -> Option<&egui::TextureHandle> {
        if self.icon.is_none() {
            let decoded = image::load_from_memory_with_format(ICON, image::ImageFormat::Png)
                .map_err(|error| tracing::warn!(%error, "the app's icon did not decode"))
                .ok()?
                .into_rgba8();
            let size = [
                usize::try_from(decoded.width()).ok()?,
                usize::try_from(decoded.height()).ok()?,
            ];
            let image = egui::ColorImage::from_rgba_unmultiplied(size, decoded.as_raw());
            self.icon =
                Some(ctx.load_texture(Text::AppName.get(), image, egui::TextureOptions::LINEAR));
        }
        self.icon.as_ref()
    }

    /// Draws the window over the app while it shows; its Close button, or Escape, hides it.
    pub(crate) fn show(&mut self, ctx: &egui::Context, theme: &Theme) {
        if self.shown == About::Hidden {
            return;
        }
        let icon = self.icon(ctx).map(|icon| (icon.id(), icon.size_vec2()));
        let mut close = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        egui::Window::new(Text::AboutTitle.get())
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    if let Some((id, _)) = icon {
                        let side = egui::Vec2::splat(ICON_SIDE);
                        ui.add(egui::Image::new((id, side)).alt_text(Text::AboutIcon.get()));
                    }
                    ui.heading(Text::AppName.get());
                    ui.label(version_line(env!("CARGO_PKG_VERSION")));
                    let muted = theme.color(Token::TextMuted);
                    ui.label(
                        RichText::new(Text::AboutCopyright.get())
                            .small()
                            .color(muted),
                    );
                    ui.label(RichText::new(Text::AboutLicense.get()).small().color(muted));
                    for (text, url) in [(Text::AboutSite, SITE), (Text::AboutSource, SOURCE)] {
                        let link = ui.link(text.get());
                        // A link to a screen reader, named by its text.
                        ui.ctx().accesskit_node_builder(link.id, |node| {
                            node.set_role(egui::accesskit::Role::Link);
                            node.set_label(text.get());
                        });
                        if link.clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                        }
                    }
                    if ui.button(Text::AboutClose.get()).clicked() {
                        close = true;
                    }
                });
            });
        if close {
            self.shown = About::Hidden;
        }
    }
}

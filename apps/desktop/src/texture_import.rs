// SPDX-License-Identifier: GPL-3.0-or-later

//! The texture import's preview (P6-16, MAP-23): a PNG chosen with Import or dropped on the
//! texture browser is shown quantized to the game palette, as `texture.import` would save it,
//! with its name and dithering to change, before it is saved.

use std::path::PathBuf;

use eframe::egui::{self, ColorImage, RichText, TextureHandle, Vec2};
use qs_wad::texture::Palette;
use qs_workbench::texture_import::{DitherChoice, quantized};

use crate::strings::Text;
use crate::theme::{Theme, Token};

/// The side the preview's picture is drawn at, in points.
const PICTURE: f32 = 128.0;
/// The width of the name field, in points.
const NAME_WIDTH: f32 = 160.0;

/// What the preview asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ImportChoice {
    /// Nothing yet.
    Waiting,
    /// Import the image with this name and dithering.
    Save(PathBuf, String, DitherChoice),
    /// Close the preview without importing.
    Cancel,
}

/// The import preview of one image.
pub(crate) struct ImportPreview {
    path: PathBuf,
    png: Vec<u8>,
    name: String,
    dither: DitherChoice,
    /// The name and dithering the picture was made for, and the picture or why there is none.
    shown: Option<((String, DitherChoice), Result<TextureHandle, String>)>,
}

impl core::fmt::Debug for ImportPreview {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ImportPreview")
            .field("path", &self.path)
            .field("name", &self.name)
            .field("dither", &self.dither)
            .finish_non_exhaustive()
    }
}

impl ImportPreview {
    /// The preview of the image at `path`, named after its file; none when it cannot be read.
    pub(crate) fn open(path: PathBuf) -> Option<Self> {
        let png = std::fs::read(&path).ok()?;
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        Some(Self {
            path,
            png,
            name,
            dither: DitherChoice::default(),
            shown: None,
        })
    }

    /// The name the texture would take, for tests.
    #[cfg(test)]
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// The picture for the name and dithering now set, made again when either changes.
    fn picture(
        &mut self,
        ctx: &egui::Context,
        palette: Option<&Palette>,
    ) -> Result<TextureHandle, String> {
        let wanted = (self.name.clone(), self.dither);
        if let Some((made, picture)) = &self.shown
            && *made == wanted
        {
            return picture.clone();
        }
        let picture = match palette {
            None => Err(Text::MapNoPalette.get().to_owned()),
            Some(palette) => quantized(&self.png, &self.name, palette, self.dither)
                .map_err(|error| error.to_string())
                .map(|decoded| {
                    let size = [
                        usize::try_from(decoded.width).unwrap_or_default(),
                        usize::try_from(decoded.height).unwrap_or_default(),
                    ];
                    let image = ColorImage::from_rgba_unmultiplied(size, &decoded.rgba);
                    ctx.load_texture("texture-import", image, egui::TextureOptions::NEAREST)
                }),
        };
        self.shown = Some((wanted, picture.clone()));
        picture
    }

    /// The preview's window; what it asked for.
    pub(crate) fn ui(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        palette: Option<&Palette>,
    ) -> ImportChoice {
        let mut choice = ImportChoice::Waiting;
        egui::Window::new(Text::MapImportTexture.get())
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                let file = self.path.file_name().map(|name| name.to_string_lossy());
                ui.label(
                    RichText::new(file.unwrap_or_default()).color(theme.color(Token::TextMuted)),
                );
                ui.horizontal(|ui| {
                    ui.label(Text::MapTextureName.get());
                    ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(NAME_WIDTH));
                });
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.dither,
                        DitherChoice::None,
                        Text::MapDitherNone.get(),
                    );
                    ui.selectable_value(
                        &mut self.dither,
                        DitherChoice::FloydSteinberg,
                        Text::MapDitherDiffuse.get(),
                    );
                });
                let ready = match self.picture(ctx, palette) {
                    Ok(handle) => {
                        let size = handle.size_vec2();
                        let scale = (PICTURE / size.x).min(PICTURE / size.y);
                        ui.image((handle.id(), Vec2::new(size.x * scale, size.y * scale)));
                        true
                    }
                    Err(why) => {
                        ui.label(RichText::new(why).color(theme.color(Token::StatusError)));
                        false
                    }
                };
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(ready, egui::Button::new(Text::MapImportSave.get()))
                        .clicked()
                    {
                        choice =
                            ImportChoice::Save(self.path.clone(), self.name.clone(), self.dither);
                    }
                    if ui.button(Text::PaletteCancel.get()).clicked() {
                        choice = ImportChoice::Cancel;
                    }
                });
            });
        choice
    }
}

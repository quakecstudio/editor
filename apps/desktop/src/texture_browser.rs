// SPDX-License-Identifier: GPL-3.0-or-later

//! The texture browser (P6-10, MAP-5): every texture of the WADs the open map's world names
//! and of the BSP it came from, in the tab below the map view where the mockup has it, each in
//! the game palette's colours with how many faces use it. Only the cells in view are laid out,
//! and a texture is decoded when its cell is first drawn, then kept until the sources change.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eframe::egui::{self, ColorImage, RichText, Sense, Stroke, TextureHandle, Ui, Vec2};
#[cfg(test)]
use qs_wad::texture::DecodedTexture;
use qs_workbench::map_edit::{MapDocument, MapRevision};
use qs_workbench::map_textures::{BrowserTexture, MapTextures, TextureOrigin, TextureSources};
use qs_workbench::texture_import::DitherChoice;

use crate::map_browser::{FILTER_WIDTH, filter_shows};
use crate::strings::{Text, texture_count, wad_label};
use crate::texture_import::{ImportChoice, ImportPreview};
use crate::theme::{Theme, Token};

/// The side of a texture's cell, in points (the mockup's texture cells).
const CELL: f32 = 84.0;
/// The space between cells, in points.
const GAP: f32 = 12.0;
/// The height of a cell with its name and count below it, in points.
const CELL_HEIGHT: f32 = 120.0;
/// The rounding of a cell's corners, in points.
const CORNER: f32 = 4.0;
/// The width of the selected cell's outline, in points.
const SELECTED_STROKE: f32 = 2.0;
/// The spacing of the hatching drawn for a texture with no picture, in points.
const HATCH: f32 = 12.0;

/// The texture browser's state.
#[derive(Default)]
pub(crate) struct TextureBrowser {
    filter: String,
    sources: Option<TextureSources>,
    listing: MapTextures,
    /// The map and edits the listing was made for, so a face's new texture is counted.
    listed: Option<(PathBuf, MapRevision)>,
    /// Each texture decoded so far, by name; `None` for one with nothing to show.
    pictures: HashMap<String, Option<TextureHandle>>,
    selected: Option<String>,
    /// How many textures were decoded since the sources were set.
    decodes: usize,
    /// The image being imported, shown quantized before it is saved (P6-16).
    import: Option<ImportPreview>,
}

impl core::fmt::Debug for TextureBrowser {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TextureBrowser")
            .field("filter", &self.filter)
            .field("textures", &self.listing.textures.len())
            .field("decodes", &self.decodes)
            .finish_non_exhaustive()
    }
}

impl TextureBrowser {
    /// Takes the map's texture sources, read again when the map opens, a WAD changes, the
    /// world names other WADs or another game is chosen: every picture is decoded again when
    /// next shown.
    pub(crate) fn set_sources(&mut self, sources: TextureSources) {
        self.sources = Some(sources);
        self.listed = None;
        self.pictures.clear();
        self.decodes = 0;
    }

    /// Whether the sources were made for other WADs or another chosen game than `document`
    /// and `chosen` now give.
    pub(crate) fn is_stale(&self, document: &MapDocument, chosen: Option<&Path>) -> bool {
        self.sources
            .as_ref()
            .is_some_and(|sources| sources.is_stale(document, chosen))
    }

    /// The WAD files to watch for another program's writes.
    pub(crate) fn watched(&self) -> Vec<PathBuf> {
        self.sources
            .as_ref()
            .map(TextureSources::watched)
            .unwrap_or_default()
    }

    /// The width and height of the texture `name`, in texels, when a source holds it.
    pub(crate) fn size(&self, name: &str) -> Option<[u32; 2]> {
        self.sources.as_ref()?.size(name)
    }

    /// The texture chosen, which the Properties panel can put on the selected brushes.
    pub(crate) fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// How many textures were decoded since the sources were set.
    #[cfg(test)]
    pub(crate) const fn decodes(&self) -> usize {
        self.decodes
    }

    /// The listing shown.
    #[cfg(test)]
    pub(crate) const fn listing(&self) -> &MapTextures {
        &self.listing
    }

    /// The header's part: the filter and what is listed, for `document`, whose listing is
    /// made again when it changes.
    pub(crate) fn header(&mut self, ui: &mut Ui, theme: &Theme, document: &MapDocument) {
        let listed = (document.path().to_path_buf(), document.edits());
        if self.listed.as_ref() != Some(&listed)
            && let Some(sources) = &self.sources
        {
            self.listing = sources.listing(document);
            self.listed = Some(listed);
        }
        ui.add(
            egui::TextEdit::singleline(&mut self.filter)
                .hint_text(Text::MapFilterTextures.get())
                .desired_width(FILTER_WIDTH),
        );
        let wads: Vec<String> = self.listing.wads.iter().map(wad_label).collect();
        let shown = self.shown().len();
        ui.label(
            RichText::new(texture_count(&wads, shown, self.listing.textures.len()))
                .color(theme.color(Token::TextMuted)),
        );
        if ui.button(Text::MapImportTexture.get()).clicked()
            && let Some(path) = rfd::FileDialog::new()
                .set_title(Text::MapImportTexture.get())
                .add_filter("PNG", &["png"])
                .pick_file()
        {
            self.import = ImportPreview::open(path);
        }
        if self.sources.as_ref().is_some_and(|s| !s.has_palette()) {
            ui.label(RichText::new(Text::MapNoPalette.get()).color(theme.color(Token::StatusWarn)));
        }
    }

    /// The places in the listing of the textures the filter shows.
    fn shown(&self) -> Vec<usize> {
        self.listing
            .textures
            .iter()
            .enumerate()
            .filter(|(_, texture)| filter_shows(&self.filter, &texture.name))
            .map(|(index, _)| index)
            .collect()
    }

    /// The import preview's window, when an image is being imported; the import asked for.
    pub(crate) fn import_window(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
    ) -> Option<(PathBuf, String, DitherChoice)> {
        let palette = self.sources.as_ref().and_then(TextureSources::palette);
        let choice = self.import.as_mut()?.ui(ctx, theme, palette);
        match choice {
            ImportChoice::Waiting => None,
            ImportChoice::Cancel => {
                self.import = None;
                None
            }
            ImportChoice::Save(path, name, dither) => {
                self.import = None;
                Some((path, name, dither))
            }
        }
    }

    /// The texture `name` as the browser decodes it, for tests.
    #[cfg(test)]
    pub(crate) fn decoded(&self, name: &str) -> Option<DecodedTexture> {
        self.sources
            .as_ref()
            .and_then(|sources| sources.decode(name))
    }

    /// The image being imported, for tests.
    #[cfg(test)]
    pub(crate) const fn importing(&self) -> Option<&ImportPreview> {
        self.import.as_ref()
    }

    /// The textures, laying out and decoding only the cells in view; a PNG dropped on the window
    /// while they show is opened for import.
    pub(crate) fn body(&mut self, ui: &mut Ui, theme: &Theme) {
        // The pointer is not followed while a file is dragged in from another program, so a
        // PNG dropped on the window while the browser shows is taken as dropped on it.
        let dropped = ui.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .find(|path| {
                    path.extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("png"))
                })
        });
        if let Some(path) = dropped {
            self.import = ImportPreview::open(path);
        }
        let shown = self.shown();
        let count = u16::try_from(shown.len()).map_or(f32::from(u16::MAX), f32::from);
        let width = count * (CELL + GAP);
        egui::ScrollArea::horizontal().show_viewport(ui, |ui, viewport| {
            ui.set_min_size(Vec2::new(width, CELL_HEIGHT));
            let origin = ui.min_rect().min;
            let first = (viewport.min.x / (CELL + GAP)).floor().max(0.0);
            let last = (viewport.max.x / (CELL + GAP)).ceil().max(0.0);
            for (slot, &index) in shown.iter().enumerate() {
                let at = u16::try_from(slot).map_or(f32::from(u16::MAX), f32::from);
                if at < first || at >= last {
                    continue;
                }
                let Some(texture) = self.listing.textures.get(index).cloned() else {
                    continue;
                };
                let rect = egui::Rect::from_min_size(
                    egui::pos2(origin.x + at * (CELL + GAP), origin.y),
                    Vec2::new(CELL, CELL_HEIGHT),
                );
                let mut cell_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                self.cell(&mut cell_ui, theme, &texture);
            }
        });
    }

    /// One texture: its picture, decoded when first drawn on screen, and its name and uses.
    fn cell(&mut self, ui: &mut Ui, theme: &Theme, texture: &BrowserTexture) {
        ui.vertical(|ui| {
            ui.set_width(CELL);
            let (rect, response) = ui.allocate_exact_size(Vec2::splat(CELL), Sense::click());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &texture.name)
            });
            if response.clicked() {
                self.selected = Some(texture.name.clone());
            }
            if texture.origin == TextureOrigin::Missing {
                response.on_hover_text(Text::MapTextureMissing.get());
            }
            let selected = self.selected.as_deref() == Some(texture.name.as_str());
            if ui.is_rect_visible(rect) {
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, CORNER, theme.color(Token::SurfaceViewport));
                match self.picture(ui.ctx(), texture) {
                    Some(handle) => {
                        let size = handle.size_vec2();
                        let scale = (CELL / size.x).min(CELL / size.y);
                        let fitted = Vec2::new(size.x * scale, size.y * scale);
                        let at = egui::Rect::from_center_size(rect.center(), fitted);
                        painter.image(
                            handle.id(),
                            at,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                    }
                    None => hatch(&painter, rect, theme.color(Token::BorderStrong)),
                }
                if selected {
                    painter.rect_stroke(
                        rect,
                        CORNER,
                        Stroke::new(SELECTED_STROKE, theme.color(Token::Accent)),
                        egui::StrokeKind::Outside,
                    );
                }
            }
            let (name_color, uses_color) = if selected {
                (theme.color(Token::Text), theme.color(Token::Accent))
            } else if texture.origin == TextureOrigin::Missing {
                (
                    theme.color(Token::StatusError),
                    theme.color(Token::TextFaint),
                )
            } else {
                (theme.color(Token::TextMuted), theme.color(Token::TextFaint))
            };
            ui.add(
                egui::Label::new(RichText::new(&texture.name).monospace().color(name_color))
                    .truncate(),
            );
            ui.label(
                RichText::new(texture.uses.to_string())
                    .monospace()
                    .color(uses_color),
            );
        });
    }

    /// The picture of `texture`, decoded the first time it is asked for; none for a texture
    /// with nothing to show or larger than the GPU takes.
    fn picture(&mut self, ctx: &egui::Context, texture: &BrowserTexture) -> Option<TextureHandle> {
        if let Some(picture) = self.pictures.get(&texture.name) {
            return picture.clone();
        }
        let largest = ctx.input(|input| input.max_texture_side);
        let decoded = self
            .sources
            .as_ref()
            .and_then(|sources| sources.decode(&texture.name))
            .filter(|decoded| {
                [decoded.width, decoded.height]
                    .iter()
                    .all(|&side| usize::try_from(side).is_ok_and(|side| side <= largest))
            });
        let picture = decoded.map(|decoded| {
            self.decodes = self.decodes.saturating_add(1);
            let size = [
                usize::try_from(decoded.width).unwrap_or_default(),
                usize::try_from(decoded.height).unwrap_or_default(),
            ];
            let image = ColorImage::from_rgba_unmultiplied(size, &decoded.rgba);
            ctx.load_texture(
                format!("texture:{}", texture.name),
                image,
                egui::TextureOptions::NEAREST,
            )
        });
        self.pictures.insert(texture.name.clone(), picture.clone());
        picture
    }
}

/// Diagonal lines across `rect`, for a texture with no picture.
fn hatch(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let stroke = Stroke::new(1.0, color);
    let mut offset = 0.0;
    while offset < rect.width() + rect.height() {
        let start = egui::pos2(rect.left() + offset, rect.top());
        let end = egui::pos2(rect.left() + offset - rect.height(), rect.bottom());
        painter.line_segment([start, end], stroke);
        offset += HATCH;
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later

//! The texture browser (P6-10, MAP-5): every texture of the WADs the open map's world names
//! and of the BSP it came from, in the tab below the map view where the mockup has it, each in
//! the game palette's colours with how many faces use it. A texture is decoded only when its
//! cell is drawn on screen, and kept until the map or a WAD changes.

use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui::{self, ColorImage, RichText, Sense, Stroke, TextureHandle, Ui, Vec2};
use qs_workbench::map_edit::{MapDocument, MapRevision};
use qs_workbench::map_textures::{BrowserTexture, MapTextures, TextureOrigin, TextureSources};

use crate::strings::{Text, texture_count, wad_label};
use crate::theme::{Theme, Token};

/// The side of a texture's cell, in points (the mockup's texture cells).
const CELL: f32 = 84.0;
/// The rounding of a cell's corners, in points.
const CORNER: f32 = 4.0;
/// The width of the filter field, in points.
const FILTER_WIDTH: f32 = 220.0;
/// The width of the selected cell's outline, in points.
const SELECTED_STROKE: f32 = 2.0;
/// The spacing of the hatching drawn for a texture with no picture, in points.
const HATCH: f32 = 12.0;

/// What a texture's cell shows.
enum Picture {
    /// The texture, decoded.
    Shown(TextureHandle),
    /// Nothing to show: no palette, or no WAD or BSP holds it.
    None,
}

/// The texture browser's state.
#[derive(Default)]
pub(crate) struct TextureBrowser {
    filter: String,
    sources: Option<TextureSources>,
    listing: MapTextures,
    /// The map and edits the listing was made for, so a face's new texture is counted.
    listed: Option<(PathBuf, MapRevision)>,
    pictures: HashMap<String, Picture>,
    selected: Option<String>,
    /// How many textures were decoded since the sources were set.
    decodes: usize,
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
    /// Takes the map's texture sources, read again when the map opens or a WAD changes: every
    /// picture is decoded again when next shown.
    pub(crate) fn set_sources(&mut self, sources: TextureSources) {
        self.sources = Some(sources);
        self.listed = None;
        self.pictures.clear();
        self.decodes = 0;
    }

    /// The WAD files to watch for another program's writes.
    pub(crate) fn watched(&self) -> Vec<PathBuf> {
        self.sources
            .as_ref()
            .map(TextureSources::watched)
            .unwrap_or_default()
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
        if self.sources.as_ref().is_some_and(|s| !s.has_palette()) {
            ui.label(RichText::new(Text::MapNoPalette.get()).color(theme.color(Token::StatusWarn)));
        }
    }

    /// The textures the filter shows.
    fn shown(&self) -> Vec<BrowserTexture> {
        let filter = self.filter.trim().to_lowercase();
        self.listing
            .textures
            .iter()
            .filter(|texture| texture.name.to_lowercase().contains(&filter))
            .cloned()
            .collect()
    }

    /// The textures, decoding those whose cells are on screen.
    pub(crate) fn body(&mut self, ui: &mut Ui, theme: &Theme) {
        let shown = self.shown();
        egui::ScrollArea::horizontal().show(ui, |ui| {
            ui.horizontal(|ui| {
                for texture in &shown {
                    self.cell(ui, theme, texture);
                }
            });
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
                if self.selected.as_deref() == Some(texture.name.as_str()) {
                    painter.rect_stroke(
                        rect,
                        CORNER,
                        Stroke::new(SELECTED_STROKE, theme.color(Token::Accent)),
                        egui::StrokeKind::Outside,
                    );
                }
            }
            let (name_color, uses_color) = if self.selected.as_deref() == Some(&texture.name) {
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

    /// The picture of `texture`, decoded the first time it is asked for.
    fn picture(&mut self, ctx: &egui::Context, texture: &BrowserTexture) -> Option<TextureHandle> {
        if let Some(picture) = self.pictures.get(&texture.name) {
            return match picture {
                Picture::Shown(handle) => Some(handle.clone()),
                Picture::None => None,
            };
        }
        let decoded = self
            .sources
            .as_ref()
            .and_then(|sources| sources.decode(&texture.name));
        let picture = match decoded {
            Some(decoded) => {
                self.decodes = self.decodes.saturating_add(1);
                let size = [
                    usize::try_from(decoded.width).unwrap_or_default(),
                    usize::try_from(decoded.height).unwrap_or_default(),
                ];
                let image = ColorImage::from_rgba_unmultiplied(size, &decoded.rgba);
                Picture::Shown(ctx.load_texture(
                    format!("texture:{}", texture.name),
                    image,
                    egui::TextureOptions::NEAREST,
                ))
            }
            None => Picture::None,
        };
        let handle = match &picture {
            Picture::Shown(handle) => Some(handle.clone()),
            Picture::None => None,
        };
        self.pictures.insert(texture.name.clone(), picture);
        handle
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

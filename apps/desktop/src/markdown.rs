// SPDX-License-Identifier: GPL-3.0-or-later

//! What an agent says, drawn as Markdown (L-43): headings, bold, italic and struck text, inline
//! code, code blocks, lists, quotes, tables, links and rules, as Zed's agent panel draws a
//! reply. The text is read as CommonMark by `pulldown-cmark`, with tables, strikethrough and
//! task lists; a reply still arriving is drawn as far as it reads, an unclosed mark as the
//! text it is so far. A link is drawn with its address after it and opens nothing: the app
//! makes no request of its own.

use eframe::egui::text::LayoutJob;
use eframe::egui::{self, FontId, Label, Stroke, TextFormat, TextStyle, Ui};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::theme::{Theme, Token, semibold};

/// How much larger than the body a heading of each level is, the first level first; a level
/// past the last is the body's size, told apart by its weight.
const HEADING_SCALE: [f32; 3] = [1.4, 1.25, 1.1];
/// The step of the spacing scale a list's items, a quote and a code block are set in by.
const INDENT: usize = 2;
/// The step of the spacing scale between blocks.
const GAP: usize = 0;
/// The mark before an item of a list with no numbers.
const BULLET: &str = "\u{2022}";
/// The marks of a task list's items, done and not, as they are typed: the bundled fonts have
/// no ballot boxes, and these are in every one of them.
const TASK_DONE: &str = "[x] ";
const TASK_OPEN: &str = "[ ] ";
/// The step of the spacing scale as wide as the column a list's marks are drawn in, which the
/// later blocks of an item are set in by too.
const MARK_COLUMN: usize = 4;

/// The inline marks open where text is read.
#[derive(Clone, Copy, Debug, Default)]
struct Marks {
    strong: u8,
    emphasis: u8,
    struck: u8,
    link: u8,
}

/// A table being read: its rows of cells, and how many of them are its head.
#[derive(Default)]
struct Table {
    rows: Vec<Vec<LayoutJob>>,
    head: usize,
}

/// A reply being drawn, block by block.
struct Draw<'a> {
    ui: &'a mut Ui,
    theme: &'a Theme,
    /// The text of the block being read.
    job: LayoutJob,
    marks: Marks,
    heading: Option<HeadingLevel>,
    /// The lists open, outermost first: the next number of a numbered one.
    lists: Vec<Option<u64>>,
    /// The mark the next block is drawn after: a list item's.
    marker: Option<String>,
    quotes: usize,
    /// The text of the code block being read.
    code: Option<String>,
    table: Option<Table>,
    /// The links open, outermost first: where each one's text starts in the block's text,
    /// and its address. An image in a link is one in another.
    links: Vec<(usize, String)>,
    /// How many blocks were drawn, for each one's id.
    drawn: usize,
}

impl Draw<'_> {
    /// The body's font.
    fn body(&self) -> FontId {
        TextStyle::Body.resolve(self.ui.style())
    }

    /// The format of text read now, under the open marks.
    fn format(&self) -> TextFormat {
        let body = self.body();
        let heading = self.heading.map(|level| {
            let scale = HEADING_SCALE
                .get(heading_index(level))
                .copied()
                .unwrap_or(1.0);
            FontId::new(body.size * scale, semibold())
        });
        let font = match heading {
            Some(font) => font,
            None if self.marks.strong > 0 => FontId::new(body.size, semibold()),
            None => body,
        };
        let color = if self.marks.link > 0 {
            self.theme.color(Token::Focus)
        } else if self.quotes > 0 {
            self.theme.color(Token::TextMuted)
        } else {
            self.theme.color(Token::Text)
        };
        let line = Stroke::new(1.0, color);
        TextFormat {
            font_id: font,
            color,
            italics: self.marks.emphasis > 0,
            underline: if self.marks.link > 0 {
                line
            } else {
                Stroke::NONE
            },
            strikethrough: if self.marks.struck > 0 {
                line
            } else {
                Stroke::NONE
            },
            ..TextFormat::default()
        }
    }

    /// The format of inline code.
    fn code_format(&self) -> TextFormat {
        TextFormat {
            font_id: TextStyle::Monospace.resolve(self.ui.style()),
            color: self.theme.color(Token::TextCode),
            background: self.theme.color(Token::SurfaceRaised),
            ..TextFormat::default()
        }
    }

    /// Adds `text` to the block being read, in `format`.
    fn push(&mut self, text: &str, format: TextFormat) {
        self.job.append(text, 0.0, format);
    }

    /// Draws the block read so far, if it has text: set in by its lists and quotes, after
    /// its list's mark, or under the text of the item it goes on.
    fn flush(&mut self) {
        let job = core::mem::take(&mut self.job);
        let marker = self.marker.take();
        if job.text.is_empty() && marker.is_none() {
            return;
        }
        let inset = self.inset();
        let column = self.mark_column();
        let muted = self.theme.color(Token::TextMuted);
        let id = self.next_id();
        self.ui.push_id(id, |ui| {
            ui.horizontal_top(|ui| {
                ui.add_space(inset);
                match marker {
                    Some(marker) => {
                        let mark = egui::RichText::new(marker).color(muted);
                        ui.add_sized([column, 0.0], Label::new(mark).selectable(false));
                    }
                    None => ui.add_space(column),
                }
                ui.vertical(|ui| ui.add(Label::new(job).wrap()));
            });
        });
        self.ui.add_space(self.theme.space(GAP));
    }

    /// How far a block's row starts in: a step for each list past the first and each quote.
    fn inset(&self) -> f32 {
        let steps = self
            .lists
            .len()
            .saturating_sub(1)
            .saturating_add(self.quotes);
        let step = self.theme.space(INDENT);
        (0..steps).map(|_| step).sum()
    }

    /// The width of the column a list's marks are drawn in; none outside a list.
    fn mark_column(&self) -> f32 {
        if self.lists.is_empty() {
            0.0
        } else {
            self.theme.space(MARK_COLUMN)
        }
    }

    /// How far a block with no mark of its own is set in: under its item's text.
    fn indent(&self) -> f32 {
        self.inset() + self.mark_column()
    }

    /// An id for the next block drawn.
    fn next_id(&mut self) -> usize {
        self.drawn = self.drawn.saturating_add(1);
        self.drawn
    }

    /// Draws a code block's `text` in the code font on the editor's surface, scrolled across
    /// when a line is wider than the panel.
    fn code_block(&mut self, text: &str) {
        let indent = self.indent();
        let font = TextStyle::Monospace.resolve(self.ui.style());
        let color = self.theme.color(Token::TextCode);
        let fill = self.theme.color(Token::SurfaceEditor);
        let pad = self.theme.space(GAP);
        let id = self.next_id();
        let text = text.strip_suffix('\n').unwrap_or(text);
        self.ui.horizontal_top(|ui| {
            ui.add_space(indent);
            egui::Frame::NONE
                .fill(fill)
                .inner_margin(pad)
                .show(ui, |ui| {
                    egui::ScrollArea::horizontal().id_salt(id).show(ui, |ui| {
                        let mut job = LayoutJob::default();
                        job.append(
                            text,
                            0.0,
                            TextFormat {
                                font_id: font,
                                color,
                                ..TextFormat::default()
                            },
                        );
                        ui.add(Label::new(job).extend());
                    });
                });
        });
        self.ui.add_space(self.theme.space(GAP));
    }

    /// Draws a table: its head's cells above a rule, each column as wide as its widest cell.
    fn table(&mut self, table: Table) {
        let id = self.next_id();
        let indent = self.indent();
        let columns = table.rows.iter().map(Vec::len).max().unwrap_or(0);
        self.ui.horizontal_top(|ui| {
            ui.add_space(indent);
            egui::ScrollArea::horizontal().id_salt(id).show(ui, |ui| {
                egui::Grid::new(id).striped(true).show(ui, |ui| {
                    for (index, row) in table.rows.into_iter().enumerate() {
                        for cell in row {
                            ui.add(Label::new(cell));
                        }
                        ui.end_row();
                        if index.saturating_add(1) == table.head {
                            for _ in 0..columns {
                                ui.separator();
                            }
                            ui.end_row();
                        }
                    }
                });
            });
        });
        self.ui.add_space(self.theme.space(GAP));
    }

    /// Takes the start of a block or of a run of marked text.
    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { level, .. } => {
                // A list item's own text has no end of its own before a heading in it.
                self.flush();
                self.heading = Some(level);
            }
            Tag::HtmlBlock => self.flush(),
            Tag::BlockQuote(_) => {
                self.flush();
                self.quotes = self.quotes.saturating_add(1);
            }
            Tag::CodeBlock(_) => {
                self.flush();
                self.code = Some(String::new());
            }
            Tag::List(first) => {
                // An item's own text is drawn before the list inside it.
                self.flush();
                self.lists.push(first);
            }
            Tag::Item => {
                let marker = match self.lists.last_mut() {
                    Some(Some(number)) => {
                        let marker = format!("{number}.");
                        *number = number.saturating_add(1);
                        marker
                    }
                    Some(None) | None => BULLET.to_owned(),
                };
                self.marker = Some(marker);
            }
            Tag::Table(_) => {
                self.flush();
                self.table = Some(Table::default());
            }
            Tag::TableHead | Tag::TableRow => {
                if let Some(table) = &mut self.table {
                    table.rows.push(Vec::new());
                }
            }
            Tag::Emphasis => self.marks.emphasis = self.marks.emphasis.saturating_add(1),
            Tag::Strong => self.marks.strong = self.marks.strong.saturating_add(1),
            Tag::Strikethrough => self.marks.struck = self.marks.struck.saturating_add(1),
            Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                self.marks.link = self.marks.link.saturating_add(1);
                self.links
                    .push((self.job.text.len(), dest_url.into_string()));
            }
            Tag::Paragraph
            | Tag::FootnoteDefinition(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::TableCell
            | Tag::Superscript
            | Tag::Subscript
            | Tag::MetadataBlock(_) => {}
        }
    }

    /// Takes the end of a block or of a run of marked text.
    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::Item | TagEnd::HtmlBlock => self.flush(),
            TagEnd::Heading(_) => {
                self.flush();
                self.heading = None;
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.quotes = self.quotes.saturating_sub(1);
            }
            TagEnd::CodeBlock => {
                if let Some(text) = self.code.take() {
                    self.code_block(&text);
                }
            }
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
            }
            TagEnd::TableHead => {
                if let Some(table) = &mut self.table {
                    table.head = table.rows.len();
                }
            }
            TagEnd::TableCell => {
                let cell = core::mem::take(&mut self.job);
                if let Some(row) = self.table.as_mut().and_then(|table| table.rows.last_mut()) {
                    row.push(cell);
                }
            }
            TagEnd::Table => {
                if let Some(table) = self.table.take() {
                    self.table(table);
                }
            }
            TagEnd::Emphasis => self.marks.emphasis = self.marks.emphasis.saturating_sub(1),
            TagEnd::Strong => self.marks.strong = self.marks.strong.saturating_sub(1),
            TagEnd::Strikethrough => self.marks.struck = self.marks.struck.saturating_sub(1),
            TagEnd::Link | TagEnd::Image => {
                self.marks.link = self.marks.link.saturating_sub(1);
                // The address, when the link's text is not it.
                if let Some((from, address)) = self.links.pop() {
                    let shown = self.job.text.get(from..).unwrap_or_default();
                    if !address.is_empty() && shown != address {
                        let mut format = self.format();
                        format.color = self.theme.color(Token::TextMuted);
                        self.push(&format!(" ({address})"), format);
                    }
                }
            }
            TagEnd::TableRow
            | TagEnd::FootnoteDefinition
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::Superscript
            | TagEnd::Subscript
            | TagEnd::MetadataBlock(_) => {}
        }
    }

    /// Takes one thing the text holds.
    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => match &mut self.code {
                Some(code) => code.push_str(&text),
                None => {
                    let format = self.format();
                    self.push(&text, format);
                }
            },
            Event::Code(text) => {
                let format = self.code_format();
                self.push(&text, format);
            }
            // Markup the panel does not draw is shown as it was written.
            Event::Html(text) | Event::InlineHtml(text) => {
                let format = self.format();
                self.push(&text, format);
            }
            Event::InlineMath(text) | Event::DisplayMath(text) => {
                let format = self.code_format();
                self.push(&text, format);
            }
            Event::FootnoteReference(name) => {
                let format = self.format();
                self.push(&format!("[{name}]"), format);
            }
            Event::SoftBreak => {
                let format = self.format();
                self.push(" ", format);
            }
            Event::HardBreak => {
                let format = self.format();
                self.push("\n", format);
            }
            Event::Rule => {
                self.flush();
                self.ui.separator();
            }
            Event::TaskListMarker(done) => {
                let format = self.code_format();
                self.push(if done { TASK_DONE } else { TASK_OPEN }, format);
            }
        }
    }
}

/// A heading level's place in [`HEADING_SCALE`].
const fn heading_index(level: HeadingLevel) -> usize {
    match level {
        HeadingLevel::H1 => 0,
        HeadingLevel::H2 => 1,
        HeadingLevel::H3 => 2,
        HeadingLevel::H4 | HeadingLevel::H5 | HeadingLevel::H6 => HEADING_SCALE.len(),
    }
}

/// Which of a thread's texts one is: what tells its code blocks and tables from another's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Part {
    /// A message or an item, by its place in the thread.
    Message(usize),
    /// The reply still arriving.
    Streaming,
    /// The run's summary.
    Summary,
}

/// What a text took when it was last drawn: what tells whether it is the same text in the same
/// width, and how tall it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Drawn {
    /// The text's length and its hash.
    text: (usize, u64),
    /// The bits of the width it was wrapped to and of the height it took.
    size: [u32; 2],
}

/// Draws `text` as Markdown down `ui`, wrapped to its width. `part` tells this text from the
/// others drawn in the same place, a thread's other replies: each one's code blocks and tables
/// keep their own scroll and their own columns. A text scrolled out of sight is not read or
/// laid out again: the room it took when it was last drawn is left for it, so a long thread
/// costs a frame only what shows of it.
pub(crate) fn show(ui: &mut Ui, theme: &Theme, part: Part, text: &str) {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let id = ui.id().with(part);
    let width = ui.available_width();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    core::hash::Hash::hash(text, &mut hasher);
    let same = (text.len(), core::hash::Hasher::finish(&hasher));
    let known: Option<Drawn> = ui.data(|data| data.get_temp(id));
    if let Some(known) = known.filter(|known| known.text == same)
        && known.size[0] == width.to_bits()
    {
        let size = egui::vec2(width, f32::from_bits(known.size[1]));
        if !ui.is_rect_visible(egui::Rect::from_min_size(ui.cursor().min, size)) {
            ui.allocate_space(size);
            return;
        }
    }
    let drawn = ui.push_id(part, |ui| {
        ui.vertical(|ui| draw(ui, theme, text, options));
    });
    let took = Drawn {
        text: same,
        size: [width.to_bits(), drawn.response.rect.height().to_bits()],
    };
    ui.data_mut(|data| data.insert_temp(id, took));
}

/// Reads `text` and draws each block of it.
fn draw(ui: &mut Ui, theme: &Theme, text: &str, options: Options) {
    {
        let mut draw = Draw {
            ui,
            theme,
            job: LayoutJob::default(),
            marks: Marks::default(),
            heading: None,
            lists: Vec::new(),
            marker: None,
            quotes: 0,
            code: None,
            table: None,
            links: Vec::new(),
            drawn: 0,
        };
        for event in Parser::new_ext(text, options) {
            draw.event(event);
        }
        // A reply still arriving may end inside a block.
        if let Some(code) = draw.code.take() {
            draw.code_block(&code);
        }
        draw.flush();
    }
}

#[cfg(test)]
mod tests;

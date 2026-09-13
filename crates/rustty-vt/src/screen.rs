//! Owned screen storage. Rows retain identity when they enter scrollback.
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Color {
    #[default]
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Underline {
    #[default]
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Style {
    pub foreground: Color,
    pub background: Color,
    pub underline_color: Color,
    pub bold: bool,
    pub faint: bool,
    pub italic: bool,
    pub blink: bool,
    pub inverse: bool,
    pub invisible: bool,
    pub strikethrough: bool,
    pub overline: bool,
    pub underline: Underline,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SemanticContent {
    #[default]
    Output,
    Prompt,
    Input,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    /// Empty for unwritten cells and the continuation of a wide glyph.
    pub text: String,
    /// Zero denotes the continuation of a two-cell glyph.
    pub width: u8,
    pub style: Style,
    pub hyperlink: Option<String>,
    pub protected: bool,
    pub semantic: SemanticContent,
    /// Padding before a wide glyph that wrapped at the right edge.
    pub spacer_head: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            text: String::new(),
            width: 1,
            style: Style::default(),
            hyperlink: None,
            protected: false,
            semantic: SemanticContent::Output,
            spacer_head: false,
        }
    }
}

impl Cell {
    pub(crate) fn blank(background: Color) -> Self {
        Self {
            style: Style {
                background,
                ..Style::default()
            },
            ..Self::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.width != 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Row {
    pub id: u64,
    pub cells: Vec<Cell>,
    pub wrapped: bool,
    pub semantic: SemanticContent,
    pub dirty: bool,
}

impl Row {
    pub(crate) fn new(id: u64, cols: usize, background: Color) -> Self {
        Self {
            id,
            cells: vec![Cell::blank(background); cols],
            wrapped: false,
            semantic: SemanticContent::Output,
            dirty: true,
        }
    }

    pub fn text(&self) -> String {
        let mut result = String::new();
        for cell in &self.cells {
            if cell.width == 0 || cell.spacer_head {
                continue;
            }
            if cell.text.is_empty() {
                result.push(' ');
            } else {
                result.push_str(&cell.text);
            }
        }
        result.truncate(result.trim_end_matches(' ').len());
        result
    }

    pub(crate) fn used(&self) -> usize {
        self.cells
            .iter()
            .rposition(|c| {
                !c.text.is_empty() || c.width == 0 || c.style.background != Color::Default
            })
            .map_or(0, |i| i + 1)
    }

    pub(crate) fn repair_wide(&mut self, background: Color) {
        for col in 0..self.cells.len() {
            if self.cells[col].width == 0 && (col == 0 || self.cells[col - 1].width != 2)
                || self.cells[col].width == 2
                    && (col + 1 == self.cells.len() || self.cells[col + 1].width != 0)
            {
                self.cells[col] = Cell::blank(background);
            }
        }
        self.dirty = true;
    }

    pub(crate) fn erase(
        &mut self,
        mut start: usize,
        mut end: usize,
        background: Color,
        protected: bool,
    ) {
        if start >= self.cells.len() || start >= end {
            return;
        }
        end = end.min(self.cells.len());
        if self.cells[start].width == 0 && start > 0 {
            start -= 1;
        }
        if end < self.cells.len() && self.cells[end - 1].width == 2 {
            end += 1;
        }
        for cell in &mut self.cells[start..end] {
            if !protected || !cell.protected {
                *cell = Cell::blank(background);
            }
        }
        self.dirty = true;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CursorShape {
    #[default]
    Block,
    Bar,
    Underline,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub col: usize,
    pub row: usize,
    pub shape: CursorShape,
    pub visible: bool,
    pub blink: bool,
    pub pending_wrap: bool,
    pub style: Style,
    pub protected: bool,
    pub hyperlink: Option<String>,
    pub semantic: SemanticContent,
}

impl Default for Cursor {
    fn default() -> Self {
        Self {
            col: 0,
            row: 0,
            shape: CursorShape::Block,
            visible: true,
            blink: false,
            pending_wrap: false,
            style: Style::default(),
            protected: false,
            hyperlink: None,
            semantic: SemanticContent::Output,
        }
    }
}

/// A reference to a cell by stable row identity, including scrollback rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GridPoint {
    pub row: u64,
    pub col: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    pub start: GridPoint,
    pub end: GridPoint,
    pub rectangular: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TrackedPoint(u64);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Charset {
    #[default]
    Utf8,
    Ascii,
    British,
    DecSpecial,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CharsetState {
    pub slots: [Charset; 4],
    pub gl: usize,
    pub gr: usize,
    pub single: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SavedCursor {
    pub cursor: Cursor,
    pub origin: bool,
    pub charset: CharsetState,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Screen {
    pub rows: Vec<Row>,
    pub history: VecDeque<Row>,
    pub cursor: Cursor,
    pub selection: Option<Selection>,
    pub viewport_offset: usize,
    pub kitty_keyboard: Vec<u8>,
    pub(crate) saved_cursor: Option<SavedCursor>,
    pub(crate) charset: CharsetState,
    pub(crate) iso_protection: bool,
    pub(crate) scrollback_limit: usize,
    pub(crate) next_row: u64,
    #[serde(skip)]
    tracked: HashMap<u64, Option<GridPoint>>,
    #[serde(skip)]
    next_track: u64,
}

impl Screen {
    pub(crate) fn new(cols: usize, rows: usize, scrollback_limit: usize) -> Self {
        Self {
            rows: (0..rows)
                .map(|i| Row::new(i as u64, cols, Color::Default))
                .collect(),
            history: VecDeque::new(),
            cursor: Cursor::default(),
            selection: None,
            viewport_offset: 0,
            kitty_keyboard: Vec::new(),
            saved_cursor: None,
            charset: CharsetState::default(),
            iso_protection: false,
            scrollback_limit,
            next_row: rows as u64,
            tracked: HashMap::new(),
            next_track: 0,
        }
    }

    pub fn all_rows(&self) -> impl DoubleEndedIterator<Item = &Row> {
        self.history.iter().chain(self.rows.iter())
    }

    pub fn viewport(&self) -> impl Iterator<Item = &Row> {
        let start = self.history.len().saturating_sub(self.viewport_offset);
        self.all_rows().skip(start).take(self.rows.len())
    }

    /// Copy only visible rows so shaping and GPU preparation need no session lock.
    pub fn snapshot_viewport(&self) -> Self {
        let mut cursor = self.cursor.clone();
        cursor.row = cursor.row.saturating_add(self.viewport_offset);
        cursor.visible &= cursor.row < self.rows.len();
        cursor.row = cursor.row.min(self.rows.len() - 1);
        Self {
            rows: self.viewport().cloned().collect(),
            history: VecDeque::new(),
            cursor,
            selection: self.selection,
            viewport_offset: 0,
            kitty_keyboard: self.kitty_keyboard.clone(),
            saved_cursor: None,
            charset: self.charset.clone(),
            iso_protection: self.iso_protection,
            scrollback_limit: 0,
            next_row: self.next_row,
            tracked: HashMap::new(),
            next_track: 0,
        }
    }

    pub fn scroll_viewport(&mut self, rows: isize) {
        self.viewport_offset = self
            .viewport_offset
            .saturating_add_signed(rows)
            .min(self.history.len());
    }

    pub fn row_by_id(&self, id: u64) -> Option<&Row> {
        self.all_rows().find(|r| r.id == id)
    }

    pub fn point(&self, row: usize, col: usize) -> Option<GridPoint> {
        let row = self.all_rows().nth(row)?;
        (col < row.cells.len()).then_some(GridPoint { row: row.id, col })
    }

    pub fn track(&mut self, point: GridPoint) -> TrackedPoint {
        let id = self.next_track;
        self.next_track = self.next_track.wrapping_add(1);
        let valid = self
            .row_by_id(point.row)
            .is_some_and(|r| point.col < r.cells.len());
        self.tracked.insert(id, valid.then_some(point));
        TrackedPoint(id)
    }

    pub fn resolve(&self, point: TrackedPoint) -> Option<GridPoint> {
        self.tracked.get(&point.0).copied().flatten()
    }

    pub fn untrack(&mut self, point: TrackedPoint) {
        self.tracked.remove(&point.0);
    }

    pub fn selection_text(&self) -> Option<String> {
        let selection = self.selection?;
        let rows: Vec<_> = self.all_rows().collect();
        let mut first = rows.iter().position(|r| r.id == selection.start.row)?;
        let mut last = rows.iter().position(|r| r.id == selection.end.row)?;
        let mut left = selection.start.col;
        let mut right = selection.end.col;
        if (first, left) > (last, right) {
            std::mem::swap(&mut first, &mut last);
            std::mem::swap(&mut left, &mut right);
        }
        let mut result = String::new();
        for (i, row) in rows.iter().enumerate().take(last + 1).skip(first) {
            let start = if selection.rectangular {
                left.min(right)
            } else if i == first {
                left
            } else {
                0
            };
            let end = if selection.rectangular {
                left.max(right) + 1
            } else if i == last {
                right + 1
            } else {
                row.cells.len()
            };
            let line_start = result.len();
            for cell in &row.cells[start.min(row.cells.len())..end.min(row.cells.len())] {
                if cell.width == 0 || cell.spacer_head {
                    continue;
                }
                if cell.text.is_empty() {
                    result.push(' ');
                } else {
                    result.push_str(&cell.text);
                }
            }
            if selection.rectangular || !row.wrapped {
                while result.len() > line_start && result.ends_with(' ') {
                    result.pop();
                }
                if i < last {
                    result.push('\n');
                }
            }
        }
        Some(result)
    }

    pub(crate) fn blank_row(&mut self, cols: usize, background: Color) -> Row {
        let id = self.next_row;
        self.next_row = self.next_row.wrapping_add(1);
        Row::new(id, cols, background)
    }

    pub(crate) fn discard_row(&mut self, id: u64) {
        for point in self.tracked.values_mut() {
            if point.is_some_and(|p| p.row == id) {
                *point = None;
            }
        }
        if self
            .selection
            .is_some_and(|s| s.start.row == id || s.end.row == id)
        {
            self.selection = None;
        }
    }

    pub(crate) fn push_history(&mut self, row: Row) {
        if self.scrollback_limit == 0 {
            self.discard_row(row.id);
            return;
        }
        self.history.push_back(row);
        if self.viewport_offset > 0 {
            self.viewport_offset += 1;
        }
        while self.history.len() > self.scrollback_limit {
            let row = self.history.pop_front().unwrap();
            self.discard_row(row.id);
        }
        self.viewport_offset = self.viewport_offset.min(self.history.len());
    }

    pub(crate) fn resize(&mut self, cols: usize, rows: usize, reflow: bool) {
        let old_cols = self.rows[0].cells.len();
        let old_cursor = GridPoint {
            row: self.rows[self.cursor.row].id,
            col: self.cursor.col,
        };
        let mut contents: Vec<Row> = self.history.drain(..).chain(self.rows.drain(..)).collect();
        let cursor_index = contents
            .iter()
            .position(|r| r.id == old_cursor.row)
            .unwrap();
        while contents.len() > rows
            && contents.len() > cursor_index + 1
            && contents.last().is_some_and(|r| r.used() == 0 && !r.wrapped)
        {
            contents.pop();
        }
        let mut mapped_cursor = old_cursor;
        if cols != old_cols && reflow {
            let mut map = HashMap::<GridPointKey, GridPoint>::new();
            let mut output = Vec::new();
            let mut line = self.blank_row(cols, Color::Default);
            let mut x: usize = 0;
            for old in &contents {
                let used = if old.wrapped {
                    old.cells.len()
                } else {
                    old.used().max(if old.id == old_cursor.row {
                        old_cursor.col + 1
                    } else {
                        0
                    })
                };
                for (old_col, cell) in old.cells.iter().take(used).enumerate() {
                    if cell.width == 0 {
                        let p = map
                            .get(&(old.id, old_col - 1))
                            .copied()
                            .unwrap_or(GridPoint {
                                row: line.id,
                                col: x.saturating_sub(1),
                            });
                        map.insert(
                            (old.id, old_col),
                            GridPoint {
                                col: (p.col + 1).min(cols - 1),
                                ..p
                            },
                        );
                        continue;
                    }
                    if cell.spacer_head {
                        continue;
                    }
                    let width = usize::from(cell.width).min(cols);
                    if x + width > cols {
                        line.wrapped = true;
                        output.push(line);
                        line = self.blank_row(cols, Color::Default);
                        x = 0;
                    }
                    map.insert(
                        (old.id, old_col),
                        GridPoint {
                            row: line.id,
                            col: x,
                        },
                    );
                    let mut cell = cell.clone();
                    cell.width = width as u8;
                    line.cells[x] = cell.clone();
                    if width == 2 {
                        cell.text.clear();
                        cell.width = 0;
                        line.cells[x + 1] = cell;
                    }
                    x += width;
                }
                // Empty rows and tracked blank columns still need a stable mapping.
                for old_col in used..old.cells.len() {
                    map.insert(
                        (old.id, old_col),
                        GridPoint {
                            row: line.id,
                            col: (x + old_col - used).min(cols - 1),
                        },
                    );
                }
                if !old.wrapped {
                    line.semantic = old.semantic;
                    output.push(line);
                    line = self.blank_row(cols, Color::Default);
                    x = 0;
                }
            }
            if x > 0 {
                output.push(line);
            }
            if let Some(p) = map.get(&(old_cursor.row, old_cursor.col)) {
                mapped_cursor = *p;
            }
            for point in self.tracked.values_mut() {
                *point = point.and_then(|p| map.get(&(p.row, p.col)).copied());
            }
            self.selection = self.selection.and_then(|s| {
                Some(Selection {
                    start: *map.get(&(s.start.row, s.start.col))?,
                    end: *map.get(&(s.end.row, s.end.col))?,
                    rectangular: s.rectangular,
                })
            });
            contents = output;
        } else if cols != old_cols {
            for row in &mut contents {
                row.cells.resize(cols, Cell::default());
                row.repair_wide(Color::Default);
                row.wrapped = false;
            }
            mapped_cursor.col = mapped_cursor.col.min(cols - 1);
        }
        while contents.len() < rows {
            contents.push(self.blank_row(cols, Color::Default));
        }
        let start = contents.len() - rows;
        self.cursor.row = contents
            .iter()
            .position(|r| r.id == mapped_cursor.row)
            .unwrap_or(start)
            .saturating_sub(start)
            .min(rows - 1);
        self.cursor.col = mapped_cursor.col.min(cols - 1);
        self.cursor.pending_wrap &= self.cursor.col == cols - 1;
        self.rows = contents.split_off(start);
        for row in contents {
            self.push_history(row);
        }
        self.viewport_offset = self.viewport_offset.min(self.history.len());
        if let Some(saved) = &mut self.saved_cursor {
            saved.cursor.row = saved.cursor.row.min(rows - 1);
            saved.cursor.col = saved.cursor.col.min(cols - 1);
        }
    }
}

type GridPointKey = (u64, usize);

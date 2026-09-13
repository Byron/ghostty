//! Selection queries over physical rows, including retained history.

use crate::{Cell, GridPoint, Row, Screen, Selection};

pub const DEFAULT_WORD_BOUNDARIES: &[char] = &[
    '\0', ' ', '\t', '\'', '"', '│', '`', '|', ':', ';', ',', '(', ')', '[', ']', '{', '}', '<',
    '>', '$',
];
pub const DEFAULT_LINE_WHITESPACE: &[char] = &['\0', ' ', '\t'];

#[derive(Clone, Copy, Debug)]
pub struct SelectLine<'a> {
    /// `None` includes unwritten cells. An empty set trims only unwritten cells.
    pub whitespace: Option<&'a [char]>,
    /// Stop at any prompt/input/output transition, including within one row.
    pub semantic_prompt_boundary: bool,
}

impl Default for SelectLine<'_> {
    fn default() -> Self {
        Self {
            whitespace: Some(DEFAULT_LINE_WHITESPACE),
            semantic_prompt_boundary: true,
        }
    }
}

impl Screen {
    /// Select the contiguous run with the clicked codepoint's boundary class.
    /// Unwritten cells cannot be selected. These queries leave the active
    /// selection unchanged; callers can apply the returned inclusive endpoints.
    pub fn select_word(&self, point: GridPoint, boundaries: &[char]) -> Option<Selection> {
        let grid = Grid(self);
        grid.word(grid.position(point)?, boundaries)
    }

    /// Select the first word encountered from `start` toward `end`, inclusive.
    /// The returned word itself may extend beyond the two search endpoints.
    pub fn select_word_between(
        &self,
        start: GridPoint,
        end: GridPoint,
        boundaries: &[char],
    ) -> Option<Selection> {
        let grid = Grid(self);
        let mut current = grid.position(start)?;
        let end = grid.position(end)?;
        let forward = current < end;
        loop {
            if let Some(selection) = grid.word(current, boundaries) {
                return Some(selection);
            }
            if current == end {
                return None;
            }
            current = if forward {
                grid.next(current)
            } else {
                grid.previous(current)
            }?;
        }
    }

    /// Select a logical line through soft wraps, with optional semantic bounds
    /// and endpoint whitespace trimming. Returns none for a trimmed empty line.
    pub fn select_line(&self, point: GridPoint, options: SelectLine<'_>) -> Option<Selection> {
        let grid = Grid(self);
        let clicked = grid.position(point)?;
        let semantic = options
            .semantic_prompt_boundary
            .then(|| grid.cell(clicked).semantic);

        let mut start = (clicked.0, 0);
        'start: {
            if let Some(semantic) = semantic {
                for x in (0..=clicked.1).rev() {
                    if grid.row(clicked.0).cells[x].semantic != semantic {
                        start.1 = x + 1;
                        break 'start;
                    }
                }
            }
            while start.0 > 0 {
                let y = start.0 - 1;
                let row = grid.row(y);
                if !row.wrapped {
                    break;
                }
                if let Some(semantic) = semantic {
                    for x in (0..row.cells.len()).rev() {
                        if row.cells[x].semantic != semantic {
                            break 'start;
                        }
                        start = (y, x);
                    }
                } else {
                    start = (y, 0);
                }
            }
        }

        let mut y = clicked.0;
        let mut end = 'end: loop {
            let row = grid.row(y);
            let from = if y == clicked.0 { clicked.1 } else { 0 };
            if let Some(semantic) = semantic {
                for x in from..row.cells.len() {
                    if row.cells[x].semantic != semantic {
                        break 'end grid.previous((y, x))?;
                    }
                }
            }
            if !row.wrapped {
                break (y, row.cells.len() - 1);
            }
            y += 1;
            if y == grid.len() {
                return None;
            }
        };

        if let Some(whitespace) = options.whitespace {
            let start_limit = start.0;
            let end_limit = end.0;
            // Native cellIterator bounds the final row, not the final column.
            // Keep that behavior when semantic boundaries fall within a row.
            while !codepoint(grid.cell(start)).is_some_and(|c| !whitespace.contains(&c)) {
                start = grid.next(start)?;
                if start.0 > end_limit {
                    return None;
                }
            }
            while !codepoint(grid.cell(end)).is_some_and(|c| !whitespace.contains(&c)) {
                end = grid.previous(end)?;
                if end.0 < start_limit {
                    return None;
                }
            }
        }
        Some(grid.selection(start, end))
    }

    /// Select all written content, excluding surrounding NUL, space, and tab.
    pub fn select_all(&self) -> Option<Selection> {
        let written =
            |cell: &Cell| codepoint(cell).is_some_and(|c| !DEFAULT_LINE_WHITESPACE.contains(&c));
        let start = self.all_rows().find_map(|row| {
            row.cells
                .iter()
                .position(written)
                .map(|col| GridPoint { row: row.id, col })
        })?;
        let end = self.all_rows().rev().find_map(|row| {
            row.cells
                .iter()
                .rposition(written)
                .map(|col| GridPoint { row: row.id, col })
        })?;
        Some(Selection {
            start,
            end,
            rectangular: false,
        })
    }
}

type Position = (usize, usize);

/// Constant-time row access while walking cells; row IDs only locate endpoints.
struct Grid<'a>(&'a Screen);

impl Grid<'_> {
    fn len(&self) -> usize {
        self.0.history.len() + self.0.rows.len()
    }

    fn row(&self, y: usize) -> &Row {
        if y < self.0.history.len() {
            &self.0.history[y]
        } else {
            &self.0.rows[y - self.0.history.len()]
        }
    }

    fn cell(&self, point: Position) -> &Cell {
        &self.row(point.0).cells[point.1]
    }

    fn position(&self, point: GridPoint) -> Option<Position> {
        let y = self.0.all_rows().position(|row| row.id == point.row)?;
        (point.col < self.row(y).cells.len()).then_some((y, point.col))
    }

    fn next(&self, (y, x): Position) -> Option<Position> {
        if x + 1 < self.row(y).cells.len() {
            Some((y, x + 1))
        } else {
            (y + 1 < self.len()).then_some((y + 1, 0))
        }
    }

    fn previous(&self, (y, x): Position) -> Option<Position> {
        if x > 0 {
            Some((y, x - 1))
        } else {
            y.checked_sub(1).map(|y| (y, self.row(y).cells.len() - 1))
        }
    }

    fn selection(&self, start: Position, end: Position) -> Selection {
        let point = |(y, col)| GridPoint {
            row: self.row(y).id,
            col,
        };
        Selection {
            start: point(start),
            end: point(end),
            rectangular: false,
        }
    }

    fn word(&self, point: Position, boundaries: &[char]) -> Option<Selection> {
        let boundary = boundaries.contains(&codepoint(self.cell(point))?);
        let same =
            |cell: &Cell| codepoint(cell).is_some_and(|c| boundaries.contains(&c) == boundary);
        let mut end = point;
        while let Some(next) = self.next(end) {
            if !same(self.cell(next)) {
                break;
            }
            end = next;
            // Match native's forward scan: this check follows the initial cell.
            if end.1 + 1 == self.row(end.0).cells.len() && !self.row(end.0).wrapped {
                break;
            }
        }
        let mut start = point;
        while let Some(previous) = self.previous(start) {
            let row = self.row(previous.0);
            if (previous.1 + 1 == row.cells.len() && !row.wrapped) || !same(self.cell(previous)) {
                break;
            }
            start = previous;
        }
        Some(self.selection(start, end))
    }
}

fn codepoint(cell: &Cell) -> Option<char> {
    cell.text.chars().next().filter(|&c| c != '\0')
}

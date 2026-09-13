//! Direct selection/search/tracked-reference APIs, independent of snapshots.
use rustty_vt::selection::{DEFAULT_LINE_WHITESPACE, DEFAULT_WORD_BOUNDARIES, SelectLine};
use rustty_vt::{GridPoint, Screen, ScrollbackLimits, Selection, Terminal, TrackedPoint};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Point {
    tag: String,
    x: u16,
    y: u32,
}

impl Default for Point {
    fn default() -> Self {
        Self {
            tag: "active".into(),
            x: 0,
            y: 0,
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Operation {
    action: String,
    id: u32,
    point: Point,
    start: Point,
    end: Point,
    rectangle: bool,
    needle: String,
    delta: i32,
    lines: Option<usize>,
    bytes: Option<usize>,
    boundary_codepoints: Option<Vec<u32>>,
    whitespace: Option<Vec<u32>>,
    trim_line: Option<bool>,
    semantic_prompt_boundary: Option<bool>,
}

struct Handle {
    id: u32,
    alternate: bool,
    point: TrackedPoint,
}

#[derive(Default)]
pub struct Context {
    handles: Vec<Handle>,
}

impl Context {
    pub fn has_handles(&self) -> bool {
        !self.handles.is_empty()
    }

    pub fn run(&mut self, terminal: &mut Terminal, op: &Operation) -> Result<Value, &'static str> {
        let mut status = "ok";
        let mut matches = None;
        let mut selection_result = None;
        let columns = terminal.cols;
        match op.action.as_str() {
            "observe" => {}
            "select" => {
                let screen = terminal.screen_mut();
                match (
                    point(screen, &op.start, columns)?,
                    point(screen, &op.end, columns)?,
                ) {
                    (Some(start), Some(end)) => {
                        screen.selection = Some(Selection {
                            start,
                            end,
                            rectangular: op.rectangle,
                        })
                    }
                    _ => status = "invalid",
                }
            }
            "clear_selection" => terminal.screen_mut().selection = None,
            "select_word"
            | "select_word_between"
            | "select_line"
            | "select_all"
            | "select_output" => {
                let screen = terminal.screen();
                let boundaries = codepoints(op.boundary_codepoints.as_deref())?;
                let boundaries = boundaries.as_deref().unwrap_or(DEFAULT_WORD_BOUNDARIES);
                let whitespace = codepoints(op.whitespace.as_deref())?;
                let selected = if op.action == "select_all" {
                    screen.select_all()
                } else if op.action == "select_word_between" {
                    match (
                        point(screen, &op.start, columns)?,
                        point(screen, &op.end, columns)?,
                    ) {
                        (Some(start), Some(end)) => {
                            screen.select_word_between(start, end, boundaries)
                        }
                        _ => {
                            status = "invalid";
                            None
                        }
                    }
                } else if let Some(point) = point(screen, &op.point, columns)? {
                    if op.action == "select_word" {
                        screen.select_word(point, boundaries)
                    } else if op.action == "select_output" {
                        screen.select_output(point)
                    } else {
                        screen.select_line(
                            point,
                            SelectLine {
                                whitespace: op.trim_line.unwrap_or(true).then_some(
                                    whitespace.as_deref().unwrap_or(DEFAULT_LINE_WHITESPACE),
                                ),
                                semantic_prompt_boundary: op
                                    .semantic_prompt_boundary
                                    .unwrap_or(true),
                            },
                        )
                    }
                } else {
                    status = "invalid";
                    None
                };
                selection_result = selected.map(|selection| json!({
                    "start": location(screen, selection.start), "end": location(screen, selection.end),
                    "rectangle": selection.rectangular,
                }));
                if selected.is_none() && status == "ok" {
                    status = "no_value";
                }
            }
            "track" => {
                if self.handles.iter().any(|handle| handle.id == op.id) {
                    status = "invalid";
                } else if let Some(point) = point(terminal.screen(), &op.point, columns)? {
                    let alternate = terminal.is_alternate_screen();
                    self.handles.push(Handle {
                        id: op.id,
                        alternate,
                        point: terminal.screen_mut().track(point),
                    });
                } else {
                    status = "invalid";
                }
            }
            "untrack" => {
                if let Some(index) = self.handles.iter().position(|handle| handle.id == op.id) {
                    let handle = &self.handles[index];
                    terminal.untrack(handle.point);
                    self.handles.remove(index);
                } else {
                    status = "invalid";
                }
            }
            "viewport" => terminal.screen_mut().scroll_viewport(-(op.delta as isize)),
            "limits" => terminal.set_limits(ScrollbackLimits {
                lines: op.lines,
                bytes: op.bytes,
            }),
            "search" => {
                let needle = super::unhex(&op.needle)?;
                // Native search exposes a literal needle. Retain the Rust
                // API's result order so ordering differences cannot disappear.
                matches = Some(
                    terminal
                        .screen()
                        .search_literal(&needle)
                        .into_iter()
                        .map(|value| {
                            json!({"start": location(terminal.screen(), value.start),
                           "end": location(terminal.screen(), value.end)})
                        })
                        .collect::<Vec<_>>(),
                );
            }
            _ => return Err("UnsupportedGridAction"),
        }
        let screen = terminal.screen();
        let selection = screen.selection.map(|selection| {
            json!({"start": location(screen, selection.start), "end": location(screen, selection.end),
                   "rectangle": selection.rectangular,
                   "text": screen.selection_text().map(|text| super::hex(text.as_bytes()))})
        });
        let handles = self.handles.iter().map(|handle| {
            let screen = if handle.alternate { terminal.alternate_screen() } else { Some(terminal.primary_screen()) };
            let point = screen.and_then(|screen| screen.resolve(handle.point));
            let value = screen.zip(point).map(|(screen, point)| {
                let text = screen.row_by_id(point.row).and_then(|row| row.cells.get(point.col))
                    .map(|cell| cell.text.chars().map(u32::from).collect::<Vec<_>>());
                json!({"location": location(screen, point), "text": text})
            });
            json!({"id": handle.id, "screen": if handle.alternate { "alternate" } else { "primary" }, "value": value})
        }).collect::<Vec<_>>();
        Ok(
            json!({"action": op.action, "status": status, "matches": matches,
            "active_screen": if terminal.is_alternate_screen() { "alternate" } else { "primary" },
            "viewport_top": [0, screen.history.len().saturating_sub(screen.viewport_offset)],
            "selection": selection, "selection_result": selection_result, "tracked": handles}),
        )
    }
}

fn codepoints(values: Option<&[u32]>) -> Result<Option<Vec<char>>, &'static str> {
    values
        .map(|values| {
            values
                .iter()
                .map(|&value| char::from_u32(value).ok_or("InvalidCodepoint"))
                .collect()
        })
        .transpose()
}

fn point(screen: &Screen, point: &Point, columns: u16) -> Result<Option<GridPoint>, &'static str> {
    // Native PageList.pin validates the logical width before resolving a page.
    // A selector may still return an endpoint beyond it on a wider stored row.
    if point.x >= columns {
        return Ok(None);
    }
    let base = match point.tag.as_str() {
        "active" => screen.history.len(),
        "viewport" => screen.history.len().saturating_sub(screen.viewport_offset),
        "screen" | "history" => 0,
        _ => return Err("InvalidPointTag"),
    };
    Ok(base
        .checked_add(point.y as usize)
        .and_then(|row| screen.point(row, usize::from(point.x))))
}

fn location(screen: &Screen, point: GridPoint) -> Value {
    let Some(row) = screen.all_rows().position(|row| row.id == point.row) else {
        return Value::Null;
    };
    let relative = |base| row.checked_sub(base).map(|row| [point.col, row]);
    json!({"screen": [point.col, row], "history": [point.col, row],
        "active": relative(screen.history.len()),
        "viewport": relative(screen.history.len().saturating_sub(screen.viewport_offset))})
}

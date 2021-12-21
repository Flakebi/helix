use std::fmt::Write;

use crate::{editor::Config, graphics::Style, Document, Theme, View};

pub type GutterFn<'doc> = Box<dyn Fn(usize, bool, &mut String) -> Option<Style> + 'doc>;

pub struct Gutter {
    pub render: for<'doc> fn(&'doc Document, &View, &Theme, &Config, bool) -> GutterFn<'doc>,
    pub width: fn(&View, &Config, &Document) -> usize,
}
// pub type Gutter =
// for<'doc> fn(&'doc Document, &View, &Theme, &Config, bool, usize) -> GutterFn<'doc>;

pub const DIAGNOSTIC_GUTTER: Gutter = Gutter {
    render: diagnostic_render,
    width: |_, _, _| 1,
};

/// Computes the number of decimal digits needed to print a number.
pub fn digits10(n: usize) -> usize {
    std::iter::successors(Some(n), |n| {
        let n = n / 10;
        (n != 0).then(|| n)
    })
    .count()
}
pub fn diagnostic_render<'doc>(
    doc: &'doc Document,
    _view: &View,
    theme: &Theme,
    _config: &Config,
    _is_focused: bool,
) -> GutterFn<'doc> {
    let warning = theme.get("warning");
    let error = theme.get("error");
    let info = theme.get("info");
    let hint = theme.get("hint");
    let diagnostics = doc.diagnostics();

    Box::new(move |line: usize, _selected: bool, out: &mut String| {
        use helix_core::diagnostic::Severity;
        if let Ok(index) = diagnostics.binary_search_by_key(&line, |d| d.line) {
            let diagnostic = &diagnostics[index];
            write!(out, "●").unwrap();
            return Some(match diagnostic.severity {
                Some(Severity::Error) => error,
                Some(Severity::Warning) | None => warning,
                Some(Severity::Info) => info,
                Some(Severity::Hint) => hint,
            });
        }
        None
    })
}

pub const LINE_NUMBER_GUTTER: Gutter = Gutter {
    render: line_number_render,
    width: line_number_width,
};

fn line_number_width(_view: &View, config: &Config, doc: &Document) -> usize {
    if config.line_number == crate::editor::LineNumber::None {
        0
    } else {
        digits10(doc.text().len_lines())
    }
}

fn line_number_render<'doc>(
    doc: &'doc Document,
    view: &View,
    theme: &Theme,
    config: &Config,
    is_focused: bool,
) -> GutterFn<'doc> {
    let width = line_number_width(view, config, doc);
    let text = doc.text().slice(..);
    let last_line = view.last_line(doc);
    // Whether to draw the line number for the last line of the
    // document or not.  We only draw it if it's not an empty line.
    let draw_last = text.line_to_byte(last_line) < text.len_bytes();

    let linenr = theme.get("ui.linenr");
    let linenr_select: Style = theme.try_get("ui.linenr.selected").unwrap_or(linenr);

    let current_line = doc
        .text()
        .char_to_line(doc.selection(view.id).primary().cursor(text));

    let config = config.line_number;

    Box::new(move |line: usize, selected: bool, out: &mut String| {
        use crate::editor::LineNumber;
        if config == LineNumber::None {
            return None;
        }

        if line == last_line && !draw_last {
            write!(out, "{:>1$}", '~', width).unwrap();
            Some(linenr)
        } else {
            let line = match config {
                LineNumber::Absolute => line + 1,
                LineNumber::Relative => {
                    if current_line == line {
                        line + 1
                    } else {
                        abs_diff(current_line, line)
                    }
                }
                LineNumber::None => unreachable!(),
            };
            let style = if selected && is_focused {
                linenr_select
            } else {
                linenr
            };
            write!(out, "{:>1$}", line, width).unwrap();
            Some(style)
        }
    })
}

#[inline(always)]
const fn abs_diff(a: usize, b: usize) -> usize {
    if a > b {
        a - b
    } else {
        b - a
    }
}

use bstr::{BString};

pub enum ScrollPosition {
    Line(usize),
    StickyBottom,
}

pub struct Scrolled {
    pub ranges: Vec<(usize, ((usize, usize), usize))>,
    pub in_view: std::ops::Range<usize>,
}

pub fn wrap(
    lines: &[BString],
    max_width: usize,
    max_height: usize,
    initial_indent: usize,
    scroll: ScrollPosition,
) -> Scrolled {

    let lineno = match scroll {
        ScrollPosition::Line(lineno) => lineno.min(lines.len().saturating_sub(1)),
        ScrollPosition::StickyBottom => lines.len().saturating_sub(1),
    };

    let ranges: Vec<_> = lines.iter()
        .enumerate()
        .flat_map(|(lineno, line)|
            super::wrap::wrap(line.as_ref(), max_width, initial_indent)
            .map(move |x| (lineno, x))
        ).collect();

    let start = ranges.partition_point(|x| x.0 < lineno);

    let mut start = start.saturating_sub(max_height / 2);
    let end = (start + max_height).min(ranges.len());
    if end - start < max_height {
        start = end.saturating_sub(max_height);
    }

    Scrolled {
        ranges,
        in_view: start .. end,
    }
}


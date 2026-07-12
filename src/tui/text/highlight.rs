use std::ops::Range;
use bstr::{BString, BStr};
use crate::tui::{Style};

#[derive(Debug, Clone, Default)]
pub struct Highlight<T, S=BString> {
    pub style: Style,
    pub blend: bool,
    pub namespace: T,
    pub virtual_text: Option<S>,
    pub conceal: Option<bool>,
    pub priority: f64,
}

impl<T, S: AsRef<BStr>> Highlight<T, S> {
    pub fn is_empty(&self) -> bool {
        self.style == Style::default()
        && self.virtual_text.as_ref().is_none_or(|s| s.as_ref().is_empty())
        && !self.conceal.unwrap_or_default()
    }

    pub fn may_cause_resize(&self) -> bool {
        // only conceal and virtual text affect sizing
        self.conceal.unwrap_or_default() || self.has_virtual_text()
    }

    pub fn has_virtual_text(&self) -> bool {
        self.virtual_text.as_ref().is_some_and(|x| !x.as_ref().is_empty())
    }
}

impl<T: Default> From<Style> for Highlight<T> {
    fn from(style: Style) -> Self {
        Self {
            style,
            blend: true,
            namespace: T::default(),
            virtual_text: None,
            conceal: None,
            priority: 0.,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HighlightedRange<T, S=BString> {
    pub parano: usize,
    pub start: usize,
    pub end: usize,
    pub inner: Highlight<T, S>,
}

impl<T, S> HighlightedRange<T, S> {
    pub fn shift(&mut self, range: Range<usize>, new_end: usize) {
        if range.end <= self.start {
            self.start = self.start.saturating_add(new_end) - range.end;
        } else if range.start <= self.start {
            self.start = new_end;
        }

        if range.end < self.end {
            self.end = self.end.saturating_add(new_end) - range.end;
        } else if range.start < self.end {
            self.end = new_end;
        }

        self.start = self.start.min(self.end);
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end && self.inner.virtual_text.is_none()
    }

    pub fn namespace(&self) -> &T {
        &self.inner.namespace
    }
}

impl<T: Clone, S: Clone> HighlightedRange<T, S> {
    pub fn split(&mut self, index: usize) -> Option<Self> {
        if (self.start .. self.end).contains(&index) {
            let mut other = self.clone();
            other.inner.virtual_text = None;
            other.start = index;
            self.end = index;
            Some(other)
        } else {
            None
        }
    }
}

impl<T, S> PartialEq for HighlightedRange<T, S> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl<T, S> Eq for HighlightedRange<T, S> {}

impl<T, S> PartialOrd for HighlightedRange<T, S> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T, S> Ord for HighlightedRange<T, S> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // sort in reverse order of priority so higher priority comes first
        self.parano.cmp(&other.parano).then(self.inner.priority.total_cmp(&other.inner.priority).reverse())
    }
}

#[derive(Debug, Default, Clone)]
pub struct HighlightedRangeSet<T, S=BString> {
    inner: Vec<HighlightedRange<T, S>>
}
crate::impl_deref_helper!(self: HighlightedRangeSet<T, S>, &self.inner => Vec<HighlightedRange<T, S>>);
crate::impl_deref_helper!(mut self: HighlightedRangeSet<T, S>, &mut self.inner => Vec<HighlightedRange<T, S>>);

impl<T, S> HighlightedRangeSet<T, S> {

    pub fn push(&mut self, hl: HighlightedRange<T, S>) {
        // sort in reverse order of priority so higher priority comes first
        let index = match self.binary_search(&hl) {
            Ok(index) | Err(index) => index,
        };
        self.inner.insert(index, hl);
    }

    pub fn index_for_parano(&self, parano: usize) -> Result<usize, usize> {
        let mut index = self.binary_search_by(|x| x.parano.cmp(&parano))?;
        // find the start by searching backwards
        while index > 0 && self.get(index-1).is_some_and(|x| x.parano == parano) {
            index -= 1;
        }
        Ok(index)
    }

    pub fn get_range_for_lines(&self, range: Range<usize>) -> Range<usize> {
        match self.index_for_parano(range.start) {
            Ok(start) => {
                let end = start + self[start..].partition_point(|x| x.parano < range.end);
                start .. end
            },
            Err(start) => start .. start,
        }
    }

    pub fn get_for_parano(&self, parano: usize) -> &[HighlightedRange<T, S>] {
        let range = self.get_range_for_lines(parano .. parano + 1);
        // ::log::debug!("DEBUG(purge) \t{}\t= {:?}", stringify!((parano, range, &self[range])), (parano, range, &self[range]));
        &self[range]
    }
}

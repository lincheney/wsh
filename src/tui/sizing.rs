use std::num::NonZero;

type Unit = u16;
pub type Flex = NonZero<Unit>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Metric {
    Fixed(Unit),
    Percent(Unit),
}

impl Metric {
    fn resolve(self, available: Option<Unit>) -> Unit {
        match (self, available) {
            (Self::Fixed(x), _) => x,
            (Self::Percent(x), Some(available)) => ((available * x) as f64 / 100.) as Unit,
            _ => 0, // this is good default?
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Constraint {
    pub min:  Option<Metric>,
    pub max:  Option<Metric>,
    pub flex: Option<NonZero<Unit>>,
}

impl Constraint {
    pub fn into_size(self, available: Option<Unit>, min: Option<Unit>) -> Size {
        let max = self.max.map(|c| c.resolve(available));
        let mut size = self.min.map(|c| c.resolve(available)).unwrap_or(0).max(min.unwrap_or(0));
        // clamp to the max
        if let Some(max) = max && size > max {
            size = max;
        }

        Size {
            size,
            max,
            flex: self.flex,
        }
    }
}

#[derive(Debug)]
pub struct Size {
    pub size: Unit,
    pub max: Option<Unit>,
    flex: Option<NonZero<Unit>>,
}

impl Size {
    fn apply_flex_unit(&mut self, flex_unit: f64) -> bool {
        let Some(flex) = self.flex
            else { return true };

        let new_value = flex_unit * flex.get() as f64;
        if new_value < self.size as f64 {
            false
        } else if let Some(max) = self.max && max <= new_value as Unit {
            self.size = max;
            false
        } else {
            self.size = new_value as Unit;
            true
        }
    }
}

pub struct SizeArray<'a>(pub &'a mut [Size]);

impl SizeArray<'_> {
    fn current_size(&self) -> Unit {
        self.0.iter().map(|s| s.size).sum()
    }

    fn current_non_flex_size(&self) -> Unit {
        self.0.iter().filter(|s| s.flex.is_none()).map(|s| s.size).sum()
    }

    fn flex_total(&self) -> Unit {
        self.0.iter().filter_map(|s| s.flex).map(|f| f.get()).sum()
    }

    fn flex_unit(&self, available: Unit) -> Option<f64> {
        let flex_total = self.flex_total();
        if flex_total == 0 {
            None
        } else {
            Some((available - self.current_non_flex_size()) as f64 / flex_total as f64)
        }
    }

    // distribute `total` cells among slots according to per-slot flex weights,
    // lower bounds, and upper bounds. converges by fixing slots that hit their
    // bounds and redistributing remaining space among the rest.
    pub fn allocate(&mut self, available: Option<Unit>) {

        if let Some(available) = available {

            if self.current_size() >= available {
                // already run out of space
                return
            }

            // allocate remaining space to flex
            while let Some(flex_unit) = self.flex_unit(available) {

                let mut recalc = false;
                for s in self.0.iter_mut() {
                    if !s.apply_flex_unit(flex_unit) {
                        // flex outside of bounds, so exclude it from flex
                        s.flex = None;
                        recalc = true;
                    }
                }

                if !recalc {
                    break
                }
            }

            let mut remaining = available.saturating_sub(self.current_size());
            if remaining == 0 {
                // already run out of space
                return
            }

            // allocate remaining space to anything else that can keep going up
            while self.0.iter_mut()
                    .filter(|s| s.flex.is_none() && s.max.is_none_or(|max| s.size < max))
                    .take(remaining as _)
                    .map(|s| {
                        s.size += 1;
                        remaining -= 1;
                    }).count() > 0
            {
            }

        } else {
            // available is None, ie unlimited

            // max everyone's height
            let mut flex_unit = 0f64;
            for s in self.0.iter_mut() {
                let size = s.max.unwrap_or(s.size);
                if let Some(flex) = s.flex {
                    flex_unit = flex_unit.min(size as f64 / flex.get() as f64);
                } else {
                    s.size = size;
                }
            }

            // max the flex ones
            for s in self.0.iter_mut() {
                s.apply_flex_unit(flex_unit);
            }
        }
    }
}

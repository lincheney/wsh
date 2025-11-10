use crossterm::{
    queue,
    cursor::{MoveDown, MoveToColumn},
    style::{
        Print,
        SetAttribute,
        Attribute as CAttribute,
        SetColors,
        Color as CColor,
        Colors,
        SetForegroundColor,
        SetBackgroundColor,
        SetUnderlineColor,
    },
};
use ratatui::{
    style::{Color, Modifier},
    layout::Position,
    buffer::Cell,
};

struct ModifierDiff {
    pub from: Modifier,
    pub to: Modifier,
}

impl ModifierDiff {
    fn queue<W>(self, mut w: W) -> std::io::Result<()>
    where
        W: std::io::Write,
    {
        //use crossterm::Attribute;
        let removed = self.from - self.to;
        if removed.contains(Modifier::REVERSED) {
            queue!(w, SetAttribute(CAttribute::NoReverse))?;
        }
        if removed.contains(Modifier::BOLD) {
            queue!(w, SetAttribute(CAttribute::NormalIntensity))?;
            if self.to.contains(Modifier::DIM) {
                queue!(w, SetAttribute(CAttribute::Dim))?;
            }
        }
        if removed.contains(Modifier::ITALIC) {
            queue!(w, SetAttribute(CAttribute::NoItalic))?;
        }
        if removed.contains(Modifier::UNDERLINED) {
            queue!(w, SetAttribute(CAttribute::NoUnderline))?;
        }
        if removed.contains(Modifier::DIM) {
            queue!(w, SetAttribute(CAttribute::NormalIntensity))?;
        }
        if removed.contains(Modifier::CROSSED_OUT) {
            queue!(w, SetAttribute(CAttribute::NotCrossedOut))?;
        }
        if removed.contains(Modifier::SLOW_BLINK) || removed.contains(Modifier::RAPID_BLINK) {
            queue!(w, SetAttribute(CAttribute::NoBlink))?;
        }

        let added = self.to - self.from;
        if added.contains(Modifier::REVERSED) {
            queue!(w, SetAttribute(CAttribute::Reverse))?;
        }
        if added.contains(Modifier::BOLD) {
            queue!(w, SetAttribute(CAttribute::Bold))?;
        }
        if added.contains(Modifier::ITALIC) {
            queue!(w, SetAttribute(CAttribute::Italic))?;
        }
        if added.contains(Modifier::UNDERLINED) {
            queue!(w, SetAttribute(CAttribute::Underlined))?;
        }
        if added.contains(Modifier::DIM) {
            queue!(w, SetAttribute(CAttribute::Dim))?;
        }
        if added.contains(Modifier::CROSSED_OUT) {
            queue!(w, SetAttribute(CAttribute::CrossedOut))?;
        }
        if added.contains(Modifier::SLOW_BLINK) {
            queue!(w, SetAttribute(CAttribute::SlowBlink))?;
        }
        if added.contains(Modifier::RAPID_BLINK) {
            queue!(w, SetAttribute(CAttribute::RapidBlink))?;
        }

        Ok(())
    }
}

pub fn draw<'a, W: std::io::Write, I>(
    mut writer: &mut W,
    width: u16,
    content: I,
) -> std::io::Result<()>
where
    I: Iterator<Item = (u16, u16, &'a Cell)>,
{

    let mut fg = Color::Reset;
    let mut bg = Color::Reset;
    let mut underline_color = Color::Reset;
    let mut modifier = Modifier::empty();
    let mut next_pos = Position{ x: 0, y: 0 };

    for (x, y, cell) in content {

        // this is the bit thats different
        // Move the cursor if the previous location was not (x - 1, y)
        if y != next_pos.y {
            queue!(writer, MoveDown(y - next_pos.y))?;
        }
        if x != next_pos.x {
            queue!(writer, MoveToColumn(x))?;
        }

        next_pos = Position { x: x+1, y };
        if next_pos.x >= width {
            next_pos = Position { x: 0, y: y + 1 };
        }

        if cell.modifier != modifier {
            let diff = ModifierDiff {
                from: modifier,
                to: cell.modifier,
            };
            diff.queue(&mut writer)?;
            modifier = cell.modifier;
        }
        if cell.fg != fg || cell.bg != bg {
            queue!(
                writer,
                SetColors(Colors::new(cell.fg.into(), cell.bg.into()))
            )?;
            fg = cell.fg;
            bg = cell.bg;
        }

        if cell.underline_color != underline_color {
            let color = CColor::from(cell.underline_color);
            queue!(writer, SetUnderlineColor(color))?;
            underline_color = cell.underline_color;
        }

        queue!(writer, Print(cell.symbol()))?;
    }

    queue!(
        writer,
        SetForegroundColor(CColor::Reset),
        SetBackgroundColor(CColor::Reset),
        SetUnderlineColor(CColor::Reset),
        SetAttribute(CAttribute::Reset),
    )
}

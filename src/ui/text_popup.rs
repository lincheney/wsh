use iocraft::prelude::*;
use taffy::{AvailableSpace, Size};
use unicode_width::UnicodeWidthStr;

#[derive(Default, Props, Clone)]
pub struct TextPopupProps {
    /// The color to make the text.
    pub color: Option<Color>,
    /// The content of the text.
    pub content: String,
    /// The weight of the text.
    pub weight: Weight,
    /// The text decoration.
    pub decoration: TextDecoration,
}

#[derive(Default)]
pub struct TextPopup {
    content: String,
    style: CanvasTextStyle,
}

fn wrap(
    content: &str,
    known_width: Option<f32>,
    available_width: AvailableSpace,
) -> Vec<std::borrow::Cow<str>>
{
    let width = match (known_width, available_width) {
        (Some(w), _) => w as usize,
        (None, AvailableSpace::MaxContent) => usize::MAX,
        (None, AvailableSpace::Definite(w)) => w as usize,
        (None, AvailableSpace::MinContent) => 1,
    };
    // no word splitting
    let options = textwrap::Options::new(width)
        .word_separator(textwrap::WordSeparator::Custom(|line| {
            Box::new(std::iter::once(textwrap::core::Word::from(line)))
        }));
    textwrap::wrap(content, options)
}

impl Component for TextPopup {
    type Props<'a> = TextPopupProps;

    fn new(_props: &Self::Props<'_>) -> Self {
        Self::default()
    }

    fn update(
        &mut self,
        props: &mut Self::Props<'_>,
        _hooks: Hooks,
        updater: &mut ComponentUpdater,
    ) {
        self.style = CanvasTextStyle::default();
        self.style.color = props.color;
        self.style.weight = props.weight;
        self.style.underline = props.decoration == TextDecoration::Underline;
        self.content = props.content.clone();

        let content = self.content.clone();
        updater.set_measure_func(Box::new(move |known_size, available_space, _| {
            let lines = wrap(&content, known_size.width, available_space.width);
            Size {
                width: lines.iter().map(|s| (&**s).width()).max().unwrap() as _,
                height: lines.len().max(1) as _,
            }
        }));
    }

    fn draw(&mut self, drawer: &mut ComponentDrawer<'_>) {
        let Size{ width, height } = drawer.layout().size;
        let content = wrap(&self.content, None, AvailableSpace::Definite(width));
        let starty = content.len().saturating_sub(height as _);
        for (y, line) in content[starty..].iter().enumerate() {
            drawer.canvas().set_text(0, y as _, line, self.style);
        }
    }

}

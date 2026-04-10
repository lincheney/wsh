#[derive(Debug, PartialEq, Eq, Copy, Clone, Hash)]
pub enum Button {
    Left,
    Right,
    Middle,
    Button(usize),
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub struct MouseEvent {
    pub mouse: Mouse,
    pub modifiers: super::Modifiers,
    pub x: usize,
    pub y: usize,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone, Hash)]
pub enum Mouse {
    Button{button: Button, release: bool},
    Move{button: Button},
    Scroll{down: bool},
}

impl Mouse {
    pub fn parse_from_label(key: &str) -> Option<Self> {
        Some(match key {
            "leftmouse" => Self::Button{button: Button::Left, release: false},
            "rightmouse" => Self::Button{button: Button::Right, release: false},
            "middlemouse" => Self::Button{button: Button::Middle, release: false},
            "leftmouse-release" => Self::Button{button: Button::Left, release: true},
            "rightmouse-release" => Self::Button{button: Button::Right, release: true},
            "middlemouse-release" => Self::Button{button: Button::Middle, release: true},

            "leftmouse-move" => Self::Move{button: Button::Left},
            "rightmouse-move" => Self::Move{button: Button::Right},
            "middlemouse-move" => Self::Move{button: Button::Middle},

            "scrolldown" => Self::Scroll{down: true},
            "scrollup" => Self::Scroll{down: false},

            key if key.starts_with("button") => {
                if key.ends_with("-release") && let Ok(n) = key["button".len() .. key.len() - "-release".len()].parse() {
                    Self::Button{button: Button::Button(n), release: true}
                } else if key.ends_with("-move") && let Ok(n) = key["button".len() .. key.len() - "-move".len()].parse() {
                    Self::Move{button: Button::Button(n)}
                } else if let Ok(n) = key["button".len() .. ].parse() {
                    Self::Button{button: Button::Button(n), release: false}
                } else {
                    return None
                }
            },

            _ => return None,
        })
    }
}

impl std::fmt::Display for Mouse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        match self {
            Self::Button{button: Button::Left, release: true} => write!(f, "leftmouse-release"),
            Self::Button{button: Button::Left, ..} => write!(f, "leftmouse"),
            Self::Button{button: Button::Right, release: true} => write!(f, "rightmouse-release"),
            Self::Button{button: Button::Right, ..} => write!(f, "rightmouse"),
            Self::Button{button: Button::Middle, release: true} => write!(f, "middlemouse-release"),
            Self::Button{button: Button::Middle, ..} => write!(f, "middlemouse"),
            Self::Button{button: Button::Button(n), release: true} => write!(f, "button{n}-release"),
            Self::Button{button: Button::Button(n), ..} => write!(f, "button{n}"),
            Self::Move{button: Button::Left} => write!(f, "leftmouse-move"),
            Self::Move{button: Button::Right} => write!(f, "rightmouse-move"),
            Self::Move{button: Button::Middle} => write!(f, "middlemouse-move"),
            Self::Move{button: Button::Button(n)} => write!(f, "button{n}-move"),
            Self::Scroll{down: true} => write!(f, "scrolldown"),
            Self::Scroll{..} => write!(f, "scrollup"),
        }
    }
}

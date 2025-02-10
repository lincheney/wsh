use iocraft::prelude::*;
use mlua::{prelude::*, UserData, UserDataMethods, MetaMethod};
use anyhow::Result;
use crossterm::{
    cursor,
    style,
    queue,
};
use crate::ui::Ui;
use crate::shell::Shell;
use crate::utils::*;
use super::text_popup::{TextPopup, TextPopupProps};

#[derive(Clone)]
pub struct ChildView(ArcMutex<ChildViewInner>);

struct ChildViewInner {
    view: Element<'static, View>,
    text: TextPopupProps,
    remove: bool,
}

#[derive(Default)]
pub struct Views {
    children: Vec<ChildView>,
    buffer: Vec<u8>,
}

impl Views {
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    pub fn clear(&mut self) {
        self.children.clear();
    }

    pub fn add(&mut self, string: String) -> ChildView {
        let view = element! {
            View(
                border_style: BorderStyle::Round,
                border_color: Color::Blue,
                max_height: 10,
            )
        };

        let mut text = TextPopupProps::default();
        text.content = string;

        let view = ChildViewInner{
            view,
            text,
            remove: false,
        };
        let view = ChildView(ArcMutexNew!(view));
        self.children.push(view.clone());
        view
    }

    pub fn draw(&mut self, stdout: &mut std::io::Stdout, width: u16, _height: u16) -> Result<()> {
        self.children.retain(|view| !view.0.lock().unwrap().remove);
        if self.is_empty() {
            return Ok(())
        }

        self.buffer.clear();

        let mut canvas_height = 0;
        for child in self.children.iter() {
            let mut child = child.0.lock().unwrap();
            let mut text = element! { TextPopup };
            text.props = TextPopupProps{ content: child.text.content.clone(), ..child.text };

            // update the width
            child.view.props.max_width = iocraft::Size::Length(width as _);
            child.view.props.children.clear();
            child.view.props.children.push(text.into());

            let canvas = child.view.render(Some(width as _));
            canvas_height += canvas.height();
            canvas.write_ansi(&mut self.buffer)?;
        }
        let output = std::str::from_utf8(&self.buffer)?;

        for _ in 0 .. canvas_height as _ {
            queue!(stdout, style::Print("\n"))?;
        }
        queue!(
            stdout,
            cursor::MoveUp(canvas_height as _),
            cursor::SavePosition,
            cursor::MoveToNextLine(1),
            style::Print(output.trim_end()),
            cursor::RestorePosition,
        )?;
        Ok(())
    }

}

struct AnyElement<'a> {
    key: ElementKey,
    props: AnyProps<'a>,
    helper: Box<dyn ComponentHelperExt>,
}

impl UserData for ChildView {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        macro_rules! make_setter {
            ($name:ident, $type:ty, $body:expr) => {
                methods.add_method(concat!("set_", stringify!($name)), |_lua, child, $name: $type| {
                    (|| {
                        let val = $body;
                        let mut child = child.0.lock().unwrap();
                        child.view.props.$name = val;
                        Ok(())
                    })().map_err(|e: $type| {
                        mlua::Error::RuntimeError(format!(concat!("invalid ", stringify!($name), " {:?}"), e))
                    })
                });
            };
            (text, $name:ident, $type:ty, $body:expr) => {
                methods.add_method(concat!("set_text_", stringify!($name)), |_lua, child, $name: $type| {
                    (|| {
                        let val = $body;
                        let mut child = child.0.lock().unwrap();
                        child.text.$name = val;
                        Ok(())
                    })().map_err(|e: $type| {
                        mlua::Error::RuntimeError(format!(concat!("invalid ", stringify!($name), " {:?}"), e))
                    })
                });
            };
        }

        fn parse_colour(color: u32) -> Option<Color> {
            if color > 0xffffff {
                return None
            } else {
                Some(Color::Rgb{
                    r: (color >> 16) as _,
                    g: ((color >> 8) & 0xff) as _,
                    b: (color & 0xff) as _,
                })
            }
        }

        macro_rules! parse_length {
            ($value:expr, $type:ty) => {
                match $value {
                    None => Some(<$type>::Unset),
                    Some(value) if value <= 0. => None,
                    Some(value) if value > 1. => Some(<$type>::Length(value as _)),
                    Some(value) => Some(<$type>::Length(value as _)),
                }
            };
        }

        make_setter!(border_style, String, match border_style.as_str() {
            "None" => BorderStyle::None,
            "Single" => BorderStyle::Single,
            "Double" => BorderStyle::Double,
            "Round" => BorderStyle::Round,
            "Bold" => BorderStyle::Bold,
            "DoubleLeftRight" => BorderStyle::DoubleLeftRight,
            "DoubleTopBottom" => BorderStyle::DoubleTopBottom,
            "Classic" => BorderStyle::Classic,
            _ => return Err(border_style),
        });
        make_setter!(border_edges, Option<String>, match border_edges.as_ref().map(|s| s.as_str()) {
            Some("Top") => Some(Edges::Top),
            Some("Right") => Some(Edges::Right),
            Some("Bottom") => Some(Edges::Bottom),
            Some("Left") => Some(Edges::Left),
            None => None,
            _ => return Err(border_edges),
        });
        make_setter!(border_color, u32, Some(parse_colour(border_color).ok_or(border_color)?));
        make_setter!(background_color, u32, Some(parse_colour(background_color).ok_or(background_color)?));
        make_setter!(width, Option<f32>, parse_length!(width, Size).ok_or(width)?);
        make_setter!(height, Option<f32>, parse_length!(height, Size).ok_or(height)?);
        make_setter!(padding, Option<f32>, parse_length!(padding, Padding).ok_or(padding)?);
        make_setter!(padding_top, Option<f32>, parse_length!(padding_top, Padding).ok_or(padding_top)?);
        make_setter!(padding_right, Option<f32>, parse_length!(padding_right, Padding).ok_or(padding_right)?);
        make_setter!(padding_bottom, Option<f32>, parse_length!(padding_bottom, Padding).ok_or(padding_bottom)?);
        make_setter!(padding_left, Option<f32>, parse_length!(padding_left, Padding).ok_or(padding_left)?);
        make_setter!(position, String, match position.as_str() {
            "Relative" => Position::Relative,
            "Absolute" => Position::Absolute,
            _ => return Err(position),
        });
        make_setter!(inset, Option<f32>, parse_length!(inset, Inset).ok_or(inset)?);
        make_setter!(top, Option<f32>, parse_length!(top, Inset).ok_or(top)?);
        make_setter!(right, Option<f32>, parse_length!(right, Inset).ok_or(right)?);
        make_setter!(bottom, Option<f32>, parse_length!(bottom, Inset).ok_or(bottom)?);
        make_setter!(left, Option<f32>, parse_length!(left, Inset).ok_or(left)?);
        make_setter!(margin, Option<f32>, parse_length!(margin, Margin).ok_or(margin)?);
        make_setter!(margin_top, Option<f32>, parse_length!(margin_top, Margin).ok_or(margin_top)?);
        make_setter!(margin_right, Option<f32>, parse_length!(margin_right, Margin).ok_or(margin_right)?);
        make_setter!(margin_bottom, Option<f32>, parse_length!(margin_bottom, Margin).ok_or(margin_bottom)?);
        make_setter!(margin_left, Option<f32>, parse_length!(margin_left, Margin).ok_or(margin_left)?);

        make_setter!(text, content, String, content);
        make_setter!(text, color, u32, Some(parse_colour(color).ok_or(color)?));
        make_setter!(text, weight, String, match weight.as_str() {
            "Normal" => Weight::Normal,
            "Bold" => Weight::Bold,
            "Light" => Weight::Light,
            _ => return Err(weight),
        });

        methods.add_method("remove", |_lua, child, ()| {
            let mut child = child.0.lock().unwrap();
            child.remove = true;
            Ok(())
        });

    }
}

async fn show_message(ui: Ui, _shell: Shell, _lua: Lua, val: String) -> Result<ChildView> {
    Ok(ui.borrow_mut().await.views.add(val))
}

pub async fn init_lua(ui: &Ui, shell: &Shell) -> Result<()> {

    ui.set_lua_async_fn("show_message", shell, show_message).await?;

    Ok(())
}

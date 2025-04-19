#[derive(Default)]
pub struct StatusBar {
    pub inner: Option<super::Widget>,
    pub dirty: bool,
}

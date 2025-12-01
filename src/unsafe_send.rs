#[derive(Clone, Copy)]
pub struct UnsafeSend<T>(T);

unsafe impl<T> Send for UnsafeSend<T> {}
unsafe impl<T> Sync for UnsafeSend<T> {}

impl<T> UnsafeSend<T> {
    pub unsafe fn new(inner: T) -> Self {
        Self(inner)
    }

    pub fn into_inner(self) -> T {
        self.0
    }
}

pub trait Provider<T: ?Sized> {
    fn provide(&self) -> T;
}

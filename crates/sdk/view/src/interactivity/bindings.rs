use super::{App, MouseButton, Window};

pub(crate) type EventListener<E> = Box<dyn Fn(&E, &mut Window, &mut App) + 'static>;

pub(super) struct ButtonBinding<E> {
    pub(super) button: Option<MouseButton>,
    pub(super) listener: EventListener<E>,
}

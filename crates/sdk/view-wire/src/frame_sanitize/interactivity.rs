use super::*;

pub(super) fn sanitize_interactivity(
    interactivity: &mut Interactivity,
    walk: &Walk,
) -> Result<(), &'static str> {
    interactivity.aria.sanitize();
    interactivity.role = crate::aria::view_role(interactivity.role);
    let aria = &mut interactivity.aria;
    if interactivity.role == Some(gpui::Role::Heading)
        && aria.level.is_some_and(|level| !(1..=6).contains(&level))
    {
        aria.level = None;
    }
    aria.active_descendant &= !interactivity.focusable && interactivity.focus_handle.is_none();
    for style in interactivity.styles() {
        walk.style(style)?;
    }
    let keys = &interactivity.consumes_keys;
    if keys.iter().any(|key| gpui::Keystroke::parse(key).is_err()) {
        return Err("a consumed key gpui cannot read");
    }
    if interactivity.consumes_click && interactivity.on_click.is_none() {
        return Err("consumes a click it does not take");
    }
    Ok(())
}

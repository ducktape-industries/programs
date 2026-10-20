pub(crate) fn bounded(value: f32) -> f32 {
    match value.is_nan() {
        true => 0.0,
        false => value.clamp(0.0, f32::MAX),
    }
}

pub(crate) fn finite(value: f32) -> f32 {
    match value.is_nan() {
        true => 0.0,
        false => value.clamp(f32::MIN, f32::MAX),
    }
}

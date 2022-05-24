#[macro_export]
macro_rules! require {
    ($to_check:expr, $ret:expr) => {
        if let Some(to_check) = $to_check {
            to_check
        } else {
            return $ret;
        }
    };
}

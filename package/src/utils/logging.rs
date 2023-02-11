// Copyright 2023 Pants project contributors.
// Licensed under the Apache License, Version 2.0 (see LICENSE).

use std::sync::atomic::AtomicU8;

use lazy_static::lazy_static;

#[macro_export]
macro_rules! log {
    ($color:expr, $msg:expr $(,)?) => {
        let mut stderr = ::termcolor::StandardStream::stderr(::termcolor::ColorChoice::Always);
        stderr
            .set_color(::termcolor::ColorSpec::new().set_fg(Some($color))).unwrap();
        writeln!(&mut stderr, $msg).unwrap();
        stderr.reset().unwrap();
    };
    ($color:expr, $msg:expr, $($arg:tt)*) => {
        let mut stderr = ::termcolor::StandardStream::stderr(::termcolor::ColorChoice::Always);
        stderr
            .set_color(::termcolor::ColorSpec::new().set_fg(Some($color))).unwrap();
        writeln!(&mut stderr, "{}", format!($msg, $($arg)*)).unwrap();
        stderr.reset().unwrap();
    };
}

lazy_static! {
    pub(crate) static ref BUILD_STEP: AtomicU8 = AtomicU8::new(1);
}

#[macro_export]
macro_rules! build_step {
    ($msg:expr $(,)?) => {
        $crate::log!(
            ::termcolor::Color::Cyan,
            "{:>2}.) {}...",
            $crate::utils::logging::BUILD_STEP.fetch_add(1, ::std::sync::atomic::Ordering::Relaxed),
            $msg
        );
    };
    ($msg:expr, $($arg:tt)*) => {
        $crate::log!(
            ::termcolor::Color::Cyan,
            "{:>2}.) {}...",
            $crate::utils::logging::BUILD_STEP.fetch_add(1, ::std::sync::atomic::Ordering::Relaxed),
            format!($msg, $($arg)*)
        );
    };
}

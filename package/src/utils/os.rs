// Copyright 2023 Pants project contributors.
// Licensed under the Apache License, Version 2.0 (see LICENSE).

#[cfg(windows)]
pub(crate) const PATHSEP: &str = ";";

#[cfg(windows)]
pub(crate) const EOL: &str = "\r\n";

#[cfg(unix)]
pub(crate) const PATHSEP: &str = ":";

#[cfg(unix)]
pub(crate) const EOL: &str = "\n";

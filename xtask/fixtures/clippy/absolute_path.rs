// SPDX-License-Identifier: GPL-3.0-or-later
// expect: clippy::absolute_paths

mod outer {
    pub(crate) mod inner {
        pub(crate) fn call() {}
    }
}

pub fn run() {
    crate::outer::inner::call();
}

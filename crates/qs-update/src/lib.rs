// SPDX-License-Identifier: GPL-3.0-or-later

//! The update check (P4-19, QA-11, PRIN-9): the app says that a later release exists and how
//! to get it with what installed this one. It never downloads or installs a release.
//!
//! - [`Found::check`] reads the feed, [`FEED_URL`], with one GET ([`Https`]) and compares its
//!   [`Version`] with the one running.
//! - [`Install::channel`] finds how the app was installed, from files and the environment
//!   alone, and [`Channel::command`] is that channel's own way to update.
//! - [`State`] keeps when the background check last had an answer, so it asks at most once in
//!   [`CHECK_INTERVAL`].
//!
//! Nothing here runs unasked: the app checks when `help.check_for_updates` is run, and in the
//! background only once `updates.check` is `daily` (`docs/privacy.md`).

mod channel;
mod feed;
mod state;
mod version;

#[cfg(test)]
mod tests;

pub use channel::{AppImage, Channel, Download, Install, Os, RpmTool};
pub use feed::{CheckError, FEED_URL, Feed, Fetch, Found, Https, RELEASES, USER_AGENT};
pub use state::{CHECK_INTERVAL, State, state_path};
pub use version::{NotAVersion, Version};

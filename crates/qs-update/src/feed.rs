// SPDX-License-Identifier: GPL-3.0-or-later

//! The release feed: one small file the release workflow writes, read with one request.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::Version;

/// The feed's address: `latest.json` of quakecstudio/packages, which the release workflow
/// writes from `packaging/latest.json` on a tag with no `-`.
pub const FEED_URL: &str = "https://packages.quakecstudio.com/latest.json";

/// What every release page's address starts with; a feed naming any other page is refused,
/// since the page is opened in the browser.
pub const RELEASES: &str = "https://github.com/quakecstudio/editor/releases/";

/// What a request says it comes from: the app's name alone, the same from every install and
/// every version.
pub const USER_AGENT: &str = "quakec-studio";

/// How long a check may take before it is given up, in seconds.
const TIMEOUT_SECONDS: u64 = 10;

/// The most of an answer that is read, in bytes; the feed is two short lines.
const MAX_FEED_BYTES: u64 = 4096;

/// The latest release, as the feed names it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Feed {
    version: Version,
    url: String,
}

impl Feed {
    /// The feed in `text`, the JSON object `{"version": "2026.12.1", "url": "<release page>"}`;
    /// members it does not know are left unread, so a later feed can say more.
    ///
    /// # Errors
    ///
    /// [`CheckError::Malformed`] when the text is not that object or its version is not one,
    /// [`CheckError::NotAReleasePage`] when its page is not one of the app's release pages.
    pub fn parse(text: &str) -> Result<Self, CheckError> {
        let feed: Self = serde_json::from_str(text).map_err(CheckError::Malformed)?;
        if !feed.url.starts_with(RELEASES) {
            return Err(CheckError::NotAReleasePage(feed.url));
        }
        Ok(feed)
    }

    /// The release's version.
    #[must_use]
    pub const fn version(&self) -> Version {
        self.version
    }

    /// The release's page, with its notes and its downloads; it starts with [`RELEASES`].
    #[must_use]
    pub fn page(&self) -> &str {
        &self.url
    }

    /// What this release is to an install of version `current`.
    #[must_use]
    pub fn compared_to(self, current: Version) -> Found {
        if self.version > current {
            Found::Newer(self)
        } else {
            Found::UpToDate
        }
    }
}

/// What a check found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    /// A release later than the one running.
    Newer(Feed),
    /// The one running is the latest, or later than it, as a build from source can be.
    UpToDate,
}

impl Found {
    /// Checks for a release later than `current`: reads the feed through `fetch`, once.
    ///
    /// # Errors
    ///
    /// [`CheckError`] when the feed could not be read or is not a feed.
    pub fn check(fetch: &dyn Fetch, current: Version) -> Result<Self, CheckError> {
        Ok(Feed::parse(&fetch.latest()?)?.compared_to(current))
    }
}

/// Why a check could not say whether there is a later release.
#[derive(Debug)]
pub enum CheckError {
    /// The request failed, or its answer could not be read.
    Request(ureq::Error),
    /// The feed's host answered with this status and not the feed: a redirect, which is not
    /// followed.
    Status(u16),
    /// The answer is not a feed.
    Malformed(serde_json::Error),
    /// The feed names a page that is not one of the app's release pages.
    NotAReleasePage(String),
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Request(error) => write!(f, "{FEED_URL}: {error}"),
            Self::Status(status) => write!(f, "{FEED_URL} answered with status {status}"),
            Self::Malformed(error) => write!(f, "{FEED_URL} is not a release feed: {error}"),
            Self::NotAReleasePage(url) => {
                write!(f, "{FEED_URL} names {url}, which is not a release page")
            }
        }
    }
}

impl std::error::Error for CheckError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Request(error) => Some(error),
            Self::Malformed(error) => Some(error),
            Self::Status(_) | Self::NotAReleasePage(_) => None,
        }
    }
}

/// Reads the feed's text.
pub trait Fetch: Send + Sync {
    /// The feed's text, read once.
    ///
    /// # Errors
    ///
    /// [`CheckError::Request`] when it could not be read, [`CheckError::Status`] when what
    /// answered is not the feed.
    fn latest(&self) -> Result<String, CheckError>;
}

/// The feed over HTTPS: one GET of [`FEED_URL`], with no query and no body, saying only
/// [`USER_AGENT`] of where it comes from. A redirect is not followed.
pub struct Https {
    agent: ureq::Agent,
    url: String,
}

impl Https {
    /// A client for [`FEED_URL`], through the proxy the environment names, if any.
    #[must_use]
    pub fn new() -> Self {
        let config = Self::config().build();
        Self {
            agent: ureq::Agent::new_with_config(config),
            url: FEED_URL.to_owned(),
        }
    }

    /// What every request is made with.
    fn config() -> ureq::config::ConfigBuilder<ureq::typestate::AgentScope> {
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(TIMEOUT_SECONDS)))
            // A redirect is an error: a check asks the feed's host and no other.
            .max_redirects(0)
            .user_agent(USER_AGENT)
    }

    /// A client for the feed a test serves at `url`, reached directly whatever proxy the
    /// environment names.
    #[cfg(test)]
    pub(crate) fn at(url: String) -> Self {
        let config = Self::config().proxy(None).build();
        Self {
            agent: ureq::Agent::new_with_config(config),
            url,
        }
    }
}

impl Default for Https {
    fn default() -> Self {
        Self::new()
    }
}

impl Fetch for Https {
    fn latest(&self) -> Result<String, CheckError> {
        let mut response = self
            .agent
            .get(&self.url)
            .call()
            .map_err(CheckError::Request)?;
        // An error status is the request's error already; what is left that is not the feed
        // is a redirect.
        if response.status() != ureq::http::StatusCode::OK {
            return Err(CheckError::Status(response.status().as_u16()));
        }
        response
            .body_mut()
            .with_config()
            .limit(MAX_FEED_BYTES)
            .read_to_string()
            .map_err(CheckError::Request)
    }
}

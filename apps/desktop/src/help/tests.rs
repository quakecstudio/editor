// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::collections::BTreeSet;

use eframe::egui;
use eframe::egui::accesskit::Role;
use egui_kittest::kittest::Queryable;

use super::{GUIDE, Topic, button, take_links};
use crate::strings::{Text, help_for};
use crate::theme::{Mode, Theme};

/// The guide's pages, as the site publishes them (quakecstudio/site `public/docs/`).
const PAGES: [&str; 9] = [
    "index.html",
    "getting-started.html",
    "opening-a-mod.html",
    "play-loop.html",
    "debugging.html",
    "agent.html",
    "mcp.html",
    "map-editor.html",
    "troubleshooting.html",
];

// No spec statement: P7-11: every help link names a page of the guide and a section of it
#[test]
fn every_topic_links_a_section_of_a_guide_page() {
    for topic in Topic::ALL {
        let (page, anchor) = topic.link().split_once('#').unwrap();
        assert!(PAGES.contains(&page), "{topic:?} links {page}");
        assert!(!anchor.is_empty(), "{topic:?} links no section");
        assert_eq!(topic.url(), format!("{GUIDE}{}", topic.link()));
        assert!(!topic.summary().get().is_empty());
    }
}

// No spec statement: P7-11: help-links.txt, which the site's check reads, lists every help link
// once, and nothing else
#[test]
fn the_link_list_the_site_checks_is_every_topics_link() {
    let listed: BTreeSet<&str> = include_str!("../../help-links.txt")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    let links: BTreeSet<&str> = Topic::ALL.iter().map(|topic| topic.link()).collect();
    assert_eq!(listed, links, "regenerate apps/desktop/help-links.txt");
}

// No spec statement: P7-11: a help button is named for what it helps with, and a click shows
// the explanation and the link to the guide
#[test]
fn a_help_button_explains_and_links_to_the_guide() {
    let theme = Theme::load(Mode::Dark).unwrap();
    let mut harness = theme.harness().build_ui(|ui| {
        button(ui, &theme, Topic::Debugger);
    });
    harness.run();
    let name = help_for(Topic::Debugger.title().get());
    assert!(
        harness
            .query_by_value(Topic::Debugger.summary().get())
            .is_none()
    );
    harness.get_by_label(&name).click();
    harness.run();
    harness.get_by_value(Topic::Debugger.summary().get());
    harness.get_by_role_and_label(Role::Link, Text::HelpReadMore.get());
}

// No spec statement: P7-11: the help link asks to open the guide's section; the app takes that
// request from egui's output and leaves any other address there
#[test]
fn the_guide_link_is_taken_to_the_browser_and_nothing_else_is() {
    let theme = Theme::load(Mode::Dark).unwrap();
    let mut harness = theme.harness().build_ui_state(
        |ui, taken: &mut Vec<String>| {
            button(ui, &theme, Topic::MapIssues);
            taken.extend(take_links(ui.ctx()));
        },
        Vec::new(),
    );
    harness.run();
    harness
        .get_by_label(&help_for(Topic::MapIssues.title().get()))
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Link, Text::HelpReadMore.get())
        .click();
    harness.run();
    assert_eq!(*harness.state(), [Topic::MapIssues.url()]);

    let mut other = theme.harness().build_ui_state(
        |ui, taken: &mut (Vec<String>, usize)| {
            ui.ctx()
                .open_url(egui::OpenUrl::same_tab("https://example.com/"));
            taken.0.extend(take_links(ui.ctx()));
            taken.1 = ui.ctx().output(|output| output.commands.len());
        },
        (Vec::new(), 0),
    );
    other.run();
    assert_eq!(*other.state(), (Vec::new(), 1));
}

// No spec statement: UI-24: the usability audit has a row for every panel and window that has
// a help button, by the title the help gives it
#[test]
fn the_usability_audit_covers_every_panel() {
    let audit = include_str!("../../../../docs/audits/usability.md");
    for topic in Topic::ALL {
        let row = format!("\n| {} |", topic.title().get());
        assert!(audit.contains(&row), "no row for {topic:?} in the audit");
    }
}

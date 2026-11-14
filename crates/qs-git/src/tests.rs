// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use proptest::prelude::*;

use crate::{
    Change, ConflictIndex, DiffOf, FileDiff, FileStatus, HunkIndex, Repo, Side, conflicts,
    parse_blame, resolve,
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A repository for a test: its files, and whether it has a first commit of them.
struct RepoSpec {
    files: &'static [(&'static str, &'static str)],
    committed: bool,
}

impl Default for RepoSpec {
    fn default() -> Self {
        Self {
            files: &[(
                "a.qc",
                "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n",
            )],
            committed: true,
        }
    }
}

/// Runs git in `dir`, as a person would at the command line; its stdout.
fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("LC_ALL", "C")
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// What `git status --porcelain` prints in `dir`, each line.
fn porcelain(dir: &Path) -> Vec<String> {
    git(dir, &["status", "--porcelain", "--untracked-files=all"])
        .lines()
        .map(str::to_owned)
        .collect()
}

/// A fresh repository on branch `main` as `spec` says.
fn repo(name: &str, spec: &RepoSpec) -> (PathBuf, Repo) {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("qs-git-{name}-{}-{n}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    for (key, value) in [
        ("user.name", "Tester"),
        ("user.email", "tester@example.invalid"),
        ("commit.gpgsign", "false"),
        ("core.autocrlf", "false"),
    ] {
        git(&dir, &["config", key, value]);
    }
    for (path, text) in spec.files {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    if spec.committed {
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", "first"]);
    }
    let repo = Repo::discover(&dir).unwrap().unwrap();
    (dir, repo)
}

/// Our status as `git status --porcelain` lines.
fn ours(repo: &Repo) -> Vec<String> {
    repo.status()
        .unwrap()
        .files
        .iter()
        .map(FileStatus::porcelain)
        .collect()
}

// No spec statement: P4-35, the acceptance criterion: staging a hunk, committing and switching
// branches leave git status as the git command line reports it, and ours says the same
#[test]
fn staging_committing_and_switching_agree_with_the_command_line() {
    let (dir, repo) = repo("accept", &RepoSpec::default());
    let file = dir.join("a.qc");
    // Two hunks: the first line changed and the last, far enough apart.
    std::fs::write(
        &file,
        "ONE\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nTEN\n",
    )
    .unwrap();
    std::fs::write(dir.join("new.qc"), "new\n").unwrap();
    assert_eq!(repo.diff(&file, DiffOf::Unstaged).unwrap().hunks.len(), 2);
    assert_eq!(porcelain(&dir), [" M a.qc", "?? new.qc"]);
    assert_eq!(ours(&repo), porcelain(&dir));

    repo.stage_hunk(&file, HunkIndex::new(0)).unwrap();
    assert_eq!(porcelain(&dir), ["MM a.qc", "?? new.qc"]);
    assert_eq!(ours(&repo), porcelain(&dir));
    let staged = git(&dir, &["diff", "--cached"]);
    assert!(
        staged.contains("+ONE") && !staged.contains("+TEN"),
        "{staged}"
    );
    assert_eq!(repo.diff(&file, DiffOf::Staged).unwrap().hunks.len(), 1);

    repo.stage(&dir.join("new.qc")).unwrap();
    assert_eq!(porcelain(&dir), ["MM a.qc", "A  new.qc"]);
    assert_eq!(ours(&repo), porcelain(&dir));

    let id = repo
        .commit("Change the first line\n\nAnd add a file.")
        .unwrap();
    assert_eq!(git(&dir, &["rev-parse", "HEAD"]).trim(), id);
    assert_eq!(
        git(&dir, &["log", "-1", "--format=%s"]).trim(),
        "Change the first line"
    );
    assert_eq!(porcelain(&dir), [" M a.qc"]);
    assert_eq!(ours(&repo), porcelain(&dir));

    repo.stage(&file).unwrap();
    repo.commit("Change the last line").unwrap();
    assert!(porcelain(&dir).is_empty());
    repo.create_branch("feature").unwrap();
    assert_eq!(git(&dir, &["branch", "--show-current"]).trim(), "feature");
    std::fs::write(dir.join("feature.qc"), "x\n").unwrap();
    repo.stage(&dir.join("feature.qc")).unwrap();
    repo.commit("On the feature").unwrap();
    repo.switch("main").unwrap();
    assert_eq!(git(&dir, &["branch", "--show-current"]).trim(), "main");
    assert!(!dir.join("feature.qc").exists(), "main has no such file");
    assert!(porcelain(&dir).is_empty());
    let status = repo.status().unwrap();
    assert_eq!(status.branch.as_deref(), Some("main"));
    assert!(status.files.is_empty());
    let branches = repo.branches().unwrap();
    let names: Vec<(&str, bool)> = branches
        .iter()
        .map(|b| (b.name.as_str(), b.current))
        .collect();
    assert_eq!(names, [("feature", false), ("main", true)]);
}

// No spec statement: P4-35: unstaging a hunk takes only that hunk out of the index, and
// unstaging a file takes all of it; a new repository unstages by leaving the index
#[test]
fn unstaging_a_hunk_or_a_file() {
    let (dir, repo) = repo("unstage", &RepoSpec::default());
    let file = dir.join("a.qc");
    std::fs::write(
        &file,
        "ONE\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nTEN\n",
    )
    .unwrap();
    repo.stage(&file).unwrap();
    assert_eq!(repo.diff(&file, DiffOf::Staged).unwrap().hunks.len(), 2);
    repo.unstage_hunk(&file, HunkIndex::new(1)).unwrap();
    assert_eq!(porcelain(&dir), ["MM a.qc"]);
    let staged = git(&dir, &["diff", "--cached"]);
    assert!(
        staged.contains("+ONE") && !staged.contains("+TEN"),
        "{staged}"
    );
    assert!(matches!(
        repo.unstage_hunk(&file, HunkIndex::new(5)),
        Err(crate::GitError::NoHunk(index)) if index == HunkIndex::new(5)
    ));
    repo.unstage(&file).unwrap();
    assert_eq!(porcelain(&dir), [" M a.qc"]);

    let (fresh, new_repo) = repo_uncommitted();
    new_repo.stage(&fresh.join("a.qc")).unwrap();
    assert_eq!(porcelain(&fresh), ["A  a.qc"]);
    new_repo.unstage(&fresh.join("a.qc")).unwrap();
    assert_eq!(porcelain(&fresh), ["?? a.qc"]);
    assert_eq!(ours(&new_repo), porcelain(&fresh));
}

fn repo_uncommitted() -> (PathBuf, Repo) {
    repo(
        "fresh",
        &RepoSpec {
            committed: false,
            ..RepoSpec::default()
        },
    )
}

// No spec statement: P4-35: a file's history follows its commits, newest first; blame names
// each line's commit and author; the text at HEAD is the committed one, none for a new file
#[test]
fn history_blame_and_the_text_at_head() {
    let (dir, repo) = repo("history", &RepoSpec::default());
    let file = dir.join("a.qc");
    std::fs::write(
        &file,
        "one\nTWO\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n",
    )
    .unwrap();
    repo.stage(&file).unwrap();
    repo.commit("Second").unwrap();
    let log = repo.log(&file, 10).unwrap();
    let summaries: Vec<&str> = log.iter().map(|c| c.summary.as_str()).collect();
    assert_eq!(summaries, ["Second", "first"]);
    assert_eq!(log[0].author, "Tester");
    assert_eq!(log[0].date.len(), 10);
    assert_eq!(repo.log(&file, 1).unwrap().len(), 1);
    let blame = repo.blame(&file).unwrap();
    assert_eq!(blame.len(), 10);
    assert_eq!((blame[0].line, blame[0].summary.as_str()), (1, "first"));
    assert_eq!((blame[1].line, blame[1].summary.as_str()), (2, "Second"));
    assert_eq!(blame[1].commit, log[0].id);
    assert_eq!(
        blame[2].author, "Tester",
        "a commit seen before keeps its author"
    );
    assert_eq!(
        repo.head_text(&file).unwrap().as_deref(),
        Some("one\nTWO\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n")
    );
    std::fs::write(dir.join("new.qc"), "x\n").unwrap();
    assert_eq!(repo.head_text(&dir.join("new.qc")).unwrap(), None);
    assert!(repo.blame(&dir.join("new.qc")).is_err());
    assert!(repo.switch("no-such-branch").is_err());
    assert!(repo.create_branch("main").is_err(), "the name is taken");
    assert!(repo.commit("nothing staged").is_err());
}

// No spec statement: P4-35: a merge that conflicts lists the file as unmerged; resolving each
// conflict to a side and staging the file ends the merge as git's own resolution would
#[test]
fn a_merge_conflict_is_resolved_a_side_at_a_time() {
    let (dir, repo) = repo("merge", &RepoSpec::default());
    let file = dir.join("a.qc");
    repo.create_branch("theirs").unwrap();
    std::fs::write(
        &file,
        "one\nTHEIRS\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n",
    )
    .unwrap();
    repo.stage(&file).unwrap();
    repo.commit("Theirs").unwrap();
    repo.switch("main").unwrap();
    std::fs::write(
        &file,
        "one\nOURS\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n",
    )
    .unwrap();
    repo.stage(&file).unwrap();
    repo.commit("Ours").unwrap();
    let merged = Command::new("git")
        .args(["merge", "-q", "theirs"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(!merged.status.success(), "the merge conflicts");
    let status = repo.status().unwrap();
    assert!(status.files[0].is_conflicted());
    assert_eq!(ours(&repo), porcelain(&dir));
    let text = std::fs::read_to_string(&file).unwrap();
    assert_eq!(conflicts(&text).len(), 1);
    let resolved = resolve(&text, ConflictIndex::new(0), Side::Theirs).unwrap();
    assert_eq!(
        resolved,
        "one\nTHEIRS\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n"
    );
    std::fs::write(&file, resolved).unwrap();
    repo.stage(&file).unwrap();
    assert_eq!(porcelain(&dir), ["M  a.qc"]);
    assert_eq!(ours(&repo), porcelain(&dir));
    repo.commit("Merge theirs").unwrap();
    assert!(porcelain(&dir).is_empty());
}

// No spec statement: P4-35: the conflict markers git writes, with and without the diff3 base,
// and each side kept; markers that do not close are text
#[test]
fn conflicts_are_found_and_resolved_to_a_side() {
    let text = "a\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> branch\nb\n\
                <<<<<<< HEAD\nx\n||||||| base\nbase\n=======\ny\n>>>>>>> other\n";
    let found = conflicts(text);
    assert_eq!(found.len(), 2);
    assert_eq!(
        (found[0].start, found[0].base, found[0].middle, found[0].end),
        (1, None, 3, 5)
    );
    assert_eq!(found[1].base, Some(9));
    assert_eq!(
        resolve(text, ConflictIndex::new(0), Side::Ours).unwrap(),
        "a\nours\nb\n<<<<<<< HEAD\nx\n||||||| base\nbase\n=======\ny\n>>>>>>> other\n"
    );
    assert_eq!(
        resolve(text, ConflictIndex::new(1), Side::Theirs).unwrap(),
        "a\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> branch\nb\ny\n"
    );
    assert!(
        resolve(text, ConflictIndex::new(1), Side::Both)
            .unwrap()
            .ends_with("b\nx\ny\n")
    );
    assert_eq!(resolve(text, ConflictIndex::new(2), Side::Ours), None);
    assert!(conflicts("<<<<<<< HEAD\nonly ours\n").is_empty());
    assert!(conflicts("<<<<<<<< not a marker\n=======\n>>>>>>>\n").is_empty());
    assert_eq!(
        conflicts("<<<<<<< a\r\nx\r\n=======\r\ny\r\n>>>>>>> b\r\n").len(),
        1,
        "CRLF lines"
    );
}

// No spec statement: P4-35: a diff's hunks, their ranges and a one-hunk patch; a range with
// no count is one line
#[test]
fn a_diff_reads_its_hunks_and_makes_one_hunk_patches() {
    let text = "diff --git a/a.qc b/a.qc\nindex 1..2 100644\n--- a/a.qc\n+++ b/a.qc\n\
                @@ -1 +1 @@\n-one\n+ONE\n@@ -8,3 +8,3 @@ void()\n eight\n nine\n-ten\n+TEN\n";
    let diff = FileDiff::parse(text).unwrap();
    assert_eq!(diff.hunks.len(), 2);
    assert_eq!((diff.hunks[0].old_start, diff.hunks[0].old_lines), (1, 1));
    assert_eq!((diff.hunks[1].new_start, diff.hunks[1].new_lines), (8, 3));
    let patch = diff.patch(HunkIndex::new(1)).unwrap();
    assert!(patch.starts_with("diff --git") && patch.contains("+TEN") && !patch.contains("+ONE"));
    assert_eq!(diff.patch(HunkIndex::new(2)), None);
    assert!(FileDiff::parse("").unwrap().hunks.is_empty());
    assert!(FileDiff::parse("@@ nonsense\n").is_err());
    assert_eq!(Change::Modified.code(), 'M');
}

// No spec statement: P4-35: blame's porcelain names a commit's author and summary once and
// each later line by the commit alone
#[test]
fn blame_porcelain_is_read() {
    let text = "abc 1 1 2\nauthor A\nsummary First\n\tline one\nabc 2 2\n\tline two\n";
    let lines = parse_blame(text).unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!((lines[1].line, lines[1].author.as_str()), (2, "A"));
    assert!(parse_blame("x\n").is_err());
}

proptest! {
    // No spec statement: P4-35: resolving a conflict to a side keeps every line outside it and
    // leaves no marker of it
    #[test]
    fn resolving_keeps_the_rest_and_drops_the_markers(
        before in proptest::collection::vec("[a-z]{0,5}", 0..4),
        ours in proptest::collection::vec("[a-z]{0,5}", 0..4),
        theirs in proptest::collection::vec("[a-z]{0,5}", 0..4),
        after in proptest::collection::vec("[a-z]{0,5}", 0..4),
    ) {
        let lines = |l: &[String]| l.iter().map(|x| format!("{x}\n")).collect::<String>();
        let text = format!(
            "{}<<<<<<< HEAD\n{}=======\n{}>>>>>>> b\n{}",
            lines(&before), lines(&ours), lines(&theirs), lines(&after)
        );
        let kept = resolve(&text, ConflictIndex::new(0), Side::Both).unwrap();
        prop_assert_eq!(
            kept,
            format!("{}{}{}{}", lines(&before), lines(&ours), lines(&theirs), lines(&after))
        );
        prop_assert!(conflicts(&resolve(&text, ConflictIndex::new(0), Side::Ours).unwrap()).is_empty());
    }
}

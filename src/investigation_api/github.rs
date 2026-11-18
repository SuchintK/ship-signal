use super::Sources;
use anyhow::{Context, Result, ensure};
use regex::Regex;
use reqwest::{Url, blocking::Response};
use serde::{Deserialize, de::DeserializeOwned};
use ship_signal::investigation::{
    CodeEvidence, CodeSelection, ProductConfig, PullRequestEvidence, RepositoryRoute,
    RepositorySnapshot, Ticket, valid_repository,
};
use std::{collections::BTreeSet, io::Read, sync::LazyLock};

const MAX_FILE_BYTES: u64 = 16_000;
const MAX_PR_FILES: usize = 3_000;

static PULL_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"https://github\.com/([A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)/pull/([0-9]+)\b")
        .expect("valid GitHub PR pattern")
});

pub(super) fn pull_request_links(
    attachments: impl Iterator<Item = String>,
    description: &str,
) -> Vec<String> {
    attachments
        .chain(std::iter::once(description.to_owned()))
        .flat_map(|source| {
            PULL_URL
                .captures_iter(&source)
                .filter(|capture| valid_repository(&capture[1]))
                .map(|capture| {
                    format!(
                        "https://github.com/{}/pull/{}",
                        capture[1].to_ascii_lowercase(),
                        &capture[2]
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[derive(Deserialize)]
struct Commit {
    sha: String,
    commit: CommitMetadata,
}

#[derive(Deserialize)]
struct CommitMetadata {
    tree: TreeId,
}

#[derive(Deserialize)]
struct TreeId {
    sha: String,
}

#[derive(Deserialize)]
struct Tree {
    truncated: bool,
    tree: Vec<TreeEntry>,
}

#[derive(Deserialize)]
struct TreeEntry {
    #[serde(rename = "type")]
    kind: String,
    mode: String,
    path: String,
}

#[derive(Deserialize)]
struct PullRequest {
    title: String,
    body: Option<String>,
    state: String,
    merged: bool,
    changed_files: usize,
}

#[derive(Deserialize)]
struct PullFile {
    filename: String,
}

impl Tree {
    fn file_paths(self, search_paths: &[String]) -> Result<BTreeSet<String>> {
        ensure!(!self.truncated, "GitHub truncated the repository tree");
        Ok(self
            .tree
            .into_iter()
            .filter(|entry| {
                entry.kind == "blob" && matches!(entry.mode.as_str(), "100644" | "100755")
            })
            .filter(|entry| {
                search_paths.is_empty()
                    || search_paths.iter().any(|prefix| {
                        let prefix = prefix.trim_end_matches('/');
                        entry.path == prefix
                            || entry
                                .path
                                .strip_prefix(prefix)
                                .is_some_and(|rest| rest.starts_with('/'))
                    })
            })
            .map(|entry| entry.path)
            .collect())
    }
}

impl Sources {
    fn github(
        &self,
        repository: &str,
        endpoint: &[&str],
        query: &[(&str, &str)],
        accept: &str,
    ) -> Result<Response> {
        ensure!(valid_repository(repository), "invalid GitHub repository");
        let token =
            std::env::var("GITHUB_TOKEN").context("GITHUB_TOKEN is required for investigation")?;
        let mut url = Url::parse("https://api.github.com/repos/")?;
        url.path_segments_mut()
            .expect("GitHub URL supports path segments")
            .pop_if_empty()
            .extend(repository.split('/'))
            .extend(endpoint.iter().copied());
        url.query_pairs_mut().extend_pairs(query.iter().copied());
        let response = self
            .http
            .get(url)
            .bearer_auth(token)
            .header("Accept", accept)
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .with_context(|| format!("read GitHub repository {repository}"))?
            .error_for_status()
            .with_context(|| format!("GitHub repository {repository} HTTP status"))?;
        ensure!(
            response.status().is_success(),
            "GitHub returned an unexpected HTTP redirect"
        );
        Ok(response)
    }

    fn github_json<T: DeserializeOwned>(
        &self,
        repository: &str,
        endpoint: &[&str],
        query: &[(&str, &str)],
    ) -> Result<T> {
        self.github(repository, endpoint, query, "application/vnd.github+json")?
            .json()
            .context("decode GitHub response")
    }

    fn github_paths(&self, route: &RepositoryRoute, commit: &Commit) -> Result<BTreeSet<String>> {
        ensure!(
            commit.sha.len() == 40 && commit.sha.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "GitHub returned an invalid commit SHA"
        );
        let tree: Tree = self.github_json(
            &route.repository,
            &["git", "trees", &commit.commit.tree.sha],
            &[("recursive", "1")],
        )?;
        tree.file_paths(&route.search_paths)
    }

    pub(super) fn github_repository(&self, route: &RepositoryRoute) -> Result<RepositorySnapshot> {
        let base: Commit =
            self.github_json(&route.repository, &["commits", &route.base_ref], &[])?;
        let base_paths = self.github_paths(route, &base)?;
        let (live_sha, live_paths) = match &route.live_ref {
            Some(name) if name == &route.base_ref => (Some(base.sha.clone()), base_paths.clone()),
            Some(name) => {
                let live: Commit = self.github_json(&route.repository, &["commits", name], &[])?;
                let paths = if live.sha == base.sha {
                    base_paths.clone()
                } else {
                    self.github_paths(route, &live)?
                };
                (Some(live.sha), paths)
            }
            None => (None, BTreeSet::new()),
        };
        Ok(RepositorySnapshot {
            repository: route.repository.clone(),
            base_sha: base.sha,
            live_sha,
            base_paths: base_paths.into_iter().collect(),
            live_paths: live_paths.into_iter().collect(),
        })
    }

    pub(super) fn github_code(
        &self,
        snapshot: &RepositorySnapshot,
        selection: &CodeSelection,
    ) -> Result<CodeEvidence> {
        let sha = if selection.live {
            snapshot
                .live_sha
                .as_deref()
                .context("repository has no configured live ref")?
        } else {
            &snapshot.base_sha
        };
        let mut endpoint = vec!["contents"];
        endpoint.extend(selection.path.split('/'));
        let response = self.github(
            &snapshot.repository,
            &endpoint,
            &[("ref", sha)],
            "application/vnd.github.raw+json",
        )?;
        let mut bytes = Vec::new();
        response.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= MAX_FILE_BYTES && !bytes.contains(&0),
            "selected code is too large or binary"
        );
        let text = String::from_utf8(bytes).context("selected code is not UTF-8")?;
        Ok(CodeEvidence {
            repository: snapshot.repository.clone(),
            path: selection.path.clone(),
            sha: sha.into(),
            live: selection.live,
            text,
        })
    }

    pub(super) fn github_pull_requests(
        &self,
        product: &ProductConfig,
        tickets: &[Ticket],
    ) -> Result<Vec<PullRequestEvidence>> {
        let mut evidence = Vec::new();
        let urls: BTreeSet<_> = tickets
            .iter()
            .flat_map(|ticket| &ticket.pull_request_urls)
            .collect();
        for url in urls {
            let captures = PULL_URL
                .captures(url)
                .context("invalid GitHub pull request link")?;
            let Some(route) = product
                .repositories
                .iter()
                .find(|route| route.repository.eq_ignore_ascii_case(&captures[1]))
            else {
                continue;
            };
            let number = &captures[2];
            let repository = &route.repository;
            if let Ok(pull) = self.github_pull_request(repository, number, url) {
                evidence.push(pull);
            }
        }
        Ok(evidence)
    }
    fn github_pull_request(
        &self,
        repository: &str,
        number: &str,
        url: &str,
    ) -> Result<PullRequestEvidence> {
        let pull: PullRequest = self.github_json(repository, &["pulls", number], &[])?;
        ensure!(
            pull.changed_files <= MAX_PR_FILES,
            "GitHub limits the listed files for this pull request"
        );
        let mut paths = Vec::new();
        for page in 1..=pull.changed_files.div_ceil(100) {
            let files: Vec<PullFile> = self.github_json(
                repository,
                &["pulls", number, "files"],
                &[("per_page", "100"), ("page", &page.to_string())],
            )?;
            paths.extend(files.into_iter().map(|file| file.filename));
        }
        ensure!(
            paths.len() == pull.changed_files,
            "GitHub returned an incomplete pull request file list"
        );
        Ok(PullRequestEvidence {
            url: url.to_owned(),
            repository: repository.to_owned(),
            title: pull.title,
            description: pull.body.unwrap_or_default(),
            state: pull.state,
            merged: pull.merged,
            paths,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_regular_files_at_the_requested_revision_enter_the_catalog() -> Result<()> {
        let tree: Tree = serde_json::from_str(
            r#"{
            "truncated": false,
            "tree": [
                { "path": "src/alerts.rs", "type": "blob", "mode": "100644" },
                { "path": "src/symlink", "type": "blob", "mode": "120000" },
                { "path": "src/execute", "type": "blob", "mode": "100755" },
                { "path": "src/doc", "type": "tree", "mode": "040000" },
                { "path": "src-old/alerts.rs", "type": "blob", "mode": "100644" }
            ]
        }"#,
        )?;
        assert_eq!(
            tree.file_paths(&["src/".into()])?,
            BTreeSet::from(["src/alerts.rs".into(), "src/execute".into()])
        );
        Ok(())
    }
}

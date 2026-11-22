mod github;
mod linear;

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use ship_signal::investigation::{
    CodeEvidence, CodeSelection, InvestigationSources, LinearReference, LinearWrite, ProductConfig,
    PullRequestEvidence, RepositoryRoute, RepositorySnapshot, Ticket,
};
use std::time::Duration;

pub struct Sources {
    http: Client,
}

impl Sources {
    pub fn new() -> Result<Self> {
        let http = Client::builder()
            .user_agent(concat!("ship-signal/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(25))
            .build()
            .context("initialize investigation HTTP client")?;
        Ok(Self { http })
    }
}

impl InvestigationSources for Sources {
    fn tickets(&mut self, product: &ProductConfig) -> Result<Vec<Ticket>> {
        self.linear_tickets(product)
    }

    fn ticket(&mut self, id: &str) -> Result<Ticket> {
        self.linear_ticket(id)
    }

    fn pull_requests(
        &mut self,
        product: &ProductConfig,
        tickets: &[Ticket],
    ) -> Result<Vec<PullRequestEvidence>> {
        self.github_pull_requests(product, tickets)
    }

    fn repository(&mut self, route: &RepositoryRoute) -> Result<RepositorySnapshot> {
        self.github_repository(route)
    }

    fn code(
        &mut self,
        snapshot: &RepositorySnapshot,
        selection: &CodeSelection,
    ) -> Result<CodeEvidence> {
        self.github_code(snapshot, selection)
    }

    fn validate_write(&mut self, write: &LinearWrite) -> Result<()> {
        self.linear_validate_write(write)
    }

    fn write(&mut self, write: &LinearWrite) -> Result<LinearReference> {
        self.linear_write(write)
    }
}

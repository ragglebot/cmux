//! The page visit logs of every browser profile: one SQLite file per profile
//! under `<session state dir>/history/`, or in memory for a session without
//! a state directory. A log is pruned to 90 days and 100,000 visits when it
//! opens and after a record that passes the cap.

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::PathBuf;

use cmux_history::{
    HistoryEntry, HistoryError, MAX_VISITS, NewVisit, VisitStore, VisitSummary, profile_file_name,
    profile_from_file_name,
};

/// The directory below the session state directory.
pub(super) const DIRECTORY: &str = "history";

pub(super) struct Pages {
    directory: Option<PathBuf>,
    open: BTreeMap<String, VisitStore>,
}

impl Pages {
    pub(super) fn new(directory: Option<PathBuf>) -> Self {
        Self { directory, open: BTreeMap::new() }
    }

    /// `profile`'s log, opened (and pruned) on first use.
    pub(super) fn store(
        &mut self,
        profile: &str,
        now_ms: i64,
    ) -> Result<&VisitStore, HistoryError> {
        if !self.open.contains_key(profile) {
            let store = match &self.directory {
                Some(directory) => VisitStore::open(&directory.join(profile_file_name(profile)))?,
                None => VisitStore::open_in_memory()?,
            };
            store.prune(now_ms)?;
            self.open.insert(profile.to_owned(), store);
        }
        self.open
            .get(profile)
            .ok_or_else(|| HistoryError::Io(std::io::Error::other("visit log vanished after open")))
    }

    /// Every profile with a log open or on disk, sorted.
    pub(super) fn profiles(&self) -> Result<Vec<String>, HistoryError> {
        let mut profiles: Vec<String> = self.open.keys().cloned().collect();
        if let Some(directory) = &self.directory {
            match std::fs::read_dir(directory) {
                Ok(entries) => {
                    for entry in entries {
                        let name = entry?.file_name();
                        if let Some(profile) = name.to_str().and_then(profile_from_file_name) {
                            profiles.push(profile);
                        }
                    }
                }
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        profiles.sort();
        profiles.dedup();
        Ok(profiles)
    }

    /// Records a visit; prunes when the log passes the cap. Returns the entry
    /// id `page:<profile>:<visit id>`.
    pub(super) fn record(
        &mut self,
        profile: &str,
        visit: &NewVisit,
        now_ms: i64,
    ) -> Result<String, HistoryError> {
        let store = self.store(profile, now_ms)?;
        let id = store.record(visit)?;
        if store.count()? > MAX_VISITS as u64 {
            store.prune(now_ms)?;
        }
        Ok(format!("page:{profile}:{id}"))
    }

    /// Sets the title of `url`'s newest visit in `profile`.
    pub(super) fn update_title(
        &mut self,
        profile: &str,
        url: &str,
        title: &str,
        now_ms: i64,
    ) -> Result<usize, HistoryError> {
        if !self.profiles()?.iter().any(|known| known == profile) {
            return Ok(0);
        }
        self.store(profile, now_ms)?.update_title(url, title)
    }

    /// Removes every visit of exactly `url` in `profile`.
    pub(super) fn remove_url(
        &mut self,
        profile: &str,
        url: &str,
        now_ms: i64,
    ) -> Result<usize, HistoryError> {
        if !self.profiles()?.iter().any(|known| known == profile) {
            return Ok(0);
        }
        self.store(profile, now_ms)?.remove_url(url)
    }

    pub(super) fn summaries(
        &mut self,
        profile: &str,
        limit: usize,
        now_ms: i64,
    ) -> Result<Vec<VisitSummary>, HistoryError> {
        self.store(profile, now_ms)?.summaries(limit)
    }

    /// Page entries of every profile: the folded text pre-filter, visits at or
    /// after `since_ms`, at most `limit` per profile.
    pub(super) fn entries(
        &mut self,
        text: &str,
        since_ms: Option<i64>,
        limit: usize,
        now_ms: i64,
    ) -> Result<Vec<HistoryEntry>, HistoryError> {
        let mut entries = Vec::new();
        for profile in self.profiles()? {
            entries.extend(self.store(&profile, now_ms)?.entries(&profile, text, since_ms, limit)?);
        }
        Ok(entries)
    }

    /// The entry of one visit, when it exists.
    pub(super) fn entry(
        &mut self,
        profile: &str,
        visit: i64,
        now_ms: i64,
    ) -> Result<Option<HistoryEntry>, HistoryError> {
        if !self.profiles()?.iter().any(|known| known == profile) {
            return Ok(None);
        }
        Ok(self.store(profile, now_ms)?.visit(visit)?.map(|visit| visit.entry(profile)))
    }

    pub(super) fn remove_visit(
        &mut self,
        profile: &str,
        visit: i64,
        now_ms: i64,
    ) -> Result<usize, HistoryError> {
        if !self.profiles()?.iter().any(|known| known == profile) {
            return Ok(0);
        }
        self.store(profile, now_ms)?.remove_visit(visit)
    }

    /// Removes `host` and its subdomains in `profile` or every profile.
    pub(super) fn remove_host(
        &mut self,
        host: &str,
        profile: Option<&str>,
        now_ms: i64,
    ) -> Result<usize, HistoryError> {
        let mut removed = 0;
        for profile in self.targets(profile)? {
            removed += self.store(&profile, now_ms)?.remove_host(host)?;
        }
        Ok(removed)
    }

    /// Removes visits at or after `since_ms` (`None`: all) in `profile` or
    /// every profile.
    pub(super) fn clear(
        &mut self,
        since_ms: Option<i64>,
        profile: Option<&str>,
        now_ms: i64,
    ) -> Result<usize, HistoryError> {
        let mut removed = 0;
        for profile in self.targets(profile)? {
            removed += self.store(&profile, now_ms)?.remove_since(since_ms)?;
        }
        Ok(removed)
    }

    fn targets(&self, profile: Option<&str>) -> Result<Vec<String>, HistoryError> {
        match profile {
            Some(profile) => {
                Ok(self.profiles()?.into_iter().filter(|known| known == profile).collect())
            }
            None => self.profiles(),
        }
    }
}

/// `page:<profile>:<visit id>`; the profile may contain colons.
pub(super) fn parse_page_id(id: &str) -> Option<(&str, i64)> {
    let (profile, visit) = id.strip_prefix("page:")?.rsplit_once(':')?;
    Some((profile, visit.parse().ok()?))
}

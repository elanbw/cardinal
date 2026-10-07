//! Run history, modeled on voidtools Everything: every time an item is opened
//! or revealed from Cardinal its run count goes up and its last-run time is
//! recorded. On top of that, macOS's own usage metadata (Spotlight's
//! `kMDItemUseCount` / `kMDItemLastUsedDate`) adds files opened outside
//! Cardinal. Search results with any history are promoted to the top of the
//! default (unsorted) result order by a frecency score.

use crate::DB_PATH;
use parking_lot::Mutex;
use search_cache::{SearchCache, SlabIndex};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Once,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tracing::{error, warn};

const FILE_NAME: &str = "run_history.json";
// Opens from Cardinal count double: they are explicit "this is what I searched for" signals.
const CARDINAL_RUN_WEIGHT: f64 = 2.0;
const SYSTEM_LOOKBACK_DAYS: u32 = 90;
const SYSTEM_MAX_ENTRIES: usize = 2000;
const SYSTEM_REFRESH: Duration = Duration::from_secs(10 * 60);
const MDLS_CHUNK: usize = 200;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RunEntry {
    pub count: u32,
    pub last_run: u64,
}

#[derive(Debug, Default)]
pub(crate) struct RunHistory {
    /// Opens from Cardinal, persisted to disk.
    entries: HashMap<String, RunEntry>,
    /// Opens recorded by macOS, refreshed from Spotlight in the background.
    system: HashMap<String, RunEntry>,
}

impl RunHistory {
    fn load(file: &Path) -> Self {
        let entries = match fs::read(file) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                warn!("Ignoring unreadable run history {file:?}: {e}");
                HashMap::new()
            }),
            Err(_) => HashMap::new(),
        };
        Self {
            entries,
            system: HashMap::new(),
        }
    }

    fn save(&self, file: &Path) {
        let tmp = file.with_extension("json.tmp");
        let result = serde_json::to_vec(&self.entries)
            .map_err(anyhow::Error::from)
            .and_then(|bytes| fs::write(&tmp, bytes).map_err(Into::into))
            .and_then(|()| fs::rename(&tmp, file).map_err(Into::into));
        if let Err(e) = result {
            error!("Failed to save run history to {file:?}: {e}");
        }
    }

    pub(crate) fn record(&mut self, path: &str, now: u64) {
        let entry = self.entries.entry(path.to_string()).or_default();
        entry.count = entry.count.saturating_add(1);
        entry.last_run = now;
    }

    /// Paths ordered by frecency: weighted use count scaled by how recently
    /// the item was last used, then most recent use.
    pub(crate) fn ranked_paths(&self, now: u64) -> Vec<&str> {
        let mut scored: HashMap<&str, (f64, u64)> = HashMap::new();
        for (path, entry) in &self.entries {
            let slot = scored.entry(path.as_str()).or_default();
            slot.0 += CARDINAL_RUN_WEIGHT * f64::from(entry.count);
            slot.1 = slot.1.max(entry.last_run);
        }
        for (path, entry) in &self.system {
            let slot = scored.entry(path.as_str()).or_default();
            slot.0 += f64::from(entry.count.max(1));
            slot.1 = slot.1.max(entry.last_run);
        }
        let mut ranked: Vec<(&str, f64, u64)> = scored
            .into_iter()
            .map(|(path, (uses, last))| (path, uses * recency_weight(now, last), last))
            .collect();
        ranked.sort_by(|(a_path, a_score, a_last), (b_path, b_score, b_last)| {
            b_score
                .total_cmp(a_score)
                .then(b_last.cmp(a_last))
                .then(a_path.cmp(b_path))
        });
        ranked.into_iter().map(|(path, _, _)| path).collect()
    }
}

fn recency_weight(now: u64, last: u64) -> f64 {
    const DAY: u64 = 24 * 60 * 60;
    match now.saturating_sub(last) {
        age if age < DAY => 4.0,
        age if age < 7 * DAY => 2.0,
        age if age < 30 * DAY => 1.0,
        _ => 0.5,
    }
}

static HISTORY: Mutex<Option<RunHistory>> = Mutex::new(None);

fn history_file() -> Option<PathBuf> {
    DB_PATH
        .get()
        .and_then(|db| db.parent().map(|dir| dir.join(FILE_NAME)))
}

fn with_history<T>(f: impl FnOnce(&mut RunHistory, Option<&Path>) -> T) -> T {
    let file = history_file();
    let mut guard = HISTORY.lock();
    let history =
        guard.get_or_insert_with(|| file.as_deref().map(RunHistory::load).unwrap_or_default());
    f(history, file.as_deref())
}

/// Record that `path` was opened or revealed from Cardinal.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

pub(crate) fn record_run(path: &str) {
    let now = now_secs();
    with_history(|history, file| {
        history.record(path, now);
        if let Some(file) = file {
            history.save(file);
        }
    });
}

/// Move previously run items to the front of `results`, most-run first,
/// keeping everything else in its original order.
pub(crate) fn promote_run_items(cache: &SearchCache, results: &mut Vec<SlabIndex>) {
    start_system_refresh();
    let now = now_secs();
    let rank: HashMap<SlabIndex, usize> = with_history(|history, _| {
        history
            .ranked_paths(now)
            .into_iter()
            .filter_map(|path| cache.node_index_for_path(Path::new(path)))
            .enumerate()
            .map(|(rank, index)| (index, rank))
            .collect()
    });
    promote_ranked(&rank, results);
}

/// Keep the macOS usage snapshot fresh on a background thread.
fn start_system_refresh() {
    static STARTED: Once = Once::new();
    STARTED.call_once(|| {
        let spawned = thread::Builder::new()
            .name("run-history-system".into())
            .spawn(|| {
                loop {
                    let recents = query_system_recents();
                    with_history(|history, _| history.system = recents);
                    thread::sleep(SYSTEM_REFRESH);
                }
            });
        if let Err(e) = spawned {
            error!("Failed to start system run history refresh: {e}");
        }
    });
}

/// Files macOS recorded as opened in the last `SYSTEM_LOOKBACK_DAYS` days.
fn query_system_recents() -> HashMap<String, RunEntry> {
    let query = format!("kMDItemLastUsedDate >= $time.today(-{SYSTEM_LOOKBACK_DAYS})");
    let Some(listing) = run_nul_separated("mdfind", &["-0", &query]) else {
        return HashMap::new();
    };
    let paths: Vec<String> = listing.into_iter().take(SYSTEM_MAX_ENTRIES).collect();

    let mut recents = HashMap::with_capacity(paths.len());
    for chunk in paths.chunks(MDLS_CHUNK) {
        let dates = mdls_values("kMDItemLastUsedDate", chunk);
        let counts = mdls_values("kMDItemUseCount", chunk);
        let (Some(dates), Some(counts)) = (dates, counts) else {
            continue;
        };
        if dates.len() != chunk.len() || counts.len() != chunk.len() {
            warn!("Skipping run history chunk with mismatched mdls output");
            continue;
        }
        for ((path, date), count) in chunk.iter().zip(dates).zip(counts) {
            let Some(last_run) = parse_mdls_date(&date) else {
                continue;
            };
            let count = count.trim().parse().unwrap_or(1);
            recents.insert(path.clone(), RunEntry { count, last_run });
        }
    }
    recents
}

fn mdls_values(attribute: &str, paths: &[String]) -> Option<Vec<String>> {
    let mut args = vec!["-raw", "-name", attribute];
    args.extend(paths.iter().map(String::as_str));
    run_nul_separated("mdls", &args)
}

fn run_nul_separated(program: &str, args: &[&str]) -> Option<Vec<String>> {
    let output = match Command::new(program).args(args).output() {
        Ok(output) => output,
        Err(e) => {
            warn!("Failed to run {program}: {e}");
            return None;
        }
    };
    let text = String::from_utf8_lossy(&output.stdout);
    Some(
        text.split('\0')
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect(),
    )
}

/// Parse mdls's `YYYY-MM-DD HH:MM:SS +0000` timestamps into Unix seconds.
fn parse_mdls_date(value: &str) -> Option<u64> {
    let value = value.trim();
    let num = |range: std::ops::Range<usize>| value.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hour, minute, second) = (num(11..13)?, num(14..16)?, num(17..19)?);
    let offset_sign = match value.get(20..21)? {
        "+" => 1,
        "-" => -1,
        _ => return None,
    };
    let offset = offset_sign * (num(21..23)? * 3600 + num(23..25)? * 60);

    // Days since 1970-01-01 (Howard Hinnant's days_from_civil).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;

    let secs = days * 86_400 + hour * 3600 + minute * 60 + second - offset;
    u64::try_from(secs).ok()
}

fn promote_ranked(rank: &HashMap<SlabIndex, usize>, results: &mut Vec<SlabIndex>) {
    if rank.is_empty() || results.is_empty() {
        return;
    }
    let mut front: Vec<(usize, SlabIndex)> = Vec::new();
    results.retain(|index| match rank.get(index) {
        Some(&r) => {
            front.push((r, *index));
            false
        }
        None => true,
    });
    if front.is_empty() {
        return;
    }
    front.sort_unstable_by_key(|(r, _)| *r);
    results.splice(0..0, front.into_iter().map(|(_, index)| index));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_by_count_then_recency() {
        let mut history = RunHistory::default();
        history.record("/a", 10);
        history.record("/b", 5);
        history.record("/b", 6);
        history.record("/c", 20);
        assert_eq!(history.ranked_paths(20), vec!["/b", "/c", "/a"]);
    }

    #[test]
    fn blends_cardinal_runs_with_recent_system_use() {
        const DAY: u64 = 24 * 60 * 60;
        let now = 100 * DAY;
        let mut history = RunHistory::default();
        // Opened from Cardinal twice, two months ago: 2 * 2 * 0.5 = 2.
        history.record("/old", now - 60 * DAY);
        history.record("/old", now - 60 * DAY);
        // Opened from Finder three times today: 3 * 4 = 12.
        history.system.insert(
            "/today".into(),
            RunEntry {
                count: 3,
                last_run: now - 60,
            },
        );
        // Opened from Finder once last week: 1 * 1 = 1.
        history.system.insert(
            "/week".into(),
            RunEntry {
                count: 1,
                last_run: now - 10 * DAY,
            },
        );
        assert_eq!(history.ranked_paths(now), vec!["/today", "/old", "/week"]);
    }

    #[test]
    fn parses_mdls_dates() {
        assert_eq!(parse_mdls_date("1970-01-01 00:00:00 +0000"), Some(0));
        assert_eq!(
            parse_mdls_date("2026-09-24 15:41:30 +0000"),
            Some(1_790_264_490)
        );
        assert_eq!(
            parse_mdls_date("2026-09-24 17:41:30 +0200"),
            Some(1_790_264_490)
        );
        assert_eq!(parse_mdls_date("(null)"), None);
    }

    #[test]
    fn promotes_ranked_items_and_keeps_the_rest_in_order() {
        let rank: HashMap<SlabIndex, usize> =
            [(SlabIndex::new(7), 0), (SlabIndex::new(3), 1)].into();
        let mut results: Vec<SlabIndex> = [1, 3, 5, 7, 9].map(SlabIndex::new).to_vec();
        promote_ranked(&rank, &mut results);
        let order: Vec<usize> = results.iter().map(|i| i.get()).collect();
        assert_eq!(order, vec![7, 3, 1, 5, 9]);
    }

    #[test]
    fn round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("cardinal-run-history-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join(FILE_NAME);
        let mut history = RunHistory::default();
        history.record("/x", 1);
        history.save(&file);
        let loaded = RunHistory::load(&file);
        assert_eq!(
            loaded.entries.get("/x"),
            Some(&RunEntry {
                count: 1,
                last_run: 1
            })
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}

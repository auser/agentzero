use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;

const LESSONS_FILE: &str = ".agentzero/domains/_lessons.json";
const MAX_LESSONS: usize = 500;
const DECAY_HALF_LIFE_DAYS: f64 = 90.0;
const PRUNE_THRESHOLD: f64 = 0.05;

/// A single lesson captured from domain usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainLesson {
    pub id: String,
    pub domain: String,
    pub lesson_type: String,
    pub content: String,
    pub created_at: String,
    pub use_count: u32,
}

impl std::fmt::Display for DomainLesson {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] {} (domain={}, uses={})",
            self.lesson_type, self.content, self.domain, self.use_count
        )
    }
}

/// Persistent store for domain lessons with time-decay relevance.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LessonStore {
    pub lessons: Vec<DomainLesson>,
    #[serde(default)]
    next_id: u64,
}

impl LessonStore {
    /// Load the lesson store from the workspace.
    pub async fn load(workspace_root: &str) -> anyhow::Result<Self> {
        let path = Path::new(workspace_root).join(LESSONS_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = fs::read_to_string(&path)
            .await
            .context("failed to read lessons store")?;
        serde_json::from_str(&data).context("failed to parse lessons store")
    }

    /// Save the lesson store to the workspace, pruning low-relevance lessons.
    pub async fn save(&mut self, workspace_root: &str) -> anyhow::Result<()> {
        self.prune();

        let path = Path::new(workspace_root).join(LESSONS_FILE);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .context("failed to create domains directory")?;
        }
        let data =
            serde_json::to_string_pretty(self).context("failed to serialize lessons store")?;
        fs::write(&path, data)
            .await
            .context("failed to write lessons store")
    }

    /// Add a new lesson and return its ID.
    pub fn add_lesson(&mut self, domain: &str, lesson_type: &str, content: &str) -> String {
        self.next_id += 1;
        let id = format!("lesson-{:04}", self.next_id);

        self.lessons.push(DomainLesson {
            id: id.clone(),
            domain: domain.to_string(),
            lesson_type: lesson_type.to_string(),
            content: content.to_string(),
            created_at: now_iso8601(),
            use_count: 0,
        });

        // If over capacity, remove lowest-scored.
        if self.lessons.len() > MAX_LESSONS {
            self.prune();
            // If still over, remove the oldest.
            while self.lessons.len() > MAX_LESSONS {
                self.lessons.remove(0);
            }
        }

        id
    }

    /// Recall lessons for a domain, sorted by decayed relevance.
    pub fn recall(
        &self,
        domain: &str,
        lesson_type: Option<&str>,
        max_results: usize,
    ) -> Vec<&DomainLesson> {
        let now = now_epoch_days();
        let mut scored: Vec<(&DomainLesson, f64)> = self
            .lessons
            .iter()
            .filter(|l| l.domain == domain)
            .filter(|l| lesson_type.map_or(true, |t| l.lesson_type == t))
            .map(|l| {
                let score = decayed_relevance(l, now);
                (l, score)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored
            .into_iter()
            .take(max_results)
            .map(|(l, _)| l)
            .collect()
    }

    /// Increment the use count for a lesson.
    pub fn increment_use_count(&mut self, id: &str) {
        if let Some(lesson) = self.lessons.iter_mut().find(|l| l.id == id) {
            lesson.use_count += 1;
        }
    }

    /// Prune lessons below the relevance threshold.
    fn prune(&mut self) {
        let now = now_epoch_days();
        self.lessons
            .retain(|l| decayed_relevance(l, now) >= PRUNE_THRESHOLD);
    }
}

/// Calculate decayed relevance for a lesson.
///
/// `score = e^(-age_days / half_life) * (1 + use_count * 0.1)`
fn decayed_relevance(lesson: &DomainLesson, now_days: f64) -> f64 {
    let created_days = parse_iso8601_to_epoch_days(&lesson.created_at).unwrap_or(now_days);
    let age_days = (now_days - created_days).max(0.0);
    let base_decay = (-age_days * std::f64::consts::LN_2 / DECAY_HALF_LIFE_DAYS).exp();
    let usage_boost = 1.0 + (lesson.use_count as f64 * 0.1);
    (base_decay * usage_boost).min(1.0)
}

fn now_epoch_days() -> f64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    secs as f64 / 86400.0
}

fn parse_iso8601_to_epoch_days(ts: &str) -> Option<f64> {
    // Parse "YYYY-MM-DDT..." format.
    if ts.len() < 10 {
        return None;
    }
    let year: i64 = ts[..4].parse().ok()?;
    let month: i64 = ts[5..7].parse().ok()?;
    let day: i64 = ts[8..10].parse().ok()?;

    // Approximate days since epoch using a simple formula.
    // This is accurate enough for decay calculation purposes.
    let days = (year - 1970) * 365 + (year - 1969) / 4 - (year - 1901) / 100 + (year - 1601) / 400;
    let month_days: [i64; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let m = (month - 1).clamp(0, 11) as usize;
    let leap_adjustment = if month > 2 && is_leap_year(year) {
        1
    } else {
        0
    };

    Some((days + month_days[m] + day - 1 + leap_adjustment) as f64)
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

fn now_iso8601() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let secs_per_day = 86400u64;
    let days = now / secs_per_day;
    let remaining = now % secs_per_day;
    let hours = remaining / 3600;
    let minutes = (remaining % 3600) / 60;
    let seconds = remaining % 60;
    let (year, month, day) = days_to_ymd(days);
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

fn days_to_ymd(days_since_epoch: u64) -> (u64, u64, u64) {
    let z = days_since_epoch + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-domain-learning-{}-{nanos}-{seq}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn lesson_store_roundtrip() {
        let dir = temp_dir();
        let ws = dir.to_string_lossy().to_string();

        let mut store = LessonStore::default();
        let id = store.add_lesson(
            "test-domain",
            "source_quality",
            "arXiv has the best coverage",
        );
        assert!(id.starts_with("lesson-"));

        store.save(&ws).await.expect("save should succeed");

        let loaded = LessonStore::load(&ws).await.expect("load should succeed");
        assert_eq!(loaded.lessons.len(), 1);
        assert_eq!(loaded.lessons[0].domain, "test-domain");
        assert_eq!(loaded.lessons[0].lesson_type, "source_quality");

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn recall_filters_by_domain() {
        let dir = temp_dir();
        let ws = dir.to_string_lossy().to_string();

        let mut store = LessonStore::default();
        store.add_lesson("domain-a", "source_quality", "Lesson A");
        store.add_lesson("domain-b", "source_quality", "Lesson B");
        store.save(&ws).await.expect("save should succeed");

        let lessons = store.recall("domain-a", None, 10);
        assert_eq!(lessons.len(), 1);
        assert_eq!(lessons[0].content, "Lesson A");

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn recall_filters_by_type() {
        let dir = temp_dir();
        let ws = dir.to_string_lossy().to_string();

        let mut store = LessonStore::default();
        store.add_lesson("d", "source_quality", "Source lesson");
        store.add_lesson("d", "query_strategy", "Query lesson");
        store.save(&ws).await.expect("save should succeed");

        let lessons = store.recall("d", Some("query_strategy"), 10);
        assert_eq!(lessons.len(), 1);
        assert_eq!(lessons[0].content, "Query lesson");

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn increment_use_count_works() {
        let mut store = LessonStore::default();
        let id = store.add_lesson("d", "t", "content");
        assert_eq!(store.lessons[0].use_count, 0);

        store.increment_use_count(&id);
        assert_eq!(store.lessons[0].use_count, 1);

        store.increment_use_count(&id);
        assert_eq!(store.lessons[0].use_count, 2);
    }

    #[test]
    fn decayed_relevance_recent_is_high() {
        let lesson = DomainLesson {
            id: "l1".to_string(),
            domain: "d".to_string(),
            lesson_type: "t".to_string(),
            content: "c".to_string(),
            created_at: now_iso8601(),
            use_count: 0,
        };
        let now = now_epoch_days();
        let score = decayed_relevance(&lesson, now);
        assert!(
            score > 0.9,
            "recent lesson should have high relevance: {score}"
        );
    }

    #[test]
    fn decayed_relevance_old_is_low() {
        let lesson = DomainLesson {
            id: "l1".to_string(),
            domain: "d".to_string(),
            lesson_type: "t".to_string(),
            content: "c".to_string(),
            created_at: "2020-01-01T00:00:00Z".to_string(),
            use_count: 0,
        };
        let now = now_epoch_days();
        let score = decayed_relevance(&lesson, now);
        assert!(score < 0.1, "old lesson should have low relevance: {score}");
    }

    #[test]
    fn usage_boosts_relevance() {
        let mut lesson = DomainLesson {
            id: "l1".to_string(),
            domain: "d".to_string(),
            lesson_type: "t".to_string(),
            content: "c".to_string(),
            created_at: now_iso8601(),
            use_count: 0,
        };
        let now = now_epoch_days();
        let score_unused = decayed_relevance(&lesson, now);

        lesson.use_count = 5;
        let score_used = decayed_relevance(&lesson, now);

        assert!(
            score_used > score_unused,
            "used lesson should score higher: {score_used} vs {score_unused}"
        );
    }

    #[test]
    fn parse_iso8601_to_epoch_days_works() {
        let days = parse_iso8601_to_epoch_days("2026-03-16T00:00:00Z").expect("should parse");
        // 2026-03-16 is roughly 20,528 days since epoch.
        assert!(days > 20_000.0);
        assert!(days < 21_000.0);
    }

    #[tokio::test]
    async fn load_empty_returns_default() {
        let dir = temp_dir();
        let ws = dir.to_string_lossy().to_string();

        let store = LessonStore::load(&ws).await.expect("load should succeed");
        assert!(store.lessons.is_empty());

        std::fs::remove_dir_all(dir).ok();
    }
}

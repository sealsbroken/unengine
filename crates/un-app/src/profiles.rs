//! The children who use the app: each child's name and progress through the lessons, kept in one
//! JSON file. A grown-up adds a profile per child; a lesson started for a child greets them by
//! name and continues where they stopped, and a finished lesson is recorded for that child only.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use un_core::strategy::{SessionConfig, SessionSummary};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    /// Lessons the child completed (not merely stopped), by id.
    #[serde(default)]
    pub completed: Vec<String>,
    /// The lesson to go on with: the one stopped for a rest, or the next after a completed one.
    #[serde(default)]
    pub current: Option<String>,
    /// What the lessons asked to remember for this child (`SessionSummary::remember`), passed back
    /// as `SessionConfig::extra`.
    #[serde(default)]
    pub extra: HashMap<String, String>,
}

/// All profiles, saved to `path` after every change.
#[derive(Debug)]
pub struct Profiles {
    path: PathBuf,
    list: Vec<Profile>,
}

impl Profiles {
    /// The profiles in `path`; none when the file is missing or unreadable.
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let list = std::fs::read(&path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        Self { path, list }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn list(&self) -> &[Profile] {
        &self.list
    }

    pub fn get(&self, id: &str) -> Option<&Profile> {
        self.list.iter().find(|p| p.id == id)
    }

    pub fn add(&mut self, name: &str) -> Profile {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let mut id = format!("{stamp:x}");
        while self.get(&id).is_some() {
            id.push('x');
        }
        let profile = Profile { id, name: name.trim().to_string(), completed: vec![], current: None, extra: HashMap::new() };
        self.list.push(profile.clone());
        self.save();
        profile
    }

    pub fn rename(&mut self, id: &str, name: &str) {
        if let Some(p) = self.list.iter_mut().find(|p| p.id == id) {
            p.name = name.trim().to_string();
            self.save();
        }
    }

    pub fn remove(&mut self, id: &str) {
        self.list.retain(|p| p.id != id);
        self.save();
    }

    /// `base` for this child: their name (the tutor greets them and skips asking for it) and
    /// what their lessons remembered.
    pub fn config(&self, id: &str, mut base: SessionConfig) -> SessionConfig {
        if let Some(p) = self.get(id) {
            base.extra.extend(p.extra.clone());
            if !p.name.is_empty() {
                base.child_name = Some(p.name.clone());
            }
        }
        base
    }

    /// A lesson of `lessons` (the app's list, in order) ended for this child: what it asked to
    /// remember is kept; a completed lesson is ticked and the child goes on with the next one not
    /// yet completed, a stopped one is gone on with next time. Recording the same summary again
    /// changes nothing.
    pub fn record(&mut self, id: &str, lesson: &str, summary: &SessionSummary, lessons: &[String]) {
        let Some(p) = self.list.iter_mut().find(|p| p.id == id) else { return };
        summary.apply_remembered(&mut p.extra);
        if summary.paused {
            p.current = Some(lesson.to_string());
        } else {
            if !p.completed.iter().any(|c| c == lesson) {
                p.completed.push(lesson.to_string());
            }
            let after = lessons.iter().position(|l| l == lesson).map_or(0, |i| i + 1);
            p.current =
                lessons[after.min(lessons.len())..].iter().chain(lessons.iter()).find(|l| !p.completed.contains(l)).cloned();
        }
        self.save();
    }

    /// The lesson this child goes on with: the recorded one while it is still offered, else the
    /// first one not completed, else the first.
    pub fn next_lesson(&self, id: &str, lessons: &[String]) -> Option<String> {
        let p = self.get(id);
        p.and_then(|p| p.current.clone())
            .filter(|c| lessons.contains(c))
            .or_else(|| lessons.iter().find(|l| p.is_none_or(|p| !p.completed.contains(l))).cloned())
            .or_else(|| lessons.first().cloned())
    }

    /// Written to a temporary file first, so a crash never leaves half a file.
    fn save(&self) {
        let Ok(json) = serde_json::to_vec_pretty(&self.list) else { return };
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = self.path.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            if let Err(error) = std::fs::rename(&tmp, &self.path) {
                log::warn!("profiles: {error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lessons() -> Vec<String> {
        ["one", "two", "three"].iter().map(|s| s.to_string()).collect()
    }

    fn file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("un-profiles-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("profiles.json")
    }

    #[test]
    fn two_children_go_through_the_lessons_each_at_their_own_pace() {
        let path = file("two");
        let mut profiles = Profiles::open(&path);
        let mia = profiles.add("Mia").id;
        let vasya = profiles.add(" Вася ").id;
        assert_eq!(profiles.next_lesson(&mia, &lessons()).as_deref(), Some("one"));

        // Mia completes lesson one; Вася stops lesson one halfway.
        let done = SessionSummary::default();
        let mut paused = SessionSummary { paused: true, ..Default::default() };
        paused.remember.insert("lesson1.step".into(), "4".into());
        profiles.record(&mia, "one", &done, &lessons());
        profiles.record(&vasya, "one", &paused, &lessons());
        profiles.record(&mia, "one", &done, &lessons());

        // Kept across a restart of the app.
        let profiles = Profiles::open(&path);
        assert_eq!(profiles.list().len(), 2);
        assert_eq!(profiles.get(&mia).unwrap().completed, vec!["one".to_string()]);
        assert_eq!(profiles.next_lesson(&mia, &lessons()).as_deref(), Some("two"));
        assert_eq!(profiles.next_lesson(&vasya, &lessons()).as_deref(), Some("one"));
        let config = profiles.config(&vasya, SessionConfig::default());
        assert_eq!(config.child_name.as_deref(), Some("Вася"));
        assert_eq!(config.extra.get("lesson1.step").map(String::as_str), Some("4"));
        assert!(profiles.config(&mia, SessionConfig::default()).extra.is_empty());
    }

    #[test]
    fn after_the_last_lesson_the_first_one_not_completed_follows() {
        let mut profiles = Profiles::open(file("order"));
        let id = profiles.add("Mia").id;
        let done = SessionSummary::default();
        profiles.record(&id, "two", &done, &lessons());
        profiles.record(&id, "three", &done, &lessons());
        assert_eq!(profiles.next_lesson(&id, &lessons()).as_deref(), Some("one"));
        profiles.record(&id, "one", &done, &lessons());
        // All done: the list starts over.
        assert_eq!(profiles.get(&id).unwrap().current, None);
        assert_eq!(profiles.next_lesson(&id, &lessons()).as_deref(), Some("one"));
    }

    #[test]
    fn renaming_and_removing_a_child() {
        let path = file("edit");
        let mut profiles = Profiles::open(&path);
        let id = profiles.add("Mia").id;
        let other = profiles.add("Tom").id;
        profiles.rename(&id, "Mila");
        profiles.remove(&other);
        let profiles = Profiles::open(&path);
        assert_eq!(profiles.list().iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["Mila"]);
    }
}

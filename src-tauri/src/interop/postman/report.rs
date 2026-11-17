//! Import and export report collection (plan 09 §4, §6). At most [`MAX_REPORT_ITEMS`]
//! entries are kept; the rest are counted. Messages name fields and variables, never
//! their values.

use super::types::{ReportItem, ReportLevel};

pub const MAX_REPORT_ITEMS: usize = 1000;

/// Collects report entries up to the cap and counts the overflow.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    items: Vec<ReportItem>,
    truncated: u32,
}

impl Report {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, level: ReportLevel, file: &str, path: &str, message: impl Into<String>) {
        if self.items.len() >= MAX_REPORT_ITEMS {
            self.truncated = self.truncated.saturating_add(1);
            return;
        }
        self.items.push(ReportItem {
            level,
            file: file.to_string(),
            path: path.to_string(),
            message: message.into(),
        });
    }

    pub fn info(&mut self, file: &str, path: &str, message: impl Into<String>) {
        self.push(ReportLevel::Info, file, path, message);
    }

    pub fn warning(&mut self, file: &str, path: &str, message: impl Into<String>) {
        self.push(ReportLevel::Warning, file, path, message);
    }

    pub fn skipped(&mut self, file: &str, path: &str, message: impl Into<String>) {
        self.push(ReportLevel::Skipped, file, path, message);
    }

    /// Appends every entry of `other` (still capped).
    pub fn extend(&mut self, other: Report) {
        let extra = other.truncated;
        for item in other.items {
            self.push(item.level, &item.file, &item.path, item.message);
        }
        self.truncated = self.truncated.saturating_add(extra);
    }

    pub fn items(&self) -> &[ReportItem] {
        &self.items
    }

    /// Entries that did not fit under the cap.
    pub fn truncated(&self) -> u32 {
        self.truncated
    }

    pub fn into_parts(self) -> (Vec<ReportItem>, u32) {
        (self.items, self.truncated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_entries_in_order_with_their_level() {
        let mut report = Report::new();

        report.info("a.json", "item[0]", "one");
        report.warning("a.json", "item[1]", "two");
        report.skipped("b.json", "", "three");

        let levels: Vec<ReportLevel> = report.items().iter().map(|item| item.level).collect();
        assert_eq!(
            levels,
            [
                ReportLevel::Info,
                ReportLevel::Warning,
                ReportLevel::Skipped
            ]
        );
        assert_eq!(report.items()[2].file, "b.json");
        assert_eq!(report.truncated(), 0);
    }

    #[test]
    fn caps_entries_and_counts_the_rest() {
        let mut report = Report::new();

        for index in 0..MAX_REPORT_ITEMS + 5 {
            report.info("a.json", &format!("item[{index}]"), "x");
        }

        let (items, truncated) = report.into_parts();
        assert_eq!(items.len(), MAX_REPORT_ITEMS);
        assert_eq!(truncated, 5);
    }

    #[test]
    fn extend_keeps_the_cap_and_adds_truncated_counts() {
        let mut first = Report::new();
        let mut second = Report::new();
        for _ in 0..MAX_REPORT_ITEMS {
            first.info("a", "", "x");
            second.info("b", "", "y");
        }
        second.info("b", "", "overflow");

        first.extend(second);

        assert_eq!(first.items().len(), MAX_REPORT_ITEMS);
        assert_eq!(first.truncated(), MAX_REPORT_ITEMS as u32 + 1);
    }
}

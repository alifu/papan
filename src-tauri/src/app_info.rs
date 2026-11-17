//! Basic facts about the running app, shown in the UI (e.g. the About box).

use serde::Serialize;
use specta::Type;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
}

impl AppInfo {
    pub fn new(name: impl Into<String>, version: impl ToString) -> Self {
        Self {
            name: name.into(),
            version: version.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serialises_name_and_version() {
        let info = AppInfo::new("Papan", "0.1.0");

        let value = serde_json::to_value(&info).unwrap();

        assert_eq!(value, json!({ "name": "Papan", "version": "0.1.0" }));
    }
}

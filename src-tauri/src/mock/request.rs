//! The request as the mock engine sees it: parsed once, read by rules and templates.

use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RequestCtx {
    pub method: String,
    /// Path as received, without the query.
    pub path: String,
    pub query: Vec<(String, String)>,
    /// Header names are lower-cased.
    pub headers: Vec<(String, String)>,
    pub cookies: Vec<(String, String)>,
    pub body_text: String,
    pub body_json: Option<Value>,
    pub form: Vec<(String, String)>,
    /// Route params, filled in once a route matched.
    pub params: Vec<(String, String)>,
    pub ip: String,
    pub host: String,
    /// How many requests this route has had, including this one.
    pub request_number: u64,
    /// The data buckets by name, filled in only for templates that read them.
    pub data: Value,
}

fn pairs_get<'a>(pairs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

impl RequestCtx {
    pub fn query_param(&self, name: &str) -> Option<&str> {
        pairs_get(&self.query, name)
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        pairs_get(&self.headers, &name.to_ascii_lowercase())
    }

    pub fn cookie(&self, name: &str) -> Option<&str> {
        pairs_get(&self.cookies, name)
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        pairs_get(&self.params, name)
    }

    /// A value from the body: a JSON path for JSON bodies, a field for form bodies.
    pub fn body_value(&self, path: &str) -> Option<Value> {
        match &self.body_json {
            Some(json) => json_path(json, path).cloned(),
            None => pairs_get(&self.form, path).map(|v| Value::String(v.to_string())),
        }
    }
}

/// `a.b[0].c`, optionally prefixed with `$` or `$.`; an empty path is the whole value.
pub fn json_path<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let path = path.trim().trim_start_matches('$').trim_start_matches('.');
    let mut current = root;
    for segment in path.split('.').filter(|s| !s.is_empty()) {
        let (name, indexes) = match segment.find('[') {
            Some(at) => (&segment[..at], &segment[at..]),
            None => (segment, ""),
        };
        if !name.is_empty() {
            current = current.get(name)?;
        }
        for index in indexes.split('[').filter(|s| !s.is_empty()) {
            let number = index.strip_suffix(']')?.parse::<usize>().ok()?;
            current = current.get(number)?;
        }
    }
    Some(current)
}

/// Parses `a=1&b=2` (percent-decoded).
pub fn parse_pairs(text: &str) -> Vec<(String, String)> {
    url::form_urlencoded::parse(text.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

/// Parses a `Cookie` header.
pub fn parse_cookies(header: &str) -> Vec<(String, String)> {
    header
        .split(';')
        .filter_map(|part| {
            let (name, value) = part.trim().split_once('=')?;
            (!name.is_empty()).then(|| (name.to_string(), value.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn walks_json_paths() {
        let v = json!({"a": {"b": [10, {"c": "x"}]}});
        assert_eq!(json_path(&v, "a.b[0]"), Some(&json!(10)));
        assert_eq!(json_path(&v, "$.a.b[1].c"), Some(&json!("x")));
        assert_eq!(json_path(&v, "a.z"), None);
        assert_eq!(json_path(&v, "a.b[9]"), None);
        assert_eq!(json_path(&v, ""), Some(&v));
    }

    #[test]
    fn parses_query_and_cookies() {
        assert_eq!(
            parse_pairs("a=1&b=x%20y"),
            vec![("a".into(), "1".into()), ("b".into(), "x y".into())]
        );
        assert_eq!(
            parse_cookies("a=1; b=2"),
            vec![("a".into(), "1".into()), ("b".into(), "2".into())]
        );
    }

    #[test]
    fn body_value_reads_json_or_form() {
        let json = RequestCtx {
            body_json: Some(json!({"n": 1})),
            ..Default::default()
        };
        assert_eq!(json.body_value("n"), Some(json!(1)));
        let form = RequestCtx {
            form: vec![("n".into(), "2".into())],
            ..Default::default()
        };
        assert_eq!(form.body_value("n"), Some(json!("2")));
    }
}

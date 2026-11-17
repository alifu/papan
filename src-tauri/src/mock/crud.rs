//! Data buckets and CRUD routes (plan 11 §11): a bucket is JSON held in memory while the mock
//! runs; a CRUD route serves a bucket holding an array as a REST resource.

use std::sync::{Mutex, MutexGuard, PoisonError};

use serde_json::{json, Map, Value};

use super::model::DataBucket;

/// Items one bucket may hold, and the serialized size it may reach.
pub const MAX_ITEMS: usize = 10_000;
pub const MAX_BUCKET_BYTES: usize = 2 * 1024 * 1024;
/// What all buckets of one mock may hold together.
pub const MAX_TOTAL_BYTES: usize = 4 * 1024 * 1024;

struct Bucket {
    id: String,
    name: String,
    value: Value,
    bytes: usize,
}

/// What a CRUD call answers.
#[derive(Debug, PartialEq)]
pub struct Outcome {
    pub status: u16,
    pub body: Value,
    /// Methods to offer when the answer is 405.
    pub allow: Option<&'static str>,
}

fn answer(status: u16, body: Value) -> Outcome {
    Outcome {
        status,
        body,
        allow: None,
    }
}

fn failure(status: u16, message: &str) -> Outcome {
    answer(status, json!({ "error": message }))
}

/// The state of every bucket of one running mock; starts from the file and resets with a restart.
pub struct Buckets {
    items: Mutex<Vec<Bucket>>,
}

fn id_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn position(items: &[Value], key: &str, id: &str) -> Option<usize> {
    items
        .iter()
        .position(|item| item.get(key).and_then(id_text).as_deref() == Some(id))
}

impl Buckets {
    pub fn new(data: &[DataBucket]) -> Self {
        let items: Vec<Bucket> = data
            .iter()
            .map(|bucket| Bucket {
                id: bucket.id.clone(),
                name: bucket.name.clone(),
                value: serde_json::from_str(&bucket.value).unwrap_or(Value::Null),
                bytes: 0,
            })
            .collect();
        let items = items
            .into_iter()
            .map(|mut bucket: Bucket| {
                bucket.bytes = serde_json::to_vec(&bucket.value).map_or(0, |bytes| bytes.len());
                bucket
            })
            .collect();
        Self {
            items: Mutex::new(items),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Vec<Bucket>> {
        self.items.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `{name: value}` for templates.
    pub fn snapshot(&self) -> Value {
        let map: Map<String, Value> = self
            .lock()
            .iter()
            .map(|b| (b.name.clone(), b.value.clone()))
            .collect();
        Value::Object(map)
    }

    /// Runs one REST call against the bucket `bucket_id`; `id` is the trailing path segment.
    pub fn call(
        &self,
        bucket_id: &str,
        id_key: &str,
        method: &str,
        id: Option<&str>,
        body: Option<&Value>,
    ) -> Outcome {
        let mut buckets = self.lock();
        let others: usize = buckets
            .iter()
            .filter(|b| b.id != bucket_id)
            .map(|b| b.bytes)
            .sum();
        let Some(bucket) = buckets.iter_mut().find(|b| b.id == bucket_id) else {
            return failure(500, "The data bucket of this route does not exist.");
        };
        let Value::Array(items) = &bucket.value else {
            return failure(500, "The data bucket does not hold an array.");
        };
        let method = method.to_ascii_uppercase();
        let room = Room {
            limit: MAX_BUCKET_BYTES.min(MAX_TOTAL_BYTES.saturating_sub(others)),
            used: bucket.bytes,
        };
        let outcome = match (id, method.as_str()) {
            (None, "GET") => answer(200, bucket.value.clone()),
            (Some(id), "GET") => match position(items, id_key, id) {
                Some(at) => answer(200, items[at].clone()),
                None => not_found(id),
            },
            (None, "POST") => create(&mut bucket.value, id_key, body, &room),
            (Some(id), "PUT" | "PATCH") => update(
                &mut bucket.value,
                id_key,
                id,
                (method == "PATCH", body),
                &room,
            ),
            (Some(id), "DELETE") => remove(&mut bucket.value, id_key, id, &room),
            (None, _) => refused("GET, POST"),
            (Some(_), _) => refused("GET, PUT, PATCH, DELETE"),
        };
        if matches!(method.as_str(), "POST" | "PUT" | "PATCH" | "DELETE") && outcome.status < 300 {
            bucket.bytes = serde_json::to_vec(&bucket.value).map_or(bucket.bytes, |v| v.len());
        }
        outcome
    }
}

fn not_found(id: &str) -> Outcome {
    failure(404, &format!("No item with id {id}."))
}

fn refused(allow: &'static str) -> Outcome {
    Outcome {
        allow: Some(allow),
        ..failure(405, "Method not allowed.")
    }
}

fn object_of(body: Option<&Value>) -> Result<&Map<String, Value>, Outcome> {
    match body {
        Some(Value::Object(map)) => Ok(map),
        _ => Err(failure(400, "The request body must be a JSON object.")),
    }
}

/// How much one bucket may grow to: a bucket already over `limit` (a big file) may still shrink.
struct Room {
    limit: usize,
    used: usize,
}

/// Stores `next` unless it is over the limits; the bucket keeps its old value otherwise.
fn commit(slot: &mut Value, next: Vec<Value>, done: Outcome, room: &Room) -> Outcome {
    if next.len() > MAX_ITEMS {
        return failure(413, "The data bucket is full.");
    }
    let candidate = Value::Array(next);
    let size = serde_json::to_vec(&candidate).map_or(usize::MAX, |bytes| bytes.len());
    if size > room.limit.max(room.used) {
        return failure(413, "The data bucket is full.");
    }
    *slot = candidate;
    done
}

fn items_of(slot: &Value) -> Vec<Value> {
    match slot {
        Value::Array(items) => items.clone(),
        _ => Vec::new(),
    }
}

fn create(slot: &mut Value, key: &str, body: Option<&Value>, room: &Room) -> Outcome {
    let mut item = match object_of(body) {
        Ok(map) => map.clone(),
        Err(outcome) => return outcome,
    };
    let mut items = items_of(slot);
    match item.get(key).and_then(id_text) {
        Some(id) if position(&items, key, &id).is_some() => {
            return failure(409, &format!("An item with id {id} exists."));
        }
        Some(_) => {}
        None => {
            item.insert(
                key.to_string(),
                Value::String(uuid::Uuid::new_v4().to_string()),
            );
        }
    }
    let created = Value::Object(item);
    items.push(created.clone());
    commit(slot, items, answer(201, created), room)
}

fn update(
    slot: &mut Value,
    key: &str,
    id: &str,
    (merge, body): (bool, Option<&Value>),
    room: &Room,
) -> Outcome {
    let changes = match object_of(body) {
        Ok(map) => map,
        Err(outcome) => return outcome,
    };
    let mut items = items_of(slot);
    let Some(at) = position(&items, key, id) else {
        return not_found(id);
    };
    let mut next = if merge {
        items[at].as_object().cloned().unwrap_or_default()
    } else {
        Map::new()
    };
    for (name, value) in changes {
        next.insert(name.clone(), value.clone());
    }
    // The id belongs to the URL: a body cannot move an item to another id.
    if let Some(original) = items[at].get(key) {
        next.insert(key.to_string(), original.clone());
    }
    let updated = Value::Object(next);
    items[at] = updated.clone();
    commit(slot, items, answer(200, updated), room)
}

fn remove(slot: &mut Value, key: &str, id: &str, room: &Room) -> Outcome {
    let mut items = items_of(slot);
    let Some(at) = position(&items, key, id) else {
        return not_found(id);
    };
    let removed = items.remove(at);
    commit(slot, items, answer(200, removed), room)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buckets(value: &str) -> Buckets {
        Buckets::new(&[DataBucket {
            id: "b".into(),
            name: "users".into(),
            value: value.into(),
        }])
    }

    fn call(b: &Buckets, method: &str, id: Option<&str>, body: Option<Value>) -> Outcome {
        b.call("b", "id", method, id, body.as_ref())
    }

    #[test]
    fn lists_and_reads_items_by_string_or_number_id() {
        let b = buckets(r#"[{"id": 1, "n": "a"}, {"id": "x", "n": "b"}]"#);
        assert_eq!(
            call(&b, "GET", None, None).body.as_array().unwrap().len(),
            2
        );
        assert_eq!(call(&b, "GET", Some("1"), None).body["n"], "a");
        assert_eq!(call(&b, "GET", Some("x"), None).body["n"], "b");
        assert_eq!(call(&b, "GET", Some("9"), None).status, 404);
    }

    #[test]
    fn post_assigns_an_id_and_refuses_a_duplicate() {
        let b = buckets("[]");
        let made = call(&b, "POST", None, Some(json!({"n": "a"})));
        assert_eq!(made.status, 201);
        assert!(made.body["id"].as_str().is_some_and(|id| id.len() == 36));
        let again = call(
            &b,
            "POST",
            None,
            Some(json!({"id": made.body["id"], "n": "b"})),
        );
        assert_eq!(again.status, 409);
        assert_eq!(call(&b, "POST", None, Some(json!([1]))).status, 400);
        assert_eq!(call(&b, "POST", None, None).status, 400);
        assert_eq!(
            call(&b, "GET", None, None).body.as_array().unwrap().len(),
            1
        );
    }

    #[test]
    fn put_replaces_and_patch_merges_but_neither_changes_the_id() {
        let b = buckets(r#"[{"id": 5, "a": 1, "b": 2}]"#);
        let put = call(&b, "PUT", Some("5"), Some(json!({"id": 99, "c": 3})));
        assert_eq!(put.body, json!({"id": 5, "c": 3}));
        let patch = call(&b, "PATCH", Some("5"), Some(json!({"d": 4})));
        assert_eq!(patch.body, json!({"id": 5, "c": 3, "d": 4}));
        assert_eq!(call(&b, "PUT", Some("7"), Some(json!({}))).status, 404);
    }

    #[test]
    fn delete_removes_and_returns_the_item() {
        let b = buckets(r#"[{"id": 1}, {"id": 2}]"#);
        assert_eq!(call(&b, "DELETE", Some("1"), None).body, json!({"id": 1}));
        assert_eq!(call(&b, "DELETE", Some("1"), None).status, 404);
        assert_eq!(b.snapshot()["users"], json!([{"id": 2}]));
    }

    #[test]
    fn other_methods_are_405_with_the_allowed_ones() {
        let b = buckets("[]");
        let list = call(&b, "DELETE", None, None);
        assert_eq!((list.status, list.allow), (405, Some("GET, POST")));
        let one = call(&b, "POST", Some("1"), None);
        assert_eq!(one.status, 405);
    }

    #[test]
    fn a_bucket_stops_growing_at_its_limits() {
        let b = buckets("[]");
        let big = json!({"pad": "x".repeat(900 * 1024)});
        let mut statuses = Vec::new();
        for _ in 0..13 {
            statuses.push(call(&b, "POST", None, Some(big.clone())).status);
        }
        assert_eq!(statuses.iter().filter(|s| **s == 201).count(), 2);
        assert_eq!(*statuses.last().unwrap(), 413);
        let full = buckets(&serde_json::to_string(&vec![json!({"id": 1}); MAX_ITEMS]).unwrap());
        assert_eq!(
            call(&full, "POST", None, Some(json!({"id": "new"}))).status,
            413
        );
    }

    #[test]
    fn a_missing_or_non_array_bucket_is_a_500_not_a_panic() {
        let b = buckets(r#"{"not": "an array"}"#);
        assert_eq!(call(&b, "GET", None, None).status, 500);
        assert_eq!(b.call("nope", "id", "GET", None, None).status, 500);
    }

    #[test]
    fn buckets_share_a_total_limit_but_may_still_shrink() {
        let pad = "x".repeat(1024 * 1024 + 512 * 1024);
        let one = |id: &str| DataBucket {
            id: id.into(),
            name: id.into(),
            value: format!(r#"[{{"id": 0, "pad": "{pad}"}}]"#),
        };
        let b = Buckets::new(&[one("a"), one("b"), one("c")]);
        let room = |at: &str| {
            b.call(
                at,
                "id",
                "POST",
                None,
                Some(&json!({"pad": "x".repeat(3000)})),
            )
        };
        assert_eq!(room("a").status, 413);
        let removed = b.call("a", "id", "DELETE", Some("0"), None);
        assert_eq!(removed.status, 200);
        assert_eq!(room("a").status, 201);
    }
}

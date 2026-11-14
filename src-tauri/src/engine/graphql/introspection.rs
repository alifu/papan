//! The introspection request and its result (plan 06 §6).

use std::sync::LazyLock;

use serde_json::Value;

use crate::error::{AppError, ErrorCode};
use crate::store::types::{Body, RequestContent, RequestKind, Scripts, SoapSettings};

/// A larger schema is refused.
pub const MAX_SCHEMA_BYTES: usize = 20 * 1024 * 1024;
const OPERATION_NAME: &str = "IntrospectionQuery";
const MAX_LISTED_ERRORS: usize = 3;
/// How many `ofType` levels the type references unwrap (`[[Int!]!]!` needs 6).
const TYPE_REF_DEPTH: usize = 9;

const QUERY_HEAD: &str = r"query IntrospectionQuery {
  __schema {
    description
    queryType { name }
    mutationType { name }
    subscriptionType { name }
    types { ...FullType }
    directives {
      name
      description
      isRepeatable
      locations
      args { ...InputValue }
    }
  }
}

fragment FullType on __Type {
  kind
  name
  description
  specifiedByURL
  fields(includeDeprecated: true) {
    name
    description
    args { ...InputValue }
    type { ...TypeRef }
    isDeprecated
    deprecationReason
  }
  inputFields { ...InputValue }
  interfaces { ...TypeRef }
  enumValues(includeDeprecated: true) {
    name
    description
    isDeprecated
    deprecationReason
  }
  possibleTypes { ...TypeRef }
}

fragment InputValue on __InputValue {
  name
  description
  type { ...TypeRef }
  defaultValue
}

fragment TypeRef on __Type {
";

/// The standard full introspection query.
pub static INTROSPECTION_QUERY: LazyLock<String> = LazyLock::new(|| {
    let opening: String = (0..TYPE_REF_DEPTH)
        .map(|level| format!("{}kind\n{0}name\n{0}ofType {{\n", "  ".repeat(level + 1)))
        .collect();
    let innermost = format!("{}kind\n{0}name\n", "  ".repeat(TYPE_REF_DEPTH + 1));
    let closing: String = (0..TYPE_REF_DEPTH)
        .rev()
        .map(|level| format!("{}}}\n", "  ".repeat(level + 1)))
        .collect();
    format!("{QUERY_HEAD}{opening}{innermost}{closing}}}\n")
});

/// The request's URL, headers, auth and settings with the introspection query as its
/// GraphQL body. Scripts are dropped: introspection never runs them (§13 D3).
pub fn introspection_content(content: &RequestContent) -> RequestContent {
    RequestContent {
        method: "POST".into(),
        body: Body::Graphql {
            query: INTROSPECTION_QUERY.clone(),
            variables: String::new(),
            operation_name: OPERATION_NAME.into(),
        },
        scripts: Scripts::default(),
        kind: RequestKind::Graphql,
        soap: SoapSettings::default(),
        ..content.clone()
    }
}

fn not_a_schema() -> AppError {
    AppError::invalid_input("The response is not a GraphQL introspection result.")
}

/// The introspection JSON when `bytes` holds `data.__schema` (or a bare `__schema`, as
/// some tools save it); an `errors`-only response lists up to three messages. The
/// caller masks secrets in the message.
pub fn validate_result(bytes: &[u8]) -> Result<String, AppError> {
    if bytes.len() > MAX_SCHEMA_BYTES {
        return Err(too_large());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| not_a_schema())?;
    let value: Value = serde_json::from_str(text).map_err(|_| not_a_schema())?;
    let schema = value
        .get("data")
        .unwrap_or(&value)
        .get("__schema")
        .filter(|schema| schema.is_object());
    if schema.is_some() {
        return Ok(text.to_string());
    }
    match value.get("errors").and_then(Value::as_array) {
        Some(errors) if !errors.is_empty() => Err(server_errors(errors)),
        _ => Err(not_a_schema()),
    }
}

pub fn too_large() -> AppError {
    AppError::invalid_input("The schema is larger than 20 MiB.")
}

fn server_errors(errors: &[Value]) -> AppError {
    let messages: Vec<&str> = errors
        .iter()
        .filter_map(|error| error.get("message").and_then(Value::as_str))
        .take(MAX_LISTED_ERRORS)
        .collect();
    let more = errors.len().saturating_sub(MAX_LISTED_ERRORS);
    let suffix = if more > 0 {
        format!(" (and {more} more)")
    } else {
        String::new()
    };
    AppError::new(
        ErrorCode::InvalidInput,
        format!(
            "The server refused the introspection query: {}{suffix}",
            messages.join("; ")
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/graphql");
        std::fs::read(dir.join(name)).unwrap()
    }

    #[test]
    fn the_query_nests_type_refs_nine_levels_deep_and_balances_braces() {
        let query = INTROSPECTION_QUERY.as_str();

        assert_eq!(query.matches("ofType {").count(), TYPE_REF_DEPTH);
        assert_eq!(query.matches('{').count(), query.matches('}').count());
        for field in [
            "specifiedByURL",
            "isRepeatable",
            "deprecationReason",
            "description",
        ] {
            assert!(query.contains(field), "{field}");
        }
    }

    #[test]
    fn a_result_with_data_schema_is_returned_as_is() {
        let bytes = fixture("introspection.json");

        assert_eq!(
            validate_result(&bytes).unwrap().as_bytes(),
            bytes.as_slice()
        );
        assert!(validate_result(br#"{"__schema": {"types": []}}"#).is_ok());
    }

    #[test]
    fn an_errors_only_result_lists_up_to_three_messages() {
        let one = validate_result(&fixture("errors-only.json")).unwrap_err();
        let many = validate_result(
            br#"{"errors": [{"message": "a"}, {"message": "b"}, {"message": "c"}, {"message": "d"}]}"#,
        )
        .unwrap_err();

        assert!(
            one.message.contains("Cannot query field"),
            "{}",
            one.message
        );
        assert!(
            many.message.ends_with("a; b; c (and 1 more)"),
            "{}",
            many.message
        );
    }

    #[test]
    fn neither_data_nor_errors_or_too_big_is_refused() {
        for bytes in [&b"{}"[..], b"not json", br#"{"data": {"__schema": 1}}"#] {
            let error = validate_result(bytes).unwrap_err();
            assert!(error.message.contains("not a GraphQL"), "{}", error.message);
        }
        let big = vec![b' '; MAX_SCHEMA_BYTES + 1];
        assert!(validate_result(&big)
            .unwrap_err()
            .message
            .contains("20 MiB"));
    }

    #[test]
    fn introspection_keeps_url_headers_and_auth_and_drops_scripts() {
        let content = RequestContent {
            scripts: Scripts {
                pre_request: "x()".into(),
                post_response: String::new(),
            },
            auth: crate::store::types::Auth::Bearer { token: "t".into() },
            ..RequestContent::new("POST", "https://x/graphql")
        };

        let built = introspection_content(&content);

        assert_eq!(built.url, content.url);
        assert_eq!(built.auth, content.auth);
        assert_eq!(built.scripts, Scripts::default());
        assert_eq!(built.kind, RequestKind::Graphql);
        assert!(
            matches!(&built.body, Body::Graphql { operation_name, .. } if operation_name == OPERATION_NAME)
        );
    }
}

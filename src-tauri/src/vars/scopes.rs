//! Variable layers in precedence order (plan 04 §5): global, collection, environment,
//! then the runtime layer a script chain adds for one send (plan 05 §4.2).
//! Disabled variables are never put in a layer, so a lower layer shows through.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::types::VarSource;

/// A variable's value as the resolver sees it. `text` is `None` for a secret that has
/// no value on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Value {
    pub text: Option<String>,
    #[serde(default)]
    pub secret: bool,
}

impl Value {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            secret: false,
        }
    }

    pub fn secret(text: Option<String>) -> Self {
        Self { text, secret: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Layer {
    pub source: VarSource,
    pub vars: BTreeMap<String, Value>,
}

/// Layers ordered lowest → highest.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scopes {
    pub layers: Vec<Layer>,
}

impl Scopes {
    pub fn new(layers: Vec<Layer>) -> Self {
        Self { layers }
    }

    /// The highest layer's definition of `name`, if any.
    pub fn lookup(&self, name: &str) -> Option<&Value> {
        self.lookup_with_source(name).map(|(value, _)| value)
    }

    pub fn lookup_with_source(&self, name: &str) -> Option<(&Value, VarSource)> {
        self.layers
            .iter()
            .rev()
            .find_map(|layer| layer.vars.get(name).map(|value| (value, layer.source)))
    }

    /// Every defined name, each once.
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .layers
            .iter()
            .flat_map(|layer| layer.vars.keys().map(String::as_str))
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(source: VarSource, vars: &[(&str, &str)]) -> Layer {
        Layer {
            source,
            vars: vars
                .iter()
                .map(|(name, text)| (name.to_string(), Value::plain(*text)))
                .collect(),
        }
    }

    #[test]
    fn highest_layer_wins_and_names_are_deduplicated() {
        let scopes = Scopes::new(vec![
            layer(VarSource::Global, &[("a", "g"), ("b", "g")]),
            layer(VarSource::Environment, &[("a", "e")]),
        ]);

        assert_eq!(
            scopes.lookup_with_source("a"),
            Some((&Value::plain("e"), VarSource::Environment))
        );
        assert_eq!(scopes.lookup("b"), Some(&Value::plain("g")));
        assert_eq!(scopes.lookup("c"), None);
        assert_eq!(scopes.names(), ["a", "b"]);
        assert_eq!(Value::secret(None).text, None);
    }
}

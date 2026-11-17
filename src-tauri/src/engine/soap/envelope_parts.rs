//! Text helpers and lookups of the envelope generator (split from `envelope.rs`).

use crate::engine::soap::model::{Operation, Particle, Port, WsdlModel};
use crate::error::AppError;

/// Escapes text and attribute values (`&<>"'`).
pub fn escape(text: &str) -> String {
    text.chars()
        .fold(String::with_capacity(text.len()), |mut out, c| {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' => out.push_str("&quot;"),
                '\'' => out.push_str("&apos;"),
                other => out.push(other),
            }
            out
        })
}

/// Comment text that cannot end the comment early.
pub(super) fn comment(text: &str) -> String {
    format!("<!-- {} -->", text.replace("--", "- -"))
}

pub(super) fn label(particle: &Particle) -> String {
    match particle {
        Particle::Element { decl } => decl.name.clone(),
        Particle::ElementRef { name, .. } | Particle::GroupRef { name, .. } => name.name.clone(),
        Particle::Group { compositor, .. } => format!("{compositor:?}").to_lowercase(),
        Particle::Any { .. } => "any".to_string(),
    }
}

pub(super) fn find<'m>(
    model: &'m WsdlModel,
    service: &str,
    port: &str,
    operation: &str,
) -> Result<(&'m Port, &'m Operation), AppError> {
    let found_service = model
        .services
        .iter()
        .find(|candidate| candidate.name == service)
        .ok_or_else(|| AppError::not_found(format!("Service \"{service}\" was not found.")))?;
    let found_port = found_service
        .ports
        .iter()
        .find(|candidate| candidate.name == port)
        .ok_or_else(|| {
            AppError::not_found(format!(
                "Port \"{port}\" was not found in service \"{service}\"."
            ))
        })?;
    let found_operation = found_port
        .operations
        .iter()
        .find(|candidate| candidate.name == operation)
        .ok_or_else(|| {
            AppError::not_found(format!(
                "Operation \"{operation}\" was not found in port \"{port}\"."
            ))
        })?;
    Ok((found_port, found_operation))
}

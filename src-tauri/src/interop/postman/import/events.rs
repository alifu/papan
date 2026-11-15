//! Postman `event[]` to our scripts: verbatim, with unsupported APIs reported
//! (plan 09 §5). Whether they run is decided elsewhere: imports switch them off (D7).

use crate::interop::postman::report::Report;
use crate::interop::postman::schema;
use crate::interop::postman::scripts::{join_exec, scan};
use crate::store::types::Scripts;

const PRE_REQUEST: &str = "prerequest";
const POST_RESPONSE: &str = "test";

/// The scripts of `events` and how many are non-empty; findings go to `report`.
pub fn plan_scripts(
    source: &str,
    path: &str,
    events: &[schema::Event],
    report: &mut Report,
) -> (Scripts, u32) {
    let dropped = events.iter().filter(|event| event.disabled).count();
    if dropped > 0 {
        report.warning(
            source,
            path,
            format!("{dropped} disabled script event(s) were not imported."),
        );
    }
    let of = |listen: &str| {
        let lines: Vec<Vec<String>> = events
            .iter()
            .filter(|event| event.listen == listen && !event.disabled)
            .map(|event| event.script.exec.clone())
            .collect();
        join_exec(&lines)
    };
    let scripts = Scripts {
        enabled: true,
        pre_request: of(PRE_REQUEST),
        post_response: of(POST_RESPONSE),
    };
    let mut count = 0;
    for (kind, text) in [
        ("pre-request", &scripts.pre_request),
        ("post-response", &scripts.post_response),
    ] {
        if text.is_empty() {
            continue;
        }
        count += 1;
        for finding in scan(text) {
            if finding.supported {
                report.info(
                    source,
                    path,
                    format!(
                        "The {kind} script uses `{}` (line {}): it can send data to other servers.",
                        finding.api, finding.line
                    ),
                );
            } else {
                report.warning(
                    source,
                    path,
                    format!(
                        "The {kind} script uses `{}` (line {}), which Papan does not support.",
                        finding.api, finding.line
                    ),
                );
            }
        }
    }
    (scripts, count)
}

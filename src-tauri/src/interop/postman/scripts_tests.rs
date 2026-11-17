use super::*;

fn apis(script: &str) -> Vec<(u32, String)> {
    scan(script)
        .into_iter()
        .filter(|finding| !finding.supported)
        .map(|finding| (finding.line, finding.api))
        .collect()
}

#[test]
fn supported_uses_are_not_reported() {
    let script = "\
pm.environment.set('a', 1);
pm.collectionVariables.get('b');
pm.globals.unset('c'); pm.variables.toObject();
pm.request.headers.upsert({ key: 'k', value: 'v' });
pm.request.url = 'x'; pm.request.body.raw;
pm.test('ok', () => { pm.response.to.have.status(200); });
pm.response.to.be.ok; pm.response.to.not.have.header('x');
pm.expect(pm.response.json().a).to.equal(1);
const t = pm.response.responseTime; console.log(btoa('x'), atob('y'));
";
    assert_eq!(apis(script), Vec::<(u32, String)>::new());
}

#[test]
fn unsupported_uses_are_listed_with_their_line() {
    let script = "\
pm.cookies.get('a');
pm.iterationData.get('x');
pm.info.requestName; pm.execution.setNextRequest('x');
pm.visualizer.set('t');
pm.require('lib'); const m = require('moment');
pm.vault.get('k'); pm.variables.replaceIn('{{a}}');
pm.response.to.have.jsonBody('a'); pm.response.to.have.jsonSchema({});
postman.setEnvironmentVariable('a', 'b');
tests['ok'] = responseCode.code === 200;
_.get(a, 'b'); CryptoJS.MD5('x'); moment(); tv4.validate(); xml2Json(x); cheerio.load(x);
setTimeout(() => {}, 1); pm.whatever; pm.request.auth; pm.environment.clear();
";
    let found = apis(script);
    let expected = [
        (1, "pm.cookies"),
        (2, "pm.iterationData"),
        (3, "pm.execution"),
        (3, "pm.info"),
        (4, "pm.visualizer"),
        (5, "pm.require"),
        (5, "require()"),
        (6, "pm.variables.replaceIn"),
        (6, "pm.vault"),
        (7, "pm.response.to.have.jsonBody"),
        (7, "pm.response.to.have.jsonSchema"),
        (8, "postman.setEnvironmentVariable"),
        (9, "responseCode"),
        (9, "tests[...]"),
        (10, "CryptoJS"),
        (10, "_ (lodash)"),
        (10, "cheerio"),
        (10, "moment"),
        (10, "tv4"),
        (10, "xml2Json"),
        (11, "pm.environment.clear"),
        (11, "pm.request.auth"),
        (11, "pm.whatever"),
        (11, "setTimeout"),
    ];
    let expected: Vec<(u32, String)> = expected
        .iter()
        .map(|(line, api)| (*line, (*api).to_string()))
        .collect();
    assert_eq!(found, expected);
}

#[test]
fn comments_strings_and_templates_are_ignored_and_lines_kept() {
    let script = "// pm.cookies\n/* pm.vault\n pm.info */ 'pm.cookies \\' pm.vault'\n\"tests[1]\" `pm.visualizer\n` pm.iterationData";

    assert_eq!(apis(script), vec![(5, "pm.iterationData".to_string())]);
}

#[test]
fn properties_named_like_apis_are_not_reported() {
    assert!(apis("obj.pm.cookies; data.tests['x']; a.require('b'); x._.y;").is_empty());
}

#[test]
fn send_request_is_reported_as_supported_information() {
    let found = scan("pm.sendRequest('https://x', () => {});");

    assert_eq!(
        found,
        vec![ScriptFinding {
            line: 1,
            api: "pm.sendRequest".into(),
            supported: true
        }]
    );
}

#[test]
fn chains_split_over_lines_keep_the_first_line() {
    assert_eq!(
        apis("\n\npm\n  .cookies\n  .get('a')"),
        vec![(3, "pm.cookies".into())]
    );
}

#[test]
fn join_exec_joins_lines_and_scripts() {
    let scripts = vec![
        vec!["a();".to_string(), "b();".to_string()],
        vec![" ".to_string()],
        vec!["c();".to_string()],
    ];
    assert_eq!(join_exec(&scripts), "a();\nb();\n\nc();");
}

/// Fails when `prelude.js` gains or loses a `pm` member (plan 09 §5).
#[test]
fn pm_members_match_the_prelude() {
    let prelude = include_str!("../../script/prelude.js");
    let start = prelude.find("const pm = Object.freeze({").unwrap();
    let body = &prelude[start..];
    let end = body.find("});").unwrap();
    let mut members: Vec<&str> = body[..end]
        .lines()
        .skip(1)
        .filter_map(|line| {
            let line = line.trim().trim_end_matches(',');
            let name = line.split(':').next()?.trim();
            (!name.is_empty()).then_some(name)
        })
        .collect();
    members.sort_unstable();

    assert_eq!(members, PM_MEMBERS);
}

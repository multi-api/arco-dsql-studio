//! Running a tinker snippet through an Arco Cloud environment's console Lambda, with the user's own AWS profile.
//!
//! Arco Cloud gives each environment a function `<app>-<environment>-console` with an alias `live`; its payload is
//! `{"command":"<artisan command>"}`, the same as Arco Cloud's Run command (spec section 6.5). This is how the role
//! wizard runs the owner-side grants, which only the app's role can run.

use aws_config::SdkConfig;
use aws_sdk_lambda::primitives::Blob;
use base64::Engine;

use crate::error::{AwsErrorKind, CoreError};
use crate::identity::aws_error;

/// The likely console function for a schema: `myapp_staging` → `myapp-staging-console`. The UI lets the user correct it.
pub fn console_function_name(schema: &str) -> String {
    format!("{}-console", schema.replace('_', "-"))
}

/// The Lambda payload for `tinker --execute="<php>"`. The command line may go through a shell, so `\`, `"`, `$` and
/// `` ` `` are backslash-escaped inside the double quotes; Symfony's own parser undoes the same escapes.
pub fn tinker_payload(php: &str) -> String {
    let mut escaped = String::with_capacity(php.len());
    for c in php.chars() {
        if matches!(c, '\\' | '"' | '$' | '`') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    serde_json::json!({ "command": format!("tinker --execute=\"{escaped}\"") }).to_string()
}

/// Invokes `function:live` with `php` and returns the command's output. A failed invocation or a non-zero exit is an
/// error carrying the output.
pub async fn run_tinker(cfg: &SdkConfig, function: &str, php: &str) -> Result<String, CoreError> {
    let resp = aws_sdk_lambda::Client::new(cfg)
        .invoke()
        .function_name(function)
        .qualifier("live")
        .payload(Blob::new(tinker_payload(php)))
        .send()
        .await
        .map_err(aws_error)?;
    let body = resp
        .payload()
        .map(|p| String::from_utf8_lossy(p.as_ref()).into_owned())
        .unwrap_or_default();
    let (output, status) = console_output(&body);
    match (resp.function_error(), status) {
        (None, None | Some(0)) => Ok(output),
        (fe, status) => Err(CoreError::Aws {
            kind: AwsErrorKind::Other,
            message: format!(
                "{function} failed ({}): {output}",
                fe.map(str::to_string)
                    .unwrap_or_else(|| format!("exit status {}", status.unwrap_or_default()))
            ),
        }),
    }
}

/// The readable output of a console response, and its exit status when it has one. Accepts
/// `{"output": "<base64 or text>", "statusCode": n}`, a JSON string, or anything else as-is.
pub fn console_output(body: &str) -> (String, Option<i64>) {
    match serde_json::from_str::<serde_json::Value>(body) {
        Ok(serde_json::Value::Object(map)) => {
            let status = map.get("statusCode").and_then(serde_json::Value::as_i64);
            let output = match map.get("output").and_then(serde_json::Value::as_str) {
                Some(out) => base64::engine::general_purpose::STANDARD
                    .decode(out)
                    .ok()
                    .and_then(|bytes| String::from_utf8(bytes).ok())
                    .unwrap_or_else(|| out.to_string()),
                None => body.to_string(),
            };
            (output, status)
        }
        Ok(serde_json::Value::String(s)) => (s, None),
        _ => (body.to_string(), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_function_name_from_schema() {
        assert_eq!(
            console_function_name("myapp_staging"),
            "myapp-staging-console"
        );
        assert_eq!(console_function_name("shop"), "shop-console");
    }

    #[test]
    fn tinker_payload_escaping() {
        let php = r#"DB::statement('GRANT SELECT ON "app"."it\'s" TO "r"'); echo $x . `y`;"#;
        let payload = tinker_payload(php);
        let v: serde_json::Value = serde_json::from_str(&payload).expect("valid JSON");
        let command = v["command"].as_str().unwrap();
        assert_eq!(
            command,
            r#"tinker --execute="DB::statement('GRANT SELECT ON \"app\".\"it\\'s\" TO \"r\"'); echo \$x . \`y\`;""#
        );
        // Undoing the shell escapes gives the PHP back.
        let inner = &command["tinker --execute=\"".len()..command.len() - 1];
        let mut unescaped = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            unescaped.push(if c == '\\' { chars.next().unwrap() } else { c });
        }
        assert_eq!(unescaped, php);
    }

    #[test]
    fn console_output_shapes() {
        let b64 = base64::engine::general_purpose::STANDARD.encode("done\n");
        assert_eq!(
            console_output(&format!(r#"{{"output":"{b64}","statusCode":0}}"#)),
            ("done\n".to_string(), Some(0))
        );
        assert_eq!(
            console_output(r#"{"output":"plain text!","statusCode":1}"#),
            ("plain text!".to_string(), Some(1))
        );
        assert_eq!(
            console_output(r#""just a string""#),
            ("just a string".to_string(), None)
        );
        assert_eq!(console_output("not json"), ("not json".to_string(), None));
    }
}

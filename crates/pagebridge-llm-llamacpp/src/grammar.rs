//! Minimal JSON-schema to GBNF grammar lowering.
//!
//! llama.cpp accepts GBNF grammars (a small BNF dialect) to constrain
//! generation. Most pagebridge prompts ask for a tightly-shaped JSON object,
//! so we only need to support the schema fragments we actually use:
//! `object` with `properties`, `array` with `items`, `string`, `integer`,
//! `number`, `boolean`, plus `enum` lists and `oneOf` unions.
//!
//! Everything else falls back to an unconstrained JSON value rule.

use serde_json::Value;

/// Build a GBNF grammar string from a JSON schema. The root non-terminal is
/// always `root`.
#[must_use]
pub fn schema_to_gbnf(schema: &Value) -> String {
    let mut state = State::default();
    let root = state.lower(schema);
    let mut out = String::new();
    out.push_str("root ::= ");
    out.push_str(&root);
    out.push('\n');
    for (name, body) in state.rules {
        out.push_str(&name);
        out.push_str(" ::= ");
        out.push_str(&body);
        out.push('\n');
    }
    out.push_str(PRIMITIVES);
    out
}

#[derive(Default)]
struct State {
    rules: Vec<(String, String)>,
    counter: usize,
}

impl State {
    fn fresh(&mut self, prefix: &str) -> String {
        let name = format!("{prefix}_{}", self.counter);
        self.counter += 1;
        name
    }

    fn lower(&mut self, schema: &Value) -> String {
        let Some(obj) = schema.as_object() else {
            return "json_value".into();
        };
        if let Some(en) = obj.get("enum").and_then(Value::as_array) {
            return self.lower_enum(en);
        }
        if let Some(one_of) = obj.get("oneOf").and_then(Value::as_array) {
            return self.lower_oneof(one_of);
        }
        match obj.get("type").and_then(Value::as_str) {
            Some("string") => "string".into(),
            Some("integer") => "integer".into(),
            Some("number") => "number".into(),
            Some("boolean") => "boolean".into(),
            Some("array") => {
                let items = obj.get("items").cloned().unwrap_or(Value::Null);
                self.lower_array(&items)
            }
            Some("object") => self.lower_object(obj),
            _ => "json_value".into(),
        }
    }

    fn lower_enum(&mut self, values: &[Value]) -> String {
        let alts: Vec<String> = values
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| format!("\"\\\"{}\\\"\"", escape_str(s)))
            .collect();
        if alts.is_empty() {
            return "json_value".into();
        }
        let name = self.fresh("enum");
        self.rules.push((name.clone(), alts.join(" | ")));
        name
    }

    fn lower_oneof(&mut self, alts: &[Value]) -> String {
        let lowered: Vec<String> = alts.iter().map(|a| self.lower(a)).collect();
        if lowered.is_empty() {
            return "json_value".into();
        }
        let name = self.fresh("oneof");
        self.rules.push((name.clone(), lowered.join(" | ")));
        name
    }

    fn lower_array(&mut self, items: &Value) -> String {
        let item_rule = self.lower(items);
        let name = self.fresh("array");
        let body = format!("\"[\" ws ({item_rule} (\",\" ws {item_rule})*)? ws \"]\"");
        self.rules.push((name.clone(), body));
        name
    }

    fn lower_object(&mut self, obj: &serde_json::Map<String, Value>) -> String {
        let Some(props) = obj.get("properties").and_then(Value::as_object) else {
            return "json_value".into();
        };
        let required: std::collections::HashSet<&str> = obj
            .get("required")
            .and_then(Value::as_array)
            .map(|arr| arr.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let mut parts: Vec<String> = Vec::new();
        for (k, v) in props {
            let rule = self.lower(v);
            let pair = format!(
                "\"\\\"{k}\\\"\" ws \":\" ws {rule}",
                k = escape_str(k),
                rule = rule
            );
            if required.contains(k.as_str()) {
                parts.push(pair);
            } else {
                let opt = format!("({pair})?");
                parts.push(opt);
            }
        }
        let name = self.fresh("object");
        let body = format!(
            "\"{{\" ws {} ws \"}}\"",
            parts.join(" ws \",\" ws ")
        );
        self.rules.push((name.clone(), body));
        name
    }
}

fn escape_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

const PRIMITIVES: &str = r#"
ws ::= ([ \t\n\r]*)
string ::= "\"" ( [^"\\] | "\\" . )* "\""
integer ::= "-"? [0-9]+
number ::= "-"? [0-9]+ ("." [0-9]+)? ([eE] [+-]? [0-9]+)?
boolean ::= "true" | "false"
json_value ::= string | number | "null" | boolean | "[" ws ("]" | json_value (ws "," ws json_value)* ws "]") | "{" ws ("}" | string ws ":" ws json_value (ws "," ws string ws ":" ws json_value)* ws "}")
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn primitives_compose() {
        let g = schema_to_gbnf(&json!({"type": "string"}));
        assert!(g.starts_with("root ::= string"));
        assert!(g.contains("string ::="));
    }

    #[test]
    fn object_with_required_field() {
        let schema = json!({
            "type": "object",
            "required": ["title"],
            "properties": {
                "title": {"type": "string"},
                "leaves": {"type": "array", "items": {"type": "integer"}}
            }
        });
        let g = schema_to_gbnf(&schema);
        assert!(g.contains("\"\\\"title\\\"\""));
        assert!(g.contains("\"\\\"leaves\\\"\""));
        // Optional fields wrap in parentheses then ?
        assert!(g.contains(")?"));
    }

    #[test]
    fn enum_values_lower_to_alternation() {
        let schema = json!({"enum": ["descend", "halt"]});
        let g = schema_to_gbnf(&schema);
        assert!(g.contains("\"\\\"descend\\\"\""));
        assert!(g.contains("\"\\\"halt\\\"\""));
    }
}

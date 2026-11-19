//! RFC 9457 problem details bodies.

use serde_json::{Value, json};

pub const CONTENT_TYPE: &str = "application/problem+json";

pub fn body(status: u16, title: &str, detail: &str) -> Value {
    json!({"type": "about:blank", "title": title, "status": status, "detail": detail})
}

#[cfg(test)]
mod tests {
    #[test]
    fn shape() {
        let b = super::body(404, "Not found", "no such lease");
        assert_eq!(b["status"], 404);
        assert_eq!(b["type"], "about:blank");
    }
}
